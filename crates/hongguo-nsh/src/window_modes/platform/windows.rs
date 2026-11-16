use super::super::{desktop::TICK_SECONDS, wire::Observation};
use nsh_platform_windows::{
    ComponentContext, Components, NativeComponent,
    windows::{
        Win32::{
            Foundation::{HWND, POINT, RECT},
            UI::WindowsAndMessaging::*,
        },
        core::w,
    },
};
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::{Rc, Weak},
    time::{Duration, Instant},
};

#[path = "windows_visibility.rs"]
mod visibility;

const TIMER_MS: u32 = (TICK_SECONDS * 1000.0) as u32;
const VISIBILITY_INTERVAL: Duration = Duration::from_millis(250);
thread_local! { static OBSERVERS: RefCell<HashMap<usize, Weak<RefCell<Observer>>>> = RefCell::default(); }

struct Observer {
    child: HWND,
    context: ComponentContext,
    observe_visibility: bool,
    visibility_checked: Option<Instant>,
    last: Option<Observation>,
}

impl Observer {
    fn tick(&mut self) {
        let root = unsafe { GetAncestor(self.child, GA_ROOT) };
        if root == self.child || root.0.is_null() {
            return;
        }
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
        } else if self
            .visibility_checked
            .is_none_or(|at| at.elapsed() >= VISIBILITY_INTERVAL)
        {
            self.visibility_checked = Some(Instant::now());
            match visibility::exposed(root) {
                Ok(value) => exposed = Some(value),
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
    fn window(&self) -> Option<HWND> {
        Some(self.observer.borrow().child)
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name == "observeVisibility"
            && let Some(enabled) = value.and_then(Value::as_bool)
        {
            let mut observer = self.observer.borrow_mut();
            observer.observe_visibility = enabled;
            observer.visibility_checked = None;
            observer.tick();
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        OBSERVERS.with(|observers| observers.borrow_mut().remove(&self.timer));
        unsafe {
            let _ = KillTimer(None, self.timer);
            if let Err(error) = DestroyWindow(self.observer.borrow().child) {
                log::warn!("[NSH window] observer cleanup failed: {error}");
            }
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
            let child = unsafe {
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
            };
            match child {
                Ok(child) => {
                    let observer = Rc::new(RefCell::new(Observer {
                        child,
                        context: context.clone(),
                        observe_visibility: false,
                        visibility_checked: None,
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
                }
                Err(error) => Box::new(Failed(context, Some(error.to_string()))),
            }
        }),
    );
}

struct Failed(ComponentContext, Option<String>);
impl NativeComponent for Failed {
    fn set_prop(&mut self, _name: &str, _value: Option<&Value>) {
        if let Some(error) = self.1.take() {
            self.0.emit("failure", Value::String(error));
        }
    }
}
