#[path = "windows_chrome.rs"]
mod chrome;
use super::super::{
    desktop::{Capture, Desktop, TICK_SECONDS, Window},
    geometry::{Geometry, Plan, Rect},
    wire::{Request, Response, State},
};
use nsh_platform_windows::{
    ComponentContext, Components, NativeComponent,
    windows::{
        Win32::{
            Foundation::{HWND, POINT, RECT},
            Graphics::Gdi::{
                GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
            },
            UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::*},
        },
        core::w,
    },
};
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::{Rc, Weak},
};

const TIMER_MS: u32 = (TICK_SECONDS * 1000.0) as u32;
thread_local! {static TIMERS:RefCell<HashMap<usize,Weak<RefCell<Desktop>>>>=RefCell::default();}

struct WinWindow {
    child: HWND,
    chrome: Box<chrome::Chrome>,
}
impl WinWindow {
    fn root(&self) -> Result<HWND, String> {
        let root = unsafe { GetAncestor(self.child, GA_ROOT) };
        if root == self.child || root.0.is_null() {
            Err("native window is not attached".into())
        } else {
            Ok(root)
        }
    }
}
impl Window for WinWindow {
    fn attached(&self) -> bool {
        self.root().is_ok()
    }
    fn capture(&self) -> Result<Capture, String> {
        unsafe {
            let root = self.root()?;
            let scale = f64::from(GetDpiForWindow(root)) / 96.0;
            if scale <= 0.0 {
                return Err("cannot read window scale".into());
            }
            let mut frame = RECT::default();
            GetWindowRect(root, &mut frame).map_err(|error| error.to_string())?;
            let mut monitor = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(
                MonitorFromWindow(root, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            )
            .as_bool()
            {
                return Err("cannot read display work area".into());
            }
            let rect = |r: RECT| Rect {
                x: f64::from(r.left) / scale,
                y: f64::from(r.top) / scale,
                width: f64::from(r.right - r.left) / scale,
                height: f64::from(r.bottom - r.top) / scale,
            };
            let mut point = POINT::default();
            let inside = GetCursorPos(&mut point).ok().map(|_| {
                point.x >= frame.left
                    && point.y >= frame.top
                    && point.x < frame.right
                    && point.y < frame.bottom
            });
            let fullscreen = GetWindowLongPtrW(root, GWL_STYLE) & WS_THICKFRAME.0 as isize == 0
                && frame == monitor.rcMonitor;
            Ok(Capture {
                geometry: Geometry {
                    frame: rect(frame),
                    work: rect(monitor.rcWork),
                    maximized: IsZoomed(root).as_bool(),
                    scale,
                },
                fullscreen,
                minimized: IsIconic(root).as_bool(),
                pointer_inside: inside,
                pinned: GetWindowLongPtrW(root, GWL_EXSTYLE) & WS_EX_TOPMOST.0 as isize != 0,
            })
        }
    }
    fn apply(&self, plan: Plan) -> Result<(), String> {
        unsafe {
            let root = self.root()?;
            // Install the minimum-size hook before SetWindowPos can query it.
            self.chrome.attach(root)?;
            if IsZoomed(root).as_bool() {
                let _ = ShowWindow(root, SW_RESTORE);
            }
            self.chrome.minimum.set(plan.minimum);
            let r = plan.frame;
            let scale = plan.scale;
            SetWindowPos(
                root,
                None,
                (r.x * scale).round() as i32,
                (r.y * scale).round() as i32,
                (r.width * scale).round() as i32,
                (r.height * scale).round() as i32,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .map_err(|error| error.to_string())?;
            if plan.maximized {
                let _ = ShowWindow(root, SW_MAXIMIZE);
            }
        }
        Ok(())
    }
    fn controls_hidden(&self, hidden: bool) -> Result<(), String> {
        self.chrome.apply(self.root()?, hidden)
    }
    fn visible(&self, visible: bool, activate: bool) -> Result<(), String> {
        unsafe {
            let root = self.root()?;
            let _ = ShowWindow(
                root,
                if !visible {
                    SW_HIDE
                } else if activate && IsIconic(root).as_bool() {
                    SW_RESTORE
                } else {
                    // Preserve maximized/snapped geometry on cursor/boss return.
                    SW_SHOWNA
                },
            );
            if visible && activate {
                let _ = SetForegroundWindow(root);
            }
        }
        Ok(())
    }
    fn pinned(&self, pinned: bool) -> Result<(), String> {
        unsafe {
            SetWindowPos(
                self.root()?,
                Some(if pinned { HWND_TOPMOST } else { HWND_NOTOPMOST }),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        }
        .map_err(|error| error.to_string())
    }
}

struct Host {
    child: HWND,
    desktop: Rc<RefCell<Desktop>>,
    timer: usize,
    context: ComponentContext,
}
impl NativeComponent for Host {
    fn window(&self) -> Option<HWND> {
        Some(self.child)
    }
    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        match name {
            "request" => {
                if let Some(value) = value.filter(|value| !value.is_null()) {
                    match serde_json::from_value::<Request>(value.clone()) {
                        Ok(request) => {
                            if self.timer == 0 {
                                self.context.emit(
                                    "applied",
                                    serde_json::to_value(Response {
                                        serial: request.serial,
                                        state: State::initial(),
                                        error: Some("system window timer is unavailable".into()),
                                    })
                                    .expect("window failure"),
                                );
                            } else {
                                self.desktop.borrow_mut().request(request);
                            }
                        }
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
        TIMERS.with(|timers| timers.borrow_mut().remove(&self.timer));
        self.desktop.borrow_mut().restore();
        unsafe {
            let _ = KillTimer(None, self.timer);
            if let Err(error) = DestroyWindow(self.child) {
                log::warn!("[NSH window] child cleanup failed: {error}");
            }
        }
    }
}

unsafe extern "system" fn tick(_window: HWND, _message: u32, id: usize, _time: u32) {
    let desktop = TIMERS.with(|timers| timers.borrow().get(&id).and_then(Weak::upgrade));
    if let Some(desktop) = desktop
        && let Ok(mut desktop) = desktop.try_borrow_mut()
    {
        desktop.tick();
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
                    let sink = context.clone();
                    let desktop = Rc::new(RefCell::new(Desktop::new(
                        Box::new(WinWindow {
                            child,
                            chrome: Box::new(chrome::Chrome::new()),
                        }),
                        move |name, value| sink.emit(name, value),
                    )));
                    let timer = unsafe { SetTimer(None, 0, TIMER_MS, Some(tick)) };
                    TIMERS
                        .with(|timers| timers.borrow_mut().insert(timer, Rc::downgrade(&desktop)));
                    Box::new(Host {
                        child,
                        desktop,
                        timer,
                        context,
                    }) as Box<dyn NativeComponent>
                }
                Err(error) => Box::new(Failed(context, error.to_string())),
            }
        }),
    );
}

struct Failed(ComponentContext, String);
impl NativeComponent for Failed {
    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name == "request"
            && let Some(request) =
                value.and_then(|value| serde_json::from_value::<Request>(value.clone()).ok())
        {
            self.0.emit(
                "applied",
                serde_json::to_value(Response {
                    serial: request.serial,
                    state: State::initial(),
                    error: Some(self.1.clone()),
                })
                .expect("window failure"),
            );
        }
    }
}
