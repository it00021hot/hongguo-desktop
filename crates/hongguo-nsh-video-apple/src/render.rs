use crate::{ImportedFrame, MetalSurface, OutputError, controller::Viewport};
use hongguo_enhancement::Pipeline;
use hongguo_nsh_video_api::{Enhancement, ErrorCode, ResizeMode};
use objc2::rc::Retained;
use objc2_quartz_core::CAMetalLayer;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

pub(crate) struct Completed {
    pub epoch: u64,
    pub elapsed: Duration,
    pub displayed: bool,
    #[cfg(feature = "test-support")]
    pub presentation_timing: Option<crate::diagnostics::PresentationTiming>,
}

pub(crate) struct Renderer {
    surface: MetalSurface,
    pipeline: Option<Pipeline>,
    options: Option<Enhancement>,
    viewport: Viewport,
    finished: Arc<AtomicBool>,
    presentation: Arc<AtomicU8>,
    gpu_elapsed: Arc<Mutex<Option<Duration>>>,
    started: Option<(u64, Instant)>,
    failure: Arc<Mutex<Option<String>>>,
    converter: hongguo_video_gpu::YuvConverter,
    pending_conversion: Option<wgpu::CommandBuffer>,
    #[cfg(feature = "test-support")]
    presentation_timing: Arc<Mutex<Option<crate::diagnostics::PresentationTiming>>>,
}

impl Renderer {
    pub async fn new(layer: Retained<CAMetalLayer>) -> Result<Self, OutputError> {
        let surface = MetalSurface::new(layer).await?;
        let failure = Arc::new(Mutex::new(None));
        let lost = failure.clone();
        surface
            .device()
            .set_device_lost_callback(move |reason, detail| {
                *lost.lock().expect("GPU failure mailbox") = Some(format!("{reason:?}: {detail}"));
            });
        let uncaptured = failure.clone();
        surface
            .device()
            .on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
                let detail = error.to_string();
                *uncaptured.lock().expect("GPU failure mailbox") = Some(detail);
            }));
        let converter = hongguo_video_gpu::YuvConverter::new(surface.device(), surface.queue());
        Ok(Self {
            surface,
            pipeline: None,
            options: None,
            viewport: Viewport::default(),
            finished: Arc::new(AtomicBool::new(true)),
            presentation: Arc::new(AtomicU8::new(1)),
            gpu_elapsed: Arc::new(Mutex::new(None)),
            started: None,
            failure,
            converter,
            pending_conversion: None,
            #[cfg(feature = "test-support")]
            presentation_timing: Arc::new(Mutex::new(None)),
        })
    }

    pub fn poll(&mut self) -> Result<Option<Completed>, OutputError> {
        self.surface
            .device()
            .poll(wgpu::PollType::Poll)
            .map_err(gpu_error)?;
        if let Some(detail) = self.failure.lock().expect("GPU failure mailbox").take() {
            return Err(gpu_error(detail));
        }
        if !self.finished.load(Ordering::Acquire) || self.presentation.load(Ordering::Acquire) == 0
        {
            return Ok(None);
        }
        Ok(self.started.take().map(|(epoch, started)| Completed {
            epoch,
            elapsed: self
                .gpu_elapsed
                .lock()
                .expect("GPU completion duration")
                .take()
                .unwrap_or_else(|| started.elapsed()),
            displayed: self.presentation.load(Ordering::Acquire) == 1,
            #[cfg(feature = "test-support")]
            presentation_timing: self
                .presentation_timing
                .lock()
                .expect("native presentation timing")
                .take(),
        }))
    }
    pub fn idle(&self) -> bool {
        self.finished.load(Ordering::Acquire) && self.started.is_none()
    }
    pub fn gpu_pending(&self) -> bool {
        !self.finished.load(Ordering::Acquire)
    }

    pub async fn draw(
        &mut self,
        frame: &ImportedFrame,
        viewport: Viewport,
        options: Enhancement,
        mode: ResizeMode,
        epoch: u64,
        valid: impl Fn() -> bool,
    ) -> Result<bool, OutputError> {
        if !self.finished.load(Ordering::Acquire) || self.started.is_some() {
            return Ok(false);
        }
        if self.viewport != viewport {
            self.surface.resize(viewport.width_px, viewport.height_px);
            self.viewport = viewport;
        }
        if !valid() {
            return Ok(false);
        }
        // Do not prepare/encode CNN work while the native surface is occluded.
        // Acquisition itself is bounded even if UIKit detaches the view meanwhile.
        let Some(target) = self.surface.acquire()? else {
            return Ok(false);
        };
        let size = frame.metadata();
        let enhance = options.anime4k_on || options.sharpen_on;
        if !enhance {
            self.pipeline = None;
            self.options = None;
        } else if self
            .pipeline
            .as_ref()
            .is_none_or(|pipeline| pipeline.input_size() != (size.width, size.height))
        {
            self.pipeline = Some(
                Pipeline::new(
                    self.surface.device(),
                    self.surface.queue(),
                    size.width,
                    size.height,
                )
                .await
                .map_err(enhancement_error)?,
            );
            self.options = None;
        }
        if let Some(pipeline) = &mut self.pipeline
            && self.options != Some(options)
        {
            pipeline.prepare(options).await.map_err(enhancement_error)?;
            self.options = Some(options);
        }
        if !valid() {
            return Ok(false);
        }
        let started = Instant::now();
        let device = self.surface.device();
        let mut encoder = device.create_command_encoder(&Default::default());
        let enhanced = if let Some(pipeline) = &self.pipeline {
            pipeline
                .encode(
                    &mut encoder,
                    &frame.texture().create_view(&Default::default()),
                )
                .map_err(enhancement_error)?
                .texture()
                .clone()
        } else {
            frame.texture().clone()
        };
        let output = if let Some(descriptor) = frame.source_descriptor() {
            self.converter
                .map_output(
                    &mut encoder,
                    &enhanced,
                    &descriptor,
                    viewport.color_target(),
                )
                .map_err(frame_error)?
        } else {
            enhanced
        };
        self.surface.encode_present(
            &mut encoder,
            &target,
            &output.create_view(&Default::default()),
            frame.aspect_ratio(),
            mode,
        )?;
        let queue = self.surface.queue();
        self.finished.store(false, Ordering::Release);
        self.presentation.store(0, Ordering::Release);
        queue.submit(
            self.pending_conversion
                .take()
                .into_iter()
                .chain(std::iter::once(encoder.finish())),
        );
        let completed = self.finished.clone();
        let gpu_elapsed = self.gpu_elapsed.clone();
        let retirement = Mutex::new(Some(frame.clone()));
        let on_completed = move || {
            retirement.lock().expect("Metal frame retirement").take();
            *gpu_elapsed.lock().expect("GPU completion duration") = Some(started.elapsed());
            completed.store(true, Ordering::Release);
        };
        let presentation = self.presentation.clone();
        #[cfg(feature = "test-support")]
        let timing = self.presentation_timing.clone();
        self.surface.present(
            target,
            move |time| {
                #[cfg(feature = "test-support")]
                {
                    let callback_time_secs = crate::diagnostics::current_media_time();
                    *timing.lock().expect("native presentation timing") =
                        Some(crate::diagnostics::PresentationTiming {
                            presented_time_secs: time,
                            callback_time_secs,
                        });
                }
                presentation.store(if time > 0.0 { 1 } else { 2 }, Ordering::Release);
            },
            on_completed,
        )?;
        self.started = Some((epoch, started));
        Ok(true)
    }

    pub fn upload(&self, frame: &hongguo_nsh_video_api::frames::PixelFrame) -> ImportedFrame {
        ImportedFrame::upload(&self.surface.device(), &self.surface.queue(), frame)
    }

    pub fn upload_native(
        &mut self,
        frame: Arc<hongguo_media_types::PresentationFrame>,
        target: hongguo_video_color::DisplayTarget,
    ) -> Result<ImportedFrame, OutputError> {
        let input = match &frame.image {
            hongguo_media_types::FrameImage::Cpu(cpu) => {
                self.converter.upload(cpu).map_err(frame_error)
            }
            hongguo_media_types::FrameImage::Hardware(_) => {
                crate::native::import(&self.surface.device(), &frame).map_err(OutputError::surface)
            }
        }?;
        let mut encoder = self
            .surface
            .device()
            .create_command_encoder(&Default::default());
        let converted = self
            .converter
            .convert(&mut encoder, input, target)
            .map_err(frame_error)?;
        // Submit conversion with the eventual display work. If the source is
        // replaced before drawing, no GPU read was queued against its surface.
        self.pending_conversion = Some(encoder.finish());
        Ok(ImportedFrame::converted(frame, converted))
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // This renderer lives on its dedicated worker. Drive retirement even
        // after a source/window is detached; hardware surfaces must outlive GPU
        // reads and queued callbacks must not retain a device indefinitely.
        if self
            .surface
            .device()
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(2)),
            })
            .is_err()
        {
            self.surface.device().destroy();
            let _ = self.surface.device().poll(wgpu::PollType::Poll);
        }
    }
}

fn gpu_error(error: impl std::fmt::Display) -> OutputError {
    OutputError::new(ErrorCode::GpuFailed, error.to_string())
}

fn frame_error(error: impl std::fmt::Display) -> OutputError {
    OutputError::new(ErrorCode::FrameFormatUnsupported, error.to_string())
}

fn enhancement_error(error: hongguo_enhancement::Error) -> OutputError {
    let code = if matches!(error, hongguo_enhancement::Error::InputTooLarge) {
        ErrorCode::InputTooLarge
    } else {
        ErrorCode::GpuFailed
    };
    OutputError::new(code, error.to_string())
}
