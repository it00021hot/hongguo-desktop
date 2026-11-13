#![cfg(target_os = "macos")]

mod view;

use hongguo_nsh_video_api::{
    COMPONENT_NAME, EVENT_NAME, ErrorCode, SEEK_COMMAND, STATE_PROP, Seek, VideoEvent, VideoProps,
};
use hongguo_nsh_video_apple::{OutputError, PlayerController};
use nsh_platform_macos::{ComponentContext, Components, NativeComponent, objc2_app_kit::NSView};
use objc2::{MainThreadMarker, rc::Retained};
use serde_json::Value;
use std::rc::Rc;

struct PixelVideo {
    view: Retained<view::PixelView>,
    controller: Result<Rc<PlayerController>, OutputError>,
    context: ComponentContext,
    source_id: String,
}

impl NativeComponent for PixelVideo {
    fn view(&self) -> Retained<NSView> {
        Retained::into_super(self.view.clone())
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
    }

    fn command(&mut self, name: &str, args: &Value) {
        if name != SEEK_COMMAND {
            return;
        }
        let result = serde_json::from_value::<Seek>(args.clone())
            .map_err(|error| (ErrorCode::InvalidProps, error.to_string()))
            .and_then(|request| {
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
            let mtm =
                MainThreadMarker::new().expect("NSH native factory runs on the AppKit thread");
            let view = view::PixelView::new(mtm);
            let sink = context.clone();
            let controller = view.attach(move |event| emit(&sink, event));
            Box::new(PixelVideo {
                view,
                controller,
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
