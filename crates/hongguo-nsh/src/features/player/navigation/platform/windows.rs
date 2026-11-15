use super::super::protocol::{COMPONENT, ENABLED, WHEEL, WheelEvent};
use nsh_platform_windows::{
    ComponentContext, Components, NativeComponent,
    windows::{
        Win32::{
            Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
            UI::{
                Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
                WindowsAndMessaging::{
                    CreateWindowExW, DestroyWindow, GA_ROOT, GetAncestor, GetWindowRect,
                    SPI_GETWHEELSCROLLLINES, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
                    SystemParametersInfoW, WINDOW_EX_STYLE, WM_MOUSEWHEEL, WM_NCDESTROY, WS_POPUP,
                },
            },
        },
        core::w,
    },
};
use serde_json::Value;
use std::{cell::Cell, rc::Rc};

const WHEEL_DELTA: f64 = 120.0;
const LINE_POINTS: f64 = 16.0;
const PAGE_LINES: u32 = 20;

struct Sink {
    context: ComponentContext,
    enabled: Cell<bool>,
    region: HWND,
    registered: Cell<bool>,
}
struct Wheel {
    window: Result<HWND, String>,
    root: Option<HWND>,
    sink: Rc<Sink>,
}

unsafe extern "system" fn wheel_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    token: usize,
    data: usize,
) -> LRESULT {
    let sink = unsafe { &*(data as *const Sink) };
    if message == WM_NCDESTROY {
        sink.enabled.set(false);
        let registered = sink.registered.replace(false);
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(wheel_proc), token);
        }
        let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        if registered {
            // The installed subclass owns one strong reference until removal or HWND destruction.
            unsafe {
                drop(Rc::from_raw(data as *const Sink));
            }
        }
        return result;
    }
    if message == WM_MOUSEWHEEL && sink.enabled.get() {
        let mut bounds = RECT::default();
        if unsafe { GetWindowRect(sink.region, &mut bounds) }.is_ok() {
            let x = (lparam.0 as u16 as i16) as i32;
            let y = ((lparam.0 >> 16) as u16 as i16) as i32;
            if x >= bounds.left && x < bounds.right && y >= bounds.top && y < bounds.bottom {
                let mut lines = 3_u32;
                let result = unsafe {
                    SystemParametersInfoW(
                        SPI_GETWHEELSCROLLLINES,
                        0,
                        Some((&mut lines as *mut u32).cast()),
                        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                    )
                };
                if let Err(error) = result {
                    log::error!("[NSH navigation] wheel settings failed: {error}");
                } else {
                    let lines = if lines == u32::MAX {
                        PAGE_LINES
                    } else {
                        lines.clamp(1, PAGE_LINES)
                    };
                    let delta = ((wparam.0 >> 16) as u16 as i16) as f64;
                    match serde_json::to_value(WheelEvent {
                        delta_y: -delta / WHEEL_DELTA * f64::from(lines) * LINE_POINTS,
                    }) {
                        Ok(value) => sink.context.emit(WHEEL, value),
                        Err(error) => {
                            log::error!("[NSH navigation] could not encode wheel event: {error}")
                        }
                    }
                }
            }
        }
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

impl NativeComponent for Wheel {
    fn window(&self) -> Option<HWND> {
        self.window.as_ref().ok().copied()
    }
    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name != ENABLED {
            return;
        }
        match value {
            Some(Value::Bool(value)) => self.sink.enabled.set(*value),
            None => self.sink.enabled.set(false),
            Some(_) => {
                log::error!("[NSH navigation] invalid wheel enabled prop");
                return;
            }
        }
        if self.root.is_none()
            && let Ok(window) = self.window
        {
            let root = unsafe { GetAncestor(window, GA_ROOT) };
            if root != window && !root.0.is_null() {
                let token = Rc::into_raw(self.sink.clone()) as usize;
                if unsafe { SetWindowSubclass(root, Some(wheel_proc), token, token) }.as_bool() {
                    self.sink.registered.set(true);
                    self.root = Some(root);
                } else {
                    unsafe {
                        drop(Rc::from_raw(token as *const Sink));
                    }
                    log::error!("[NSH navigation] could not install wheel subclass");
                }
            }
        }
    }
}

impl Drop for Wheel {
    fn drop(&mut self) {
        self.sink.enabled.set(false);
        if let Some(root) = self.root.take()
            && self.sink.registered.get()
        {
            let token = Rc::as_ptr(&self.sink) as usize;
            if unsafe { RemoveWindowSubclass(root, Some(wheel_proc), token) }.as_bool() {
                self.sink.registered.set(false);
                unsafe {
                    drop(Rc::from_raw(token as *const Sink));
                }
            } else {
                // Keep the callback's reference alive until WM_NCDESTROY rather than freeing its data.
                log::warn!(
                    "[NSH navigation] wheel subclass removal deferred to window destruction"
                );
            }
        }
        if let Ok(window) = self.window
            && let Err(error) = unsafe { DestroyWindow(window) }
        {
            log::warn!("[NSH navigation] wheel cleanup failed: {error}");
        }
    }
}

pub(crate) fn register(components: &Components) {
    components.register(
        COMPONENT,
        Rc::new(|context: ComponentContext| {
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
            let region = window.as_ref().ok().copied().unwrap_or_default();
            Box::new(Wheel {
                window,
                root: None,
                sink: Rc::new(Sink {
                    context,
                    enabled: Cell::new(false),
                    region,
                    registered: Cell::new(false),
                }),
            })
        }),
    );
}
