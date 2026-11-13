use crate::{ImportedFrame, MetalSurface, OutputError, controller::Viewport};
use hongguo_enhancement::Pipeline;
use hongguo_nsh_video_api::{Enhancement, ErrorCode, ResizeMode};
use objc2::rc::Retained;
use objc2_quartz_core::CAMetalLayer;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(crate) struct Completed {
    pub epoch: u64,
    pub elapsed: Duration,
}

pub(crate) struct Renderer {
    surface: MetalSurface,
    pipeline: Option<Pipeline>,
    options: Option<Enhancement>,
    viewport: Viewport,
    finished: Arc<AtomicBool>,
    started: Option<(u64, Instant)>,
    failure: Arc<Mutex<Option<String>>>,
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
        Ok(Self {
            surface,
            pipeline: None,
            options: None,
            viewport: Viewport::default(),
            finished: Arc::new(AtomicBool::new(true)),
            started: None,
            failure,
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
        if !self.finished.load(Ordering::Acquire) {
            return Ok(None);
        }
        Ok(self.started.take().map(|(epoch, started)| Completed {
            epoch,
            elapsed: started.elapsed(),
        }))
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
        if !self.finished.load(Ordering::Acquire) {
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
        if self
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
        let pipeline = self
            .pipeline
            .as_mut()
            .expect("pipeline initialized for this frame");
        if self.options != Some(options) {
            pipeline.prepare(options).await.map_err(enhancement_error)?;
            self.options = Some(options);
        }
        if !valid() {
            return Ok(false);
        }
        let started = Instant::now();
        let device = self.surface.device();
        let mut encoder = device.create_command_encoder(&Default::default());
        let output = pipeline
            .encode(
                &mut encoder,
                &frame.texture().create_view(&Default::default()),
            )
            .map_err(enhancement_error)?;
        self.surface.encode_present(
            &mut encoder,
            &target,
            &output.texture().create_view(&Default::default()),
            output.width(),
            output.height(),
            mode,
        )?;
        let queue = self.surface.queue();
        self.finished.store(false, Ordering::Release);
        queue.submit([encoder.finish()]);
        frame.clone().retire_after_submitted_work(&queue);
        let completed = self.finished.clone();
        queue.on_submitted_work_done(move || completed.store(true, Ordering::Release));
        target.present();
        self.started = Some((epoch, started));
        Ok(true)
    }

    pub fn upload(&self, frame: &hongguo_nsh_video_api::frames::PixelFrame) -> ImportedFrame {
        ImportedFrame::upload(&self.surface.device(), &self.surface.queue(), frame)
    }
}

fn gpu_error(error: impl std::fmt::Display) -> OutputError {
    OutputError::new(ErrorCode::GpuFailed, error.to_string())
}

fn enhancement_error(error: hongguo_enhancement::Error) -> OutputError {
    let code = if matches!(error, hongguo_enhancement::Error::InputTooLarge) {
        ErrorCode::InputTooLarge
    } else {
        ErrorCode::GpuFailed
    };
    OutputError::new(code, error.to_string())
}
