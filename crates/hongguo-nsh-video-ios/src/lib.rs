#![cfg(target_os = "ios")]

mod audio;
mod view;

use hongguo_nsh_video_api::{
    COMPONENT_NAME, EVENT_NAME, ErrorCode, SEEK_COMMAND, STATE_PROP, Seek, VideoEvent, VideoProps,
};
use hongguo_nsh_video_apple::{OutputError, PlayerController};
use nsh_platform_ios::{ComponentContext, Components, NativeComponent};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_ui_kit::UIView;
use serde_json::Value;
use std::{
    rc::Rc,
    sync::{Arc, Mutex, Weak},
};

struct PixelVideo {
    view: Retained<view::PixelView>,
    controller: Result<Arc<PlayerController>, OutputError>,
    audio: audio::Lease,
    wants_audio: bool,
    context: ComponentContext,
    source_id: String,
}

impl NativeComponent for PixelVideo {
    fn view(&self) -> &UIView {
        &self.view
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name != STATE_PROP {
            return;
        }
        let state = match value {
            Some(value) => serde_json::from_value::<VideoProps>(value.clone()),
            None => Ok(VideoProps::default()),
        };
        let state = match state {
            Ok(state) => state,
            Err(error) => {
                self.fail(ErrorCode::InvalidProps, error.to_string());
                return;
            }
        };
        self.source_id = state
            .source
            .as_ref()
            .map(|source| source.id.clone())
            .unwrap_or_default();
        self.wants_audio = state.source.is_some() && state.active && !state.paused;
        if self.wants_audio
            && let Err(error) = self.audio.play(&self.source_id)
        {
            self.fail(ErrorCode::DecoderUnavailable, error);
            return;
        }
        let result = self
            .controller
            .as_ref()
            .map_err(|error| (error.code(), error.detail().to_owned()))
            .and_then(|controller| {
                controller
                    .set_props(state)
                    .map_err(|error| (error.code(), error.detail().to_owned()))
            });
        if let Err((code, detail)) = result {
            self.fail(code, detail);
        }
        if !self.wants_audio
            && let Err(error) = self.audio.release()
        {
            log::warn!("could not restore playback audio session after pause/inactive: {error}");
        }
    }

    fn command(&mut self, name: &str, args: &Value) {
        if name != SEEK_COMMAND {
            return;
        }
        let result = serde_json::from_value::<Seek>(args.clone())
            .map_err(|error| (ErrorCode::InvalidProps, error.to_string()))
            .and_then(|request| {
                if request.source_id != self.source_id {
                    return Ok(());
                }
                if self.wants_audio {
                    self.audio
                        .play(&self.source_id)
                        .map_err(|error| (ErrorCode::DecoderUnavailable, error))?;
                }
                self.controller
                    .as_ref()
                    .map_err(|error| (error.code(), error.detail().to_owned()))
                    .and_then(|controller| {
                        controller
                            .seek(request)
                            .map_err(|error| (error.code(), error.detail().to_owned()))
                    })
            });
        if let Err((code, detail)) = result {
            self.fail(code, detail);
        }
    }
}

impl PixelVideo {
    fn fail(&self, code: ErrorCode, detail: String) {
        if let Ok(controller) = &self.controller {
            controller.stop();
        }
        if let Err(error) = self.audio.release() {
            log::warn!("could not restore audio session after native failure: {error}");
        }
        emit(
            &self.context,
            VideoEvent::Error {
                source_id: self.source_id.clone(),
                code,
                detail,
            },
        );
    }
}

impl Drop for PixelVideo {
    fn drop(&mut self) {
        if let Ok(controller) = &self.controller {
            controller.stop();
        }
    }
}

pub fn register(components: &Components) {
    components.register(
        COMPONENT_NAME,
        Rc::new(|context: ComponentContext| {
            let mtm = MainThreadMarker::new().expect("NSH native factory runs on the UIKit thread");
            let view = view::PixelView::new(mtm);
            let audio = audio::Lease::new();
            let key = audio.key();
            let slot = Arc::new(Mutex::new(Weak::new()));
            let terminal = slot.clone();
            let sink = context.clone();
            let controller = view.attach(move |event| {
                if let VideoEvent::End { source_id } | VideoEvent::Error { source_id, .. } = &event
                {
                    audio::terminal(key, source_id.clone(), terminal.clone());
                }
                emit(&sink, event);
            });
            if let Ok(controller) = &controller {
                *slot
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Arc::downgrade(controller);
            }
            Box::new(PixelVideo {
                view,
                controller,
                audio,
                wants_audio: false,
                context,
                source_id: String::new(),
            })
        }),
    );
}

fn emit(context: &ComponentContext, event: VideoEvent) {
    match serde_json::to_value(event) {
        Ok(value) => context.emit(EVENT_NAME, value),
        Err(error) => log::error!("[NSH pixel video] native event serialization failed: {error}"),
    }
}
