use super::super::registration::Registration;
use nsh_platform_macos::{
    ComponentContext, Components, NativeComponent,
    objc2::{MainThreadMarker, rc::Retained},
    objc2_app_kit::NSView,
};
use serde_json::{Value, json};
use std::rc::Rc;

struct Shortcut {
    view: Retained<NSView>,
    registration: Registration,
    context: ComponentContext,
}

impl NativeComponent for Shortcut {
    fn view(&self) -> Retained<NSView> {
        self.view.clone()
    }
    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name == "registration" {
            let Some(value) = value else {
                return;
            };
            let Some(spec) = value.get("shortcut").and_then(Value::as_str) else {
                return;
            };
            let error = self.registration.apply(spec).err();
            self.context.emit(
                "registered",
                json!({"version":value["version"],"error":error}),
            );
        }
    }
}

pub(crate) fn register(components: &Components) {
    components.register(
        "hongguo.window-shortcut",
        Rc::new(|context: ComponentContext| {
            let sink = context.clone();
            Box::new(Shortcut {
                view: NSView::new(MainThreadMarker::new().expect("native component main thread")),
                registration: Registration::new(move || sink.emit("pressed", json!(null))),
                context,
            })
        }),
    );
}
