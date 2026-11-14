use crate::{
    VideoError,
    controller::Controller,
    state::{Shared, validate},
};
use hongguo_nsh_video_api::{
    COMPONENT_NAME, EnhancementPhase, ErrorCode, SEEK_COMMAND, STATE_PROP, Seek, VideoEvent,
    VideoProps,
};
use nsh_platform_windows::{ComponentContext, Components, NativeComponent};
use serde_json::Value;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use windows::Win32::Graphics::Direct2D::ID2D1DeviceContext;

struct PixelVideo {
    shared: Arc<Shared>,
    controller: RefCell<Controller>,
}

impl NativeComponent for PixelVideo {
    fn draw(&self, dc: &ID2D1DeviceContext, size: (f32, f32)) {
        let result = self.controller.borrow_mut().draw(&self.shared, dc, size);
        if let Err(error) = result {
            self.fail(error);
        }
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name != STATE_PROP {
            return;
        }
        let result = (|| {
            let props = match value {
                Some(value) => serde_json::from_value::<VideoProps>(value.clone())
                    .map_err(|error| VideoError::new(ErrorCode::InvalidProps, error.to_string()))?,
                None => VideoProps::default(),
            };
            validate(&props)?;
            let mut current = self
                .shared
                .props
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if *current == props {
                return Ok(());
            }
            self.shared.consumer_active.store(true, Ordering::Release);
            self.controller.borrow_mut().set_props(&props)?;
            let source_changed = current.source != props.source;
            *current = props;
            if source_changed {
                self.shared.clear_frames();
            } else {
                self.shared.invalidate();
            }
            let source_id = current
                .source
                .as_ref()
                .map(|source| source.id.clone())
                .unwrap_or_default();
            let enhanced = current.enhancement.anime4k_on || current.enhancement.sharpen_on;
            let settings = current.enhancement;
            drop(current);
            let startup_error = self
                .shared
                .startup_error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if let Some(error) = startup_error {
                if error.code() != ErrorCode::GpuUnavailable {
                    return Err(error);
                }
                if enhanced {
                    self.shared.emit(VideoEvent::EnhancementStatus {
                        source_id,
                        settings,
                        phase: EnhancementPhase::Error,
                        code: Some(error.code()),
                        detail: error.detail().to_owned(),
                    });
                }
            }
            self.shared.context.redraw();
            Ok(())
        })();
        if let Err(error) = result {
            self.fail(error);
        }
    }

    fn command(&mut self, name: &str, args: &Value) {
        if name != SEEK_COMMAND {
            return;
        }
        let result = (|| {
            let request: Seek = serde_json::from_value(args.clone())
                .map_err(|error| VideoError::new(ErrorCode::InvalidProps, error.to_string()))?;
            let props = self
                .shared
                .props
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if props
                .source
                .as_ref()
                .is_none_or(|source| source.id != request.source_id)
            {
                return Ok(());
            }
            self.shared.consumer_active.store(true, Ordering::Release);
            self.controller.borrow_mut().seek(request, &props)?;
            self.shared.clear_frames();
            self.shared.context.redraw();
            Ok(())
        })();
        if let Err(error) = result {
            self.fail(error);
        }
    }
}

impl PixelVideo {
    fn fail(&self, error: VideoError) {
        self.controller.borrow_mut().stop();
        let source_id = self
            .shared
            .props
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .source
            .as_ref()
            .map(|source| source.id.clone())
            .unwrap_or_default();
        self.shared.emit(VideoEvent::Error {
            source_id,
            code: error.code(),
            detail: error.detail().to_owned(),
        });
        self.shared.consumer_active.store(false, Ordering::Release);
        self.shared.clear_frames();
    }
}

impl Drop for PixelVideo {
    fn drop(&mut self) {
        self.controller.get_mut().stop();
        self.shared.stopped.store(true, Ordering::Release);
        self.shared.wake_gpu.notify_all();
    }
}

pub fn register(components: &Components) {
    components.register(
        COMPONENT_NAME,
        Rc::new(|context: ComponentContext| {
            let shared = Shared::new(context);
            for (name, worker) in [("hongguo-video-gpu", true), ("hongguo-video-frames", false)] {
                let state = shared.clone();
                if let Err(error) = std::thread::Builder::new()
                    .name(name.into())
                    .spawn(move || {
                        if worker {
                            crate::gpu_worker::run(state);
                        } else {
                            while state.reap() || !state.stopped.load(Ordering::Acquire) {
                                let props = state
                                    .props
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .clone();
                                let active = props.active
                                    && props.source.is_some()
                                    && state.consumer_active.load(Ordering::Acquire);
                                if active {
                                    state.context.redraw();
                                }
                                std::thread::sleep(
                                    if !active
                                        || props.paused
                                            && !state.awaiting_presentation.load(Ordering::Acquire)
                                    {
                                        Duration::from_millis(u64::from(
                                            props.progress_interval_ms.min(250),
                                        ))
                                    } else {
                                        Duration::from_millis(16)
                                    },
                                );
                            }
                        }
                    })
                {
                    // Props have not arrived yet. Persist this until the real source
                    // identity is known; a GPU-only failure must retain raw playback.
                    *shared
                        .startup_error
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) =
                        Some(VideoError::new(
                            if worker {
                                ErrorCode::GpuUnavailable
                            } else {
                                ErrorCode::DecoderUnavailable
                            },
                            error.to_string(),
                        ));
                }
            }
            Box::new(PixelVideo {
                shared,
                controller: RefCell::new(Controller::default()),
            })
        }),
    );
}
