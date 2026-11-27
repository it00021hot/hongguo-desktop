use super::{
    super::{desktop::TICK_SECONDS, wire::Observation},
    exposure::ExposureRefresh,
};
use nsh_platform_macos::{
    ComponentContext, Components, NativeComponent,
    objc2::{MainThreadMarker, rc::Retained},
    objc2_app_kit::{NSEvent, NSView, NSWindowButton, NSWindowOcclusionState},
    objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSTimer},
};
use serde_json::Value;
use std::{cell::RefCell, rc::Rc};

#[path = "macos_visibility.rs"]
mod visibility;
struct Observer {
    view: Retained<NSView>,
    context: ComponentContext,
    observe_visibility: bool,
    exposure_refresh: ExposureRefresh,
    controls_hidden: bool,
    applied_controls_hidden: Option<bool>,
    last: Option<Observation>,
}

impl Observer {
    fn controls(&mut self, hidden: bool) {
        self.controls_hidden = hidden;
        if self.applied_controls_hidden == Some(hidden) {
            return;
        }
        if let Some(window) = self.view.window()
            && let Some(controls) = window
                .standardWindowButton(NSWindowButton::CloseButton)
                .and_then(|button| unsafe { button.superview() })
        {
            controls.setHidden(hidden);
            self.applied_controls_hidden = Some(hidden);
        }
    }

    fn tick(&mut self) {
        self.controls(self.controls_hidden);
        let Some(window) = self.view.window() else {
            return;
        };
        let frame = window.frame();
        let pointer = NSEvent::mouseLocation();
        let pointer_inside = Some(
            pointer.x >= frame.origin.x
                && pointer.y >= frame.origin.y
                && pointer.x < frame.origin.x + frame.size.width
                && pointer.y < frame.origin.y + frame.size.height,
        );
        let mut exposed = self.last.and_then(|last| last.exposed);
        if !self.observe_visibility {
            exposed = Some(true);
        } else if !window.isVisible()
            || window.isMiniaturized()
            || !window
                .occlusionState()
                .contains(NSWindowOcclusionState::Visible)
        {
            self.exposure_refresh.reset();
            exposed = Some(false);
        } else if self.exposure_refresh.should_scan(window.isKeyWindow()) {
            match visibility::exposed(&window) {
                Ok(value) => exposed = Some(value),
                Err(error) => self.context.emit("failure", Value::String(error)),
            }
        }
        let observation = Observation {
            pointer_inside,
            exposed,
        };
        if self.last != Some(observation) {
            if self.last.and_then(|last| last.exposed) != exposed {
                log::debug!("[NSH window] visibility changed: exposed={exposed:?}");
            }
            self.last = Some(observation);
            self.context.emit(
                "observation",
                serde_json::to_value(observation).expect("window observation"),
            );
        }
    }
}

struct Host {
    observer: Rc<RefCell<Observer>>,
    timer: Retained<NSTimer>,
}

impl NativeComponent for Host {
    fn view(&self) -> Retained<NSView> {
        self.observer.borrow().view.clone()
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        let Some(value) = value.and_then(Value::as_bool) else {
            return;
        };
        let mut observer = self.observer.borrow_mut();
        match name {
            "observeVisibility" => {
                observer.observe_visibility = value;
                observer.exposure_refresh.reset();
                observer.tick();
            }
            "controlsHidden" => observer.controls(value),
            _ => {}
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.timer.invalidate();
        self.observer.borrow_mut().controls(false);
    }
}

pub(crate) fn register(components: &Components) {
    components.register(
        "hongguo.window-modes",
        Rc::new(|context: ComponentContext| {
            let view = NSView::new(MainThreadMarker::new().expect("native component main thread"));
            let observer = Rc::new(RefCell::new(Observer {
                view,
                context,
                observe_visibility: false,
                exposure_refresh: ExposureRefresh::default(),
                controls_hidden: false,
                applied_controls_hidden: None,
                last: None,
            }));
            let weak = Rc::downgrade(&observer);
            let block = block2::RcBlock::new(move |_| {
                if let Some(observer) = weak.upgrade()
                    && let Ok(mut observer) = observer.try_borrow_mut()
                {
                    observer.tick();
                }
            });
            let timer =
                unsafe { NSTimer::timerWithTimeInterval_repeats_block(TICK_SECONDS, true, &block) };
            unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
            Box::new(Host { observer, timer })
        }),
    );
}
