use super::super::registration::Registration;
use nsh_platform_windows::{
    ComponentContext, Components, NativeComponent,
    windows::{
        Win32::{
            Foundation::HWND,
            UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP},
        },
        core::w,
    },
};
use serde_json::{Value, json};
use std::rc::Rc;

struct Shortcut {
    window: Result<HWND, String>,
    registration: Registration,
    context: ComponentContext,
}

impl NativeComponent for Shortcut {
    fn window(&self) -> Option<HWND> {
        self.window.as_ref().ok().copied()
    }
    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name == "registration" {
            let Some(value) = value else {
                return;
            };
            let Some(spec) = value.get("shortcut").and_then(Value::as_str) else {
                return;
            };
            let result = self
                .window
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|_| self.registration.apply(spec));
            self.context.emit(
                "registered",
                json!({"version":value["version"],"error":result.err()}),
            );
        }
    }
}

impl Drop for Shortcut {
    fn drop(&mut self) {
        if let Ok(window) = self.window {
            // Only destroy the child created by this component.
            if let Err(error) = unsafe { DestroyWindow(window) } {
                log::warn!("[NSH boss key] child cleanup failed: {error}");
            }
        }
    }
}

pub(crate) fn register(components: &Components) {
    components.register(
        "hongguo.window-shortcut",
        Rc::new(|context: ComponentContext| {
            let sink = context.clone();
            // STATIC is a system class; the host adopts this initially hidden popup as its child.
            let window = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("STATIC"),
                    w!(""),
                    WS_POPUP,
                    0,
                    0,
                    1,
                    1,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .map_err(|error| error.to_string());
            Box::new(Shortcut {
                window,
                registration: Registration::new(move || sink.emit("pressed", json!(null))),
                context,
            })
        }),
    );
}
