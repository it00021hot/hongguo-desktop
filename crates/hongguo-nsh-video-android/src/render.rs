use crate::{bridge::error, surface::OutputSurface};
use hongguo_enhancement::Pipeline;
use hongguo_nsh_video_api::frames::PixelFrame;
use hongguo_nsh_video_api::{Enhancement, ErrorCode, ResizeMode};
use ndk::native_window::NativeWindow;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(crate) struct Completed {
    pub elapsed: Duration,
    pub presented: bool,
    pub settings: Enhancement,
    pub frame: Option<Arc<PixelFrame>>,
}

struct Submission {
    at: Instant,
    presented: bool,
    settings: Enhancement,
    frame: Option<Arc<PixelFrame>>,
}

pub(crate) struct Renderer {
    pub surface: OutputSurface,
    direct: crate::direct_texture::DirectTexture,
    pipeline: Pipeline,
    options: Enhancement,
    failure: Arc<Mutex<Option<String>>>,
    finished: Arc<AtomicBool>,
    started: Option<Submission>,
}
impl Renderer {
    pub async fn new(
        window: NativeWindow,
        output: (u32, u32),
        size: (u32, u32),
        options: Enhancement,
    ) -> Result<Self, (ErrorCode, String)> {
        let surface = OutputSurface::new(window, output.0, output.1)
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
            *invalid.lock().expect("Android GPU failure") = Some(format!("{error}"));
        }));
        let direct = crate::direct_texture::DirectTexture::new(&surface.device, size.0, size.1);
        let mut pipeline = Pipeline::new(
            surface.device.clone(),
            surface.queue.clone(),
            size.0,
            size.1,
        )
        .await
        .map_err(enhancement)?;
        pipeline.prepare(options).await.map_err(enhancement)?;
        Ok(Self {
            surface,
            direct,
            pipeline,
            options,
            failure,
            finished: Arc::new(AtomicBool::new(true)),
            started: None,
        })
    }
    pub fn settings(&self) -> Enhancement {
        self.options
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
        if !self.finished.load(Ordering::Acquire) {
            return Ok(None);
        }
        Ok(self.started.take().map(|submission| Completed {
            elapsed: submission.at.elapsed(),
            presented: submission.presented,
            settings: submission.settings,
            frame: submission.frame,
        }))
    }
    pub async fn draw(
        &mut self,
        dirty: bool,
        options: Enhancement,
        mode: ResizeMode,
        pixel: Option<Arc<PixelFrame>>,
    ) -> Result<bool, (ErrorCode, String)> {
        if !self.finished.load(Ordering::Acquire) {
            return Ok(false);
        }
        let direct_changed = if let Some(pixel) = pixel {
            self.direct
                .upload(&self.surface.queue, pixel)
                .map_err(gpu)?
        } else {
            false
        };
        let pixels = self.direct.frame.clone();
        if pixels.is_none() || !(dirty || direct_changed) {
            return Ok(false);
        }
        let texture = &self.direct.texture;
        // A lease is always submitted, including a failed prepare/presentation.
        // This retires the exact single-slot image only after all GPU reads.
        let started = Instant::now();
        let mut encoder = self
            .surface
            .device
            .create_command_encoder(&Default::default());
        let result = async {
            if options != self.options {
                self.pipeline.prepare(options).await.map_err(enhancement)?;
                self.options = options;
            }
            let output = self
                .pipeline
                .encode(&mut encoder, &texture.create_view(&Default::default()))
                .map_err(enhancement)?;
            self.surface
                .encode(&mut encoder, output.texture(), mode)
                .map_err(gpu)
        }
        .await;
        let presented = result.as_ref().is_ok_and(|target| target.is_some());
        self.finished.store(false, Ordering::Release);
        self.surface.queue.submit([encoder.finish()]);
        let finished = self.finished.clone();
        self.surface
            .queue
            .on_submitted_work_done(move || finished.store(true, Ordering::Release));
        self.started = Some(Submission {
            at: started,
            presented,
            settings: options,
            frame: pixels,
        });
        if let Some(target) = result? {
            target.present();
        }
        Ok(true)
    }
}
fn gpu(value: impl std::fmt::Display) -> (ErrorCode, String) {
    (ErrorCode::GpuFailed, error(value))
}
fn enhancement(value: hongguo_enhancement::Error) -> (ErrorCode, String) {
    let code = if matches!(value, hongguo_enhancement::Error::InputTooLarge) {
        ErrorCode::InputTooLarge
    } else {
        ErrorCode::GpuFailed
    };
    (code, value.to_string())
}
