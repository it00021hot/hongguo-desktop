use super::super::{
    desktop::{Capture, Desktop, TICK_SECONDS, Window},
    geometry::{Geometry, Plan, Rect},
    wire::Request,
};
use nsh_platform_macos::{
    ComponentContext, Components, NativeComponent,
    objc2::{MainThreadMarker, MainThreadOnly, rc::Retained},
    objc2_app_kit::{
        NSEvent, NSScreen, NSView, NSWindow, NSWindowButton, NSWindowOcclusionState,
        NSWindowStyleMask,
    },
    objc2_foundation::{NSPoint, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize, NSTimer},
};
use serde_json::Value;
use std::{cell::RefCell, rc::Rc};

#[path = "macos_visibility.rs"]
mod visibility;

struct AppKitWindow(Retained<NSView>);
impl AppKitWindow {
    fn window(&self) -> Result<Retained<NSWindow>, String> {
        self.0
            .window()
            .ok_or_else(|| "native window is not attached".into())
    }
    fn primary_top(&self) -> Result<f64, String> {
        NSScreen::screens(self.0.mtm())
            .firstObject()
            .map(|screen| screen.frame().origin.y + screen.frame().size.height)
            .ok_or_else(|| "no display is available".into())
    }
}
impl Window for AppKitWindow {
    fn exposed(&self) -> Result<bool, String> {
        let window = self.window()?;
        if !(window.isVisible()
            && !window.isMiniaturized()
            && window
                .occlusionState()
                .contains(NSWindowOcclusionState::Visible))
        {
            return Ok(false);
        }
        visibility::exposed(&window)
    }
    fn attached(&self) -> bool {
        self.0.window().is_some()
    }
    fn capture(&self) -> Result<Capture, String> {
        let window = self.window()?;
        let top = self.primary_top()?;
        let screen = window
            .screen()
            .ok_or_else(|| "window is not on a display".to_owned())?;
        let rect = |r: NSRect| Rect {
            x: r.origin.x,
            y: top - r.origin.y - r.size.height,
            width: r.size.width,
            height: r.size.height,
        };
        let frame = window.frame();
        let pointer = NSEvent::mouseLocation();
        Ok(Capture {
            geometry: Geometry {
                frame: rect(frame),
                work: rect(screen.visibleFrame()),
                maximized: window.isZoomed(),
                scale: 1.0,
            },
            fullscreen: window.styleMask().contains(NSWindowStyleMask::FullScreen),
            minimized: window.isMiniaturized(),
            pinned: window.level() > nsh_platform_macos::objc2_app_kit::NSNormalWindowLevel,
            pointer_inside: Some(
                pointer.x >= frame.origin.x
                    && pointer.y >= frame.origin.y
                    && pointer.x < frame.origin.x + frame.size.width
                    && pointer.y < frame.origin.y + frame.size.height,
            ),
        })
    }
    fn apply(&self, plan: Plan) -> Result<(), String> {
        let window = self.window()?;
        if window.isZoomed() {
            window.zoom(None);
        }
        window.setMinSize(NSSize::new(plan.minimum.0, plan.minimum.1));
        let r = plan.frame;
        window.setFrame_display(
            NSRect::new(
                NSPoint::new(r.x, self.primary_top()? - r.y - r.height),
                NSSize::new(r.width, r.height),
            ),
            true,
        );
        if plan.maximized && !window.isZoomed() {
            window.zoom(None);
        }
        Ok(())
    }
    fn controls_hidden(&self, hidden: bool) -> Result<(), String> {
        let window = self.window()?;
        if let Some(controls) = window
            .standardWindowButton(NSWindowButton::CloseButton)
            // AppKit owns the attached control hierarchy on this main-thread callback.
            .and_then(|button| unsafe { button.superview() })
        {
            controls.setHidden(hidden);
        }
        Ok(())
    }
    fn visible(&self, visible: bool, activate: bool) -> Result<(), String> {
        let window = self.window()?;
        if !visible {
            window.orderOut(None);
        } else if activate {
            window.makeKeyAndOrderFront(None);
            nsh_platform_macos::objc2_app_kit::NSApplication::sharedApplication(self.0.mtm())
                .activate();
        } else {
            window.orderFront(None);
        }
        Ok(())
    }
    fn pinned(&self, pinned: bool) -> Result<(), String> {
        self.window()?.setLevel(if pinned {
            nsh_platform_macos::objc2_app_kit::NSFloatingWindowLevel
        } else {
            nsh_platform_macos::objc2_app_kit::NSNormalWindowLevel
        });
        Ok(())
    }
}

struct Host {
    view: Retained<NSView>,
    desktop: Rc<RefCell<Desktop>>,
    timer: Retained<NSTimer>,
    context: ComponentContext,
}
impl NativeComponent for Host {
    fn view(&self) -> Retained<NSView> {
        self.view.clone()
    }
    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        match name {
            "observeVisibility" => {
                if let Some(enabled) = value.and_then(Value::as_bool) {
                    self.desktop.borrow_mut().observe_visibility(enabled);
                }
            }
            "request" => {
                if let Some(value) = value.filter(|value| !value.is_null()) {
                    match serde_json::from_value::<Request>(value.clone()) {
                        Ok(request) => self.desktop.borrow_mut().request(request),
                        Err(error) => self
                            .context
                            .emit("failure", Value::String(error.to_string())),
                    }
                }
            }
            "bossHidden" => {
                if let Some(hidden) = value.and_then(Value::as_bool) {
                    self.desktop.borrow_mut().boss(hidden);
                }
            }
            _ => {}
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.timer.invalidate();
        self.desktop.borrow_mut().restore();
    }
}

pub(crate) fn register(components: &Components) {
    components.register(
        "hongguo.window-modes",
        Rc::new(|context: ComponentContext| {
            let view = NSView::new(MainThreadMarker::new().expect("native component main thread"));
            let sink = context.clone();
            let desktop = Rc::new(RefCell::new(Desktop::new(
                Box::new(AppKitWindow(view.clone())),
                move |name, value| sink.emit(name, value),
            )));
            let weak = Rc::downgrade(&desktop);
            let block = block2::RcBlock::new(move |_| {
                if let Some(desktop) = weak.upgrade() {
                    desktop.borrow_mut().tick();
                }
            });
            // The timer and its non-Send state live only on the main run loop.
            let timer =
                unsafe { NSTimer::timerWithTimeInterval_repeats_block(TICK_SECONDS, true, &block) };
            unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
            Box::new(Host {
                view,
                desktop,
                timer,
                context,
            })
        }),
    );
}
