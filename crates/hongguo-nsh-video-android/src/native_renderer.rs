use crate::{
    bridge::{Bridge, error},
    decoder_surface::DecoderSurface,
    raw_yuv::{ImportedImage, RawYuv},
    surface::OutputSurface,
};
use hongguo_enhancement::Pipeline;
use hongguo_media_types::{FrameImage, PresentationFrame};
use hongguo_nsh_video_api::{
    Enhancement, ErrorCode, ResizeMode,
    frames::{FrameLease, PixelFrame},
};
use hongguo_video_gpu::{ConvertedFrame, YuvConverter};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(crate) enum RenderFrame {
    Native(Arc<PresentationFrame>),
    Preview(Arc<PixelFrame>),
}
#[derive(Clone, Copy)]
pub(crate) struct FrameStamp {
    pub generation: u64,
    pub serial: u64,
    pub pts_ms: u64,
    pub width: u32,
    pub height: u32,
}
impl RenderFrame {
    pub fn stamp(&self) -> FrameStamp {
        match self {
            Self::Native(frame) => {
                let desc = frame.image.descriptor();
                let (width, height) = if matches!(desc.rotation_degrees, 90 | 270) {
                    (desc.crop.height, desc.crop.width)
                } else {
                    (desc.crop.width, desc.crop.height)
                };
                FrameStamp {
                    generation: frame.generation,
                    serial: frame.serial,
                    pts_ms: (frame.pts_secs.max(0.) * 1000.).round() as u64,
                    width: width as u32,
                    height: height as u32,
                }
            }
            Self::Preview(frame) => FrameStamp {
                generation: frame.generation,
                serial: frame.serial,
                pts_ms: frame.pts_ms,
                width: frame.width,
                height: frame.height,
            },
        }
    }
}
pub(crate) struct Completed {
    pub elapsed: Duration,
    pub presented: bool,
    pub settings: Enhancement,
    pub frame: FrameStamp,
    pub token: i64,
    pub swap_succeeded: bool,
    pub output_version: u64,
}
struct Submission {
    at: Instant,
    presented: bool,
    settings: Enhancement,
    frame: FrameStamp,
    token: i64,
    swap_succeeded: bool,
    output_version: u64,
    retirement: Arc<Mutex<Option<RetainedImage>>>,
}
struct RetainedImage {
    _frame: RenderFrame,
    _image: Option<ImportedImage>,
}
struct Pending {
    frame: Arc<PresentationFrame>,
    timestamp: i64,
    at: Instant,
}

pub(crate) struct Renderer {
    pub surface: OutputSurface,
    bridge: Bridge,
    direct: Option<crate::direct_texture::DirectTexture>,
    converter: YuvConverter,
    converted: Option<ConvertedFrame>,
    frame: Option<RenderFrame>,
    source: Option<Arc<DecoderSurface>>,
    bound: bool,
    raw: Option<RawYuv>,
    pending: Option<Pending>,
    pipeline: Option<(Pipeline, (u32, u32))>,
    options: Enhancement,
    failure: Arc<Mutex<Option<String>>>,
    finished: Arc<AtomicBool>,
    started: Option<Submission>,
    source_hdr: Option<bool>,
    epoch: u64,
    output_version: u64,
}
impl Renderer {
    pub async fn new(
        viewport: &crate::controller::Viewport,
        size: (u32, u32),
        options: Enhancement,
        bridge: Bridge,
        lease: &FrameLease,
        epoch: u64,
    ) -> Result<Self, (ErrorCode, String)> {
        let window = viewport
            .window
            .clone()
            .ok_or_else(|| gpu("output window is unavailable"))?;
        let surface = OutputSurface::new(
            window,
            viewport.width,
            viewport.height,
            viewport.hdr_peak,
            viewport.float_output,
        )
        .await
        .map_err(|detail| (ErrorCode::GpuUnavailable, detail))?;
        let limit = surface.device.limits().max_texture_dimension_2d;
        if size.0 == 0 || size.1 == 0 || size.0 > limit || size.1 > limit {
            return Err((
                ErrorCode::InputTooLarge,
                "video dimensions exceed the GLES texture limit".into(),
            ));
        }
        let failure = Arc::new(Mutex::new(None));
        let lost = failure.clone();
        surface
            .device
            .set_device_lost_callback(move |reason, message| {
                *lost.lock().expect("Android GPU failure") = Some(format!("{reason:?}: {message}"));
            });
        let invalid = failure.clone();
        surface.device.on_uncaptured_error(Arc::new(move |error| {
            *invalid.lock().expect("Android GPU failure") = Some(error.to_string());
        }));
        let candidate = DecoderSurface::new(surface.device.clone(), &bridge, size, false)
            .and_then(|source| RawYuv::new(surface.device.clone()).map(|raw| (source, raw)));
        let (source, raw, bound) = match candidate {
            Ok((source, raw)) => {
                let bound = lease
                    .configure_decoder_if_owner(Some(source.clone()))
                    .map_err(gpu)?;
                (Some(source), Some(raw), bound)
            }
            Err(detail) => {
                bridge.clear_exception();
                log::info!("[Android video] using native software frames: {detail}");
                let bound = lease.configure_decoder_if_owner(None).map_err(gpu)?;
                (None, None, bound)
            }
        };
        let converter = YuvConverter::new(surface.device.clone(), surface.queue.clone());
        Ok(Self {
            surface,
            bridge,
            direct: None,
            converter,
            converted: None,
            frame: None,
            source,
            bound,
            raw,
            pending: None,
            pipeline: None,
            options,
            failure,
            finished: Arc::new(AtomicBool::new(true)),
            started: None,
            source_hdr: None,
            epoch,
            output_version: viewport.version,
        })
    }
    pub fn has_decoder_surface(&self) -> bool {
        self.source.is_some()
    }
    pub fn binding_lost(&mut self) {
        self.bound = false;
        self.pending = None;
    }
    pub fn clear_media(&mut self) {
        self.frame = None;
        self.converted = None;
        self.pending = None;
    }
    pub fn output_changed(&mut self, version: u64) {
        self.output_version = version;
        if let Some(submission) = &mut self.started {
            submission.presented = false;
        }
    }
    pub fn bind_if_needed(&mut self, lease: &FrameLease) -> Result<bool, (ErrorCode, String)> {
        if !self.bound {
            let device = self
                .source
                .clone()
                .map(|source| source as Arc<dyn hongguo_media_types::DecodeDevice>);
            self.bound = lease.configure_decoder_if_owner(device).map_err(gpu)?;
        }
        Ok(self.bound)
    }
    pub fn ready(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
    pub fn poll(&mut self) -> Result<Option<Completed>, (ErrorCode, String)> {
        self.surface
            .device
            .poll(wgpu::PollType::Poll)
            .map_err(gpu)?;
        if let Some(error) = self.failure.lock().expect("Android GPU failure").take() {
            return Err(gpu(error));
        }
        if !self.ready() {
            return Ok(None);
        }
        Ok(self.started.take().map(|s| Completed {
            elapsed: s.at.elapsed(),
            presented: s.presented,
            settings: s.settings,
            frame: s.frame,
            token: s.token,
            swap_succeeded: s.swap_succeeded,
            output_version: s.output_version,
        }))
    }

    fn hardware_input(
        &mut self,
        frame: Arc<PresentationFrame>,
        lease: &FrameLease,
    ) -> Result<Option<(hongguo_video_gpu::YuvInput, Option<ImportedImage>)>, String> {
        let descriptor = frame.image.descriptor();
        let source = self
            .source
            .as_ref()
            .ok_or("hardware frame has no decoder surface lease")?
            .clone();
        if descriptor.bit_depth == 10 && !source.p010 {
            // An opaque SurfaceTexture cannot certify 10-bit storage. Rebind once
            // to a verifiable P010 ImageReader; this new device generation consumes
            // the shared session's cumulative recovery budget.
            match DecoderSurface::new(
                self.surface.device.clone(),
                &self.bridge,
                (descriptor.width as u32, descriptor.height as u32),
                true,
            ) {
                Ok(p010) => {
                    if !lease.configure_decoder_if_owner(Some(p010.clone()))? {
                        return Ok(None);
                    }
                    self.source = Some(p010);
                    self.pending = None;
                    return Ok(None);
                }
                Err(error) => {
                    self.bridge.clear_exception();
                    return Err(format!("10-bit GPU output is unavailable: {error}"));
                }
            }
        }
        let stamp = (frame.generation, frame.serial);
        if self
            .pending
            .as_ref()
            .is_none_or(|p| (p.frame.generation, p.frame.serial) != stamp)
        {
            let timestamp = source.timestamp()?;
            let FrameImage::Hardware(hardware) = &frame.image else {
                return Err("expected a hardware frame".into());
            };
            hardware.present_at(timestamp)?;
            self.pending = Some(Pending {
                frame: frame.clone(),
                timestamp,
                at: Instant::now(),
            });
        }
        let pending = self.pending.as_ref().expect("pending hardware image");
        if pending.at.elapsed() > Duration::from_secs(2) {
            return Err("decoder surface did not deliver the requested image".into());
        }
        let (timestamp, matrix, image) = if source.p010 {
            let Some(image) = source.acquire()? else {
                return Ok(None);
            };
            let timestamp = image.timestamp()?;
            let crop = image.crop()?;
            if crop.iter().any(|value| *value < 0) {
                return Err("negative P010 crop".into());
            }
            let visible = descriptor.crop;
            let matrix = crate::surface_geometry::p010_transform(
                image.dimensions()?,
                crop.map(|value| value as usize),
                [visible.x, visible.y, visible.width, visible.height],
                [descriptor.width, descriptor.height],
            )?;
            (timestamp, matrix, Some(image))
        } else {
            // SAFETY: SurfaceTexture.updateTexImage runs under its owning context.
            let hal = unsafe { self.surface.device.as_hal::<wgpu::hal::gles::Api>() }
                .ok_or("GLES device required")?;
            let _gl = hal.context().lock();
            // SAFETY: updateTexImage may mutate external texture bindings.
            let _state = unsafe { crate::gl_state::State::capture(&_gl) };
            let Some((timestamp, matrix)) = source.update()? else {
                return Ok(None);
            };
            let crop = descriptor.crop;
            let matrix = crate::surface_geometry::expand_crop(
                matrix,
                [crop.x, crop.y, crop.width, crop.height],
                [descriptor.width, descriptor.height],
            )?;
            (timestamp, matrix, None)
        };
        if timestamp < pending.timestamp {
            return Ok(None);
        }
        if timestamp != pending.timestamp {
            return Err("decoder image timestamp does not match the pending frame epoch".into());
        }
        let result = self
            .raw
            .as_ref()
            .ok_or("raw YUV import pipeline is unavailable")?
            .extract(&source, descriptor, matrix, image)?;
        self.pending = None;
        Ok(Some(result))
    }

    pub async fn draw(
        &mut self,
        dirty: bool,
        options: Enhancement,
        mode: ResizeMode,
        mut input: Option<RenderFrame>,
        lease: &FrameLease,
    ) -> Result<bool, (ErrorCode, String)> {
        if !self.ready() {
            return Ok(false);
        }
        if let Some(pending) = &self.pending
            && input
                .as_ref()
                .is_some_and(|input| input.stamp().generation == pending.frame.generation)
        {
            // Finish the one released image before sampling another buffer from
            // this epoch. A seek's new epoch may replace it immediately.
            input = Some(RenderFrame::Native(pending.frame.clone()));
        }
        let mut imported = None;
        let mut changed = false;
        let mut encoder = self
            .surface
            .device
            .create_command_encoder(&Default::default());
        if let Some(input) = input {
            if let RenderFrame::Native(frame) = &input {
                let hdr = matches!(frame.image.descriptor().color.transfer, Some(16 | 18));
                if self.source_hdr != Some(hdr) {
                    self.bridge.source_color(self.epoch, hdr).map_err(gpu)?;
                    self.source_hdr = Some(hdr);
                }
            }
            let stamp = input.stamp();
            changed = self.frame.as_ref().is_none_or(|old| {
                let old = old.stamp();
                (old.generation, old.serial) != (stamp.generation, stamp.serial)
            });
            if changed {
                match &input {
                    RenderFrame::Native(frame) => {
                        let planes = match &frame.image {
                            FrameImage::Cpu(cpu) => {
                                Some((self.converter.upload(cpu).map_err(format_error)?, None))
                            }
                            FrameImage::Hardware(_) => {
                                match self.hardware_input(frame.clone(), lease) {
                                    Ok(value) => value,
                                    Err(detail) => {
                                        self.bridge.clear_exception();
                                        self.pending = None;
                                        lease.reject_decoder_if_owner(&detail).map_err(gpu)?;
                                        return Ok(false);
                                    }
                                }
                            }
                        };
                        let Some((planes, image)) = planes else {
                            return Ok(false);
                        };
                        imported = image;
                        self.converted = Some(
                            self.converter
                                .convert(&mut encoder, planes, self.surface.target())
                                .map_err(format_error)?,
                        );
                    }
                    RenderFrame::Preview(frame) => {
                        self.direct
                            .get_or_insert_with(|| {
                                crate::direct_texture::DirectTexture::new(
                                    &self.surface.device,
                                    frame.width,
                                    frame.height,
                                )
                            })
                            .upload(&self.surface.queue, frame.clone())
                            .map_err(gpu)?;
                        self.converted = None;
                    }
                }
                self.frame = Some(input);
            }
        }
        let Some(frame) = self.frame.clone() else {
            return Ok(false);
        };
        if !(dirty || changed) {
            return Ok(false);
        }
        let started = Instant::now();
        let native_descriptor = self
            .converted
            .as_ref()
            .map(|converted| converted.descriptor.clone());
        let source_descriptor = match &frame {
            RenderFrame::Native(frame) => Some(frame.image.descriptor()),
            _ => None,
        };
        let texture = match (&self.converted, &self.direct) {
            (Some(converted), _) => &converted.texture,
            (_, Some(direct)) => &direct.texture,
            _ => return Err(gpu("no image texture was prepared")),
        };
        let size = (texture.width(), texture.height());
        let result = async {
            let enhanced = if options.anime4k_on || options.sharpen_on {
                if self.pipeline.as_ref().is_none_or(|(_, old)| *old != size) {
                    let mut pipeline = Pipeline::new(
                        self.surface.device.clone(),
                        self.surface.queue.clone(),
                        size.0,
                        size.1,
                    )
                    .await
                    .map_err(enhancement)?;
                    pipeline.prepare(options).await.map_err(enhancement)?;
                    self.pipeline = Some((pipeline, size));
                } else if options != self.options {
                    self.pipeline
                        .as_mut()
                        .expect("enhancement initialized")
                        .0
                        .prepare(options)
                        .await
                        .map_err(enhancement)?;
                }
                Some(
                    self.pipeline
                        .as_mut()
                        .expect("enhancement initialized")
                        .0
                        .encode(&mut encoder, &texture.create_view(&Default::default()))
                        .map_err(enhancement)?,
                )
            } else {
                None
            };
            self.options = options;
            let working = enhanced.as_ref().map_or(texture, |output| output.texture());
            let mapped = source_descriptor
                .as_ref()
                .map(|descriptor| {
                    self.converter.map_output(
                        &mut encoder,
                        working,
                        descriptor,
                        self.surface.target(),
                    )
                })
                .transpose()
                .map_err(format_error)?;
            let ratio = native_descriptor.as_ref().map(|descriptor| {
                let (num, den) = descriptor.color.sample_aspect_ratio.unwrap_or((1, 1));
                descriptor.width as f32 / descriptor.height as f32 * num as f32 / den.max(1) as f32
            });
            self.surface
                .encode(
                    &mut encoder,
                    mapped.as_ref().unwrap_or(working),
                    mode,
                    ratio,
                )
                .map_err(gpu)
        }
        .await;
        let presented = result.as_ref().is_ok_and(|target| target.is_some());
        let token = if presented {
            let token = self.bridge.timestamp().map_err(gpu)?;
            self.surface.set_presentation_time(token).map_err(gpu)?;
            token
        } else {
            0
        };
        self.finished.store(false, Ordering::Release);
        self.surface.queue.submit([encoder.finish()]);
        let finished = self.finished.clone();
        let stamp = frame.stamp();
        let retirement = Arc::new(Mutex::new(Some(RetainedImage {
            _frame: frame,
            _image: imported,
        })));
        let retired = retirement.clone();
        self.surface.queue.on_submitted_work_done(move || {
            // This callback owns external Android image leases even when the
            // renderer is retired while a command buffer is still in flight.
            retired.lock().expect("Android GPU retirement").take();
            finished.store(true, Ordering::Release);
        });
        self.started = Some(Submission {
            at: started,
            presented,
            settings: options,
            frame: stamp,
            token,
            swap_succeeded: false,
            output_version: self.output_version,
            retirement,
        });
        if let Some(target) = result? {
            target.present();
            self.started
                .as_mut()
                .expect("submitted frame")
                .swap_succeeded = self.surface.swap_succeeded(token);
        }
        Ok(true)
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        if self.ready() {
            return;
        }
        // Retirement is on the dedicated renderer worker, never the UI thread.
        // GLES callbacks need polling even after the visible surface is detached.
        if let Err(error) = self.surface.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(2)),
        }) {
            log::warn!("[Android video] retiring failed GPU: {error}");
            self.surface.device.destroy();
            let _ = self.surface.device.poll(wgpu::PollType::Poll);
            if let Some(submission) = &self.started {
                submission
                    .retirement
                    .lock()
                    .expect("Android GPU retirement")
                    .take();
            }
        }
    }
}
fn gpu(value: impl std::fmt::Display) -> (ErrorCode, String) {
    (ErrorCode::GpuFailed, error(value))
}
fn format_error(value: impl std::fmt::Display) -> (ErrorCode, String) {
    (ErrorCode::FrameFormatUnsupported, error(value))
}
fn enhancement(value: hongguo_enhancement::Error) -> (ErrorCode, String) {
    let code = if matches!(value, hongguo_enhancement::Error::InputTooLarge) {
        ErrorCode::InputTooLarge
    } else {
        ErrorCode::GpuFailed
    };
    (code, value.to_string())
}
