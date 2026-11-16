use crate::{ImportedFrame, MetalSurface, OutputError, controller::Viewport};
use hongguo_nsh_video_api::{Enhancement, ErrorCode, ResizeMode};
use hongguo_video_gpu::enhancement::Pipeline;
use objc2::rc::Retained;
use objc2_quartz_core::CAMetalLayer;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    time::{Duration, Instant},
};

const UPLOAD_CHUNK_BYTES: u64 = 4 * 1024 * 1024;
const CADENCE_INTERVAL: Duration = Duration::from_secs(1);

pub(crate) struct Completed {
    pub submission: Submission,
    pub elapsed: Duration,
}

#[derive(Clone, Copy)]
pub(crate) struct Submission {
    pub epoch: u64,
    pub generation: u64,
    pub serial: u64,
    pub display_deadline: f64,
}

struct PendingGpu {
    submission: Submission,
    started: Instant,
    elapsed: Arc<Mutex<Option<Duration>>>,
}

#[derive(Default)]
struct PresentationCounters {
    submitted: u64,
    completed: u64,
    presented: u64,
    last_presented: f64,
    max_gap: f64,
}

pub(crate) struct Cadence {
    pub submitted_fps: f64,
    pub completed_fps: f64,
    pub presented_fps: f64,
    pub max_gap_ms: f64,
    pub gpu_bytes: Option<usize>,
}

pub(crate) struct Renderer {
    surface: MetalSurface,
    wake: SyncSender<()>,
    pipeline: Option<Pipeline>,
    options: Option<Enhancement>,
    viewport: Viewport,
    finished: Arc<AtomicBool>,
    pending_gpu: Option<PendingGpu>,
    counters: Arc<Mutex<PresentationCounters>>,
    cadence_at: Instant,
    converter: hongguo_video_gpu::YuvConverter,
    pending_conversion: Option<wgpu::CommandBuffer>,
    pending_target: Option<crate::surface::SurfaceTarget>,
    uploads: wgpu::util::StagingBelt,
    cpu_input: Option<hongguo_video_gpu::YuvInput>,
    converted_output: Option<wgpu::Texture>,
    mapped_output: Option<wgpu::Texture>,
}

impl Renderer {
    pub async fn new(
        layer: Retained<CAMetalLayer>,
        wake: SyncSender<()>,
    ) -> Result<Self, OutputError> {
        let surface = MetalSurface::new(layer).await?;
        let converter = surface.converter();
        let uploads = wgpu::util::StagingBelt::new((*surface.device()).clone(), UPLOAD_CHUNK_BYTES);
        Ok(Self {
            surface,
            wake,
            pipeline: None,
            options: None,
            viewport: Viewport::default(),
            finished: Arc::new(AtomicBool::new(true)),
            pending_gpu: None,
            counters: Arc::new(Mutex::new(PresentationCounters::default())),
            cadence_at: Instant::now(),
            converter,
            pending_conversion: None,
            pending_target: None,
            uploads,
            cpu_input: None,
            converted_output: None,
            mapped_output: None,
        })
    }

    pub fn poll(&mut self) -> Result<Option<Completed>, OutputError> {
        self.surface
            .device()
            .poll(wgpu::PollType::Poll)
            .map_err(gpu_error)?;
        self.surface.check()?;
        if !self.finished.load(Ordering::Acquire) {
            return Ok(None);
        }
        let Some(pending) = self.pending_gpu.take() else {
            return Ok(None);
        };
        let elapsed = pending
            .elapsed
            .lock()
            .expect("GPU completion duration")
            .take()
            .unwrap_or_else(|| pending.started.elapsed());
        Ok(Some(Completed {
            submission: pending.submission,
            elapsed,
        }))
    }

    pub fn take_cadence(&mut self) -> Option<Cadence> {
        let elapsed = self.cadence_at.elapsed();
        if elapsed < CADENCE_INTERVAL {
            return None;
        }
        let gpu_bytes = self.surface.allocated_bytes();
        let mut counters = self.counters.lock().expect("video presentation counters");
        let seconds = elapsed.as_secs_f64();
        let cadence = Cadence {
            submitted_fps: counters.submitted as f64 / seconds,
            completed_fps: counters.completed as f64 / seconds,
            presented_fps: counters.presented as f64 / seconds,
            max_gap_ms: counters.max_gap * 1000.0,
            gpu_bytes,
        };
        counters.submitted = 0;
        counters.completed = 0;
        counters.presented = 0;
        counters.max_gap = 0.0;
        self.cadence_at = Instant::now();
        Some(cadence)
    }

    pub fn visibility_changed(&mut self) {
        self.pending_target.take();
        if self.pending_conversion.take().is_some() {
            self.uploads.recall();
        }
        *self.counters.lock().expect("video presentation counters") =
            PresentationCounters::default();
        self.cadence_at = Instant::now();
    }
    pub fn idle(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
    pub fn can_draw(&self) -> bool {
        !self.gpu_pending()
    }
    pub fn gpu_pending(&self) -> bool {
        !self.finished.load(Ordering::Acquire)
    }

    pub fn prepare_frame(&mut self, viewport: Viewport) -> Result<bool, OutputError> {
        if !self.can_draw() {
            return Ok(false);
        }
        if self.viewport != viewport {
            self.pending_target.take();
            self.surface.resize(viewport.width_px, viewport.height_px);
            self.viewport = viewport;
        }
        if self.pending_target.is_none() {
            self.pending_target = self.surface.acquire()?;
        }
        Ok(self.pending_target.is_some())
    }

    pub async fn draw(
        &mut self,
        frame: &ImportedFrame,
        viewport: Viewport,
        options: Enhancement,
        mode: ResizeMode,
        submission: Submission,
        valid: impl Fn() -> bool,
    ) -> Result<bool, OutputError> {
        if !valid() {
            return Ok(false);
        }
        if !self.prepare_frame(viewport)? {
            return Ok(false);
        }
        let target = self.pending_target.take().expect("prepared Metal drawable");
        let size = frame.metadata();
        let enhance = options.anime4k_on || options.sharpen_on;
        if !enhance {
            self.pipeline = None;
            self.options = None;
            self.mapped_output = None;
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
            pipeline
                .prepare(hongguo_video_gpu::enhancement::Settings {
                    anime4k_on: options.anime4k_on,
                    sharpen_on: options.sharpen_on,
                    strength: options.strength,
                })
                .await
                .map_err(enhancement_error)?;
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
        let output = if frame.needs_output_mapping()
            && let Some(descriptor) = frame.source_descriptor()
        {
            let output = self
                .converter
                .map_output_reusing(
                    &mut encoder,
                    &enhanced,
                    &descriptor,
                    viewport.color_target(),
                    self.mapped_output.take(),
                )
                .map_err(frame_error)?;
            self.mapped_output = Some(output.clone());
            output
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
        self.finished = Arc::new(AtomicBool::new(false));
        let elapsed = Arc::new(Mutex::new(None));
        self.pending_gpu = Some(PendingGpu {
            submission,
            started,
            elapsed: elapsed.clone(),
        });
        self.counters
            .lock()
            .expect("video presentation counters")
            .submitted += 1;
        queue.submit(
            self.pending_conversion
                .take()
                .into_iter()
                .chain(std::iter::once(encoder.finish())),
        );
        self.uploads.recall();
        let completed = self.finished.clone();
        let gpu_elapsed = elapsed.clone();
        let retirement = Mutex::new(Some(frame.clone()));
        let completion_wake = self.wake.clone();
        let completion_counters = self.counters.clone();
        let on_completed = move || {
            retirement.lock().expect("Metal frame retirement").take();
            *gpu_elapsed.lock().expect("GPU completion duration") = Some(started.elapsed());
            completed.store(true, Ordering::Release);
            completion_counters
                .lock()
                .expect("video presentation counters")
                .completed += 1;
            let _ = completion_wake.try_send(());
        };
        let presentation = self.counters.clone();
        self.surface.present(
            target,
            submission.display_deadline,
            move |time| {
                if time <= 0.0 {
                    return;
                }
                let mut counters = presentation.lock().expect("video presentation counters");
                counters.presented += 1;
                if counters.last_presented > 0.0 && time > counters.last_presented {
                    counters.max_gap = counters.max_gap.max(time - counters.last_presented);
                }
                counters.last_presented = counters.last_presented.max(time);
            },
            on_completed,
        )?;
        Ok(true)
    }

    pub fn upload(&self, frame: &hongguo_nsh_video_api::frames::PixelFrame) -> ImportedFrame {
        ImportedFrame::upload(&self.surface.device(), &self.surface.queue(), frame)
    }

    pub fn upload_native(
        &mut self,
        frame: Arc<hongguo_media_types::PresentationFrame>,
        target: hongguo_video_color::DisplayTarget,
        enhancement: Enhancement,
    ) -> Result<ImportedFrame, OutputError> {
        if self.pending_conversion.take().is_some() {
            self.uploads.recall();
            self.surface
                .device()
                .poll(wgpu::PollType::Poll)
                .map_err(gpu_error)?;
        }
        let mut encoder = self
            .surface
            .device()
            .create_command_encoder(&Default::default());
        let input = match &frame.image {
            hongguo_media_types::FrameImage::Cpu(cpu) => self
                .converter
                .upload_staged(&mut encoder, &mut self.uploads, cpu, self.cpu_input.take())
                .inspect(|input| {
                    self.cpu_input = Some(input.clone());
                })
                .map_err(frame_error),
            hongguo_media_types::FrameImage::Hardware(_) => {
                crate::native::import(&self.surface.device(), &frame).map_err(OutputError::surface)
            }
        }?;
        let previous = self.converted_output.take();
        let converted = if enhancement.anime4k_on || enhancement.sharpen_on {
            self.converter
                .convert_reusing(&mut encoder, input, target, previous)
        } else {
            self.converter
                .convert_output_reusing(&mut encoder, input, target, previous)
        }
        .map_err(frame_error)?;
        self.converted_output = Some(converted.texture.clone());
        self.uploads.finish();
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
            self.surface
                .invalidate("GPU retirement did not complete".into());
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

fn enhancement_error(error: hongguo_video_gpu::enhancement::Error) -> OutputError {
    let code = if matches!(error, hongguo_video_gpu::enhancement::Error::InputTooLarge) {
        ErrorCode::InputTooLarge
    } else {
        ErrorCode::GpuFailed
    };
    OutputError::new(code, error.to_string())
}
