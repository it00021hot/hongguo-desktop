use super::{
    super::{desktop::TICK_SECONDS, wire::Observation},
    exposure::ExposureRefresh,
};
use nsh_platform_windows::{
    ComponentContext, Components, NativeComponent,
    windows::Win32::{
        Foundation::{HWND, POINT, RECT},
        UI::WindowsAndMessaging::*,
    },
};
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::{Rc, Weak},
};

#[path = "windows_visibility.rs"]
mod visibility;

const TIMER_MS: u32 = (TICK_SECONDS * 1000.0) as u32;
thread_local! { static OBSERVERS: RefCell<HashMap<usize, Weak<RefCell<Observer>>>> = RefCell::default(); }

struct Observer {
    context: ComponentContext,
    observe_visibility: bool,
    exposure_refresh: ExposureRefresh,
    last: Option<Observation>,
}

impl Observer {
    fn tick(&mut self) {
        let Some(root) = self.context.window_hwnd() else {
            return;
        };
        let mut rect = RECT::default();
        let mut pointer = POINT::default();
        let pointer_inside = unsafe { GetWindowRect(root, &mut rect) }
            .and_then(|()| unsafe { GetCursorPos(&mut pointer) })
            .ok()
            .map(|()| {
                pointer.x >= rect.left
                    && pointer.x < rect.right
                    && pointer.y >= rect.top
                    && pointer.y < rect.bottom
            });
        let mut exposed = self.last.and_then(|last| last.exposed);
        if !self.observe_visibility {
            exposed = Some(true);
        } else if unsafe { !IsWindowVisible(root).as_bool() || IsIconic(root).as_bool() } {
            self.exposure_refresh.reset();
            exposed = Some(false);
        } else if self
            .exposure_refresh
            .should_scan(unsafe { GetForegroundWindow() == root })
        {
            match visibility::exposed(root) {
                Ok(value) => {
                    if self.last.and_then(|last| last.exposed) != Some(value) {
                        log::debug!("[NSH window] visibility changed: exposed={value}");
                    }
                    exposed = Some(value);
                }
                Err(error) => self.context.emit("failure", Value::String(error)),
            }
        }
        let observation = Observation {
            pointer_inside,
            exposed,
        };
        if self.last != Some(observation) {
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
    timer: usize,
}

impl NativeComponent for Host {
    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name == "observeVisibility"
            && let Some(enabled) = value.and_then(Value::as_bool)
        {
            let mut observer = self.observer.borrow_mut();
            observer.observe_visibility = enabled;
            observer.exposure_refresh.reset();
            observer.tick();
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        OBSERVERS.with(|observers| observers.borrow_mut().remove(&self.timer));
        unsafe {
            let _ = KillTimer(None, self.timer);
        }
    }
}

unsafe extern "system" fn tick(_window: HWND, _message: u32, id: usize, _time: u32) {
    if let Some(observer) =
        OBSERVERS.with(|observers| observers.borrow().get(&id).and_then(Weak::upgrade))
        && let Ok(mut observer) = observer.try_borrow_mut()
    {
        observer.tick();
    }
}

pub(crate) fn register(components: &Components) {
    components.register(
        "hongguo.window-modes",
        Rc::new(|context: ComponentContext| {
            let observer = Rc::new(RefCell::new(Observer {
                context: context.clone(),
                observe_visibility: false,
                exposure_refresh: ExposureRefresh::default(),
                last: None,
            }));
            let timer = unsafe { SetTimer(None, 0, TIMER_MS, Some(tick)) };
            if timer == 0 {
                context.emit(
                    "failure",
                    Value::String("system window observer timer is unavailable".into()),
                );
            }
            OBSERVERS.with(|observers| {
                observers
                    .borrow_mut()
                    .insert(timer, Rc::downgrade(&observer))
            });
            Box::new(Host { observer, timer }) as Box<dyn NativeComponent>
        }),
    );
}
