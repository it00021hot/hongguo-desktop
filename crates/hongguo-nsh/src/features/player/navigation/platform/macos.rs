use super::super::protocol::{COMPONENT, ENABLED, WHEEL, WheelEvent};
use block2::RcBlock;
use nsh_platform_macos::{
    ComponentContext, Components, NativeComponent,
    objc2::{MainThreadMarker, MainThreadOnly, rc::Retained, runtime::AnyObject},
    objc2_app_kit::{NSEvent, NSEventMask, NSView},
};
use serde_json::Value;
use std::{cell::Cell, ptr::NonNull, rc::Rc};

const POINTS_PER_LINE: f64 = 10.0;

struct Wheel {
    view: Retained<NSView>,
    enabled: Rc<Cell<bool>>,
    monitor: Option<Retained<AnyObject>>,
}

impl NativeComponent for Wheel {
    fn view(&self) -> Retained<NSView> {
        self.view.clone()
    }
    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name == ENABLED {
            match value {
                Some(Value::Bool(value)) => self.enabled.set(*value),
                None => self.enabled.set(false),
                Some(_) => log::error!("[NSH navigation] invalid wheel enabled prop"),
            }
        }
    }
}

impl Drop for Wheel {
    fn drop(&mut self) {
        self.enabled.set(false);
        if let Some(monitor) = self.monitor.take() {
            unsafe {
                NSEvent::removeMonitor(&monitor);
            }
        }
    }
}

pub(crate) fn register(components: &Components) {
    components.register(
        COMPONENT,
        Rc::new(|context: ComponentContext| {
            let view = NSView::new(MainThreadMarker::new().expect("wheel component main thread"));
            let enabled = Rc::new(Cell::new(false));
            let region = view.clone();
            let active = enabled.clone();
            let sink = context.clone();
            let handler = RcBlock::new(move |raw: NonNull<NSEvent>| {
                let event = unsafe { raw.as_ref() };
                if active.get()
                    && region.window().is_some()
                    && event.window(region.mtm()) == region.window()
                    && !region.isHiddenOrHasHiddenAncestor()
                {
                    let point = region.convertPoint_fromView(event.locationInWindow(), None);
                    let bounds = region.bounds();
                    if point.x >= bounds.origin.x
                        && point.y >= bounds.origin.y
                        && point.x < bounds.origin.x + bounds.size.width
                        && point.y < bounds.origin.y + bounds.size.height
                    {
                        let factor = if event.hasPreciseScrollingDeltas() {
                            1.0
                        } else {
                            POINTS_PER_LINE
                        };
                        match serde_json::to_value(WheelEvent {
                            delta_y: -event.scrollingDeltaY() * factor,
                        }) {
                            Ok(value) => sink.emit(WHEEL, value),
                            Err(error) => log::error!(
                                "[NSH navigation] could not encode wheel event: {error}"
                            ),
                        }
                    }
                }
                raw.as_ptr()
            });
            let monitor = unsafe {
                NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                    NSEventMask::ScrollWheel,
                    &handler,
                )
            };
            if monitor.is_none() {
                log::error!("[NSH navigation] could not install wheel monitor");
            }
            Box::new(Wheel {
                view,
                enabled,
                monitor,
            })
        }),
    );
}
