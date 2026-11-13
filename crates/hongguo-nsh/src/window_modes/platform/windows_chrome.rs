use nsh_platform_windows::windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    UI::{
        HiDpi::GetDpiForWindow,
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
use std::cell::Cell;

const SUBCLASS_ID: usize = 0x48474d4f;
const RESIZE_EDGE_DIP: f64 = 8.0;

pub(super) struct Chrome {
    root: Cell<HWND>,
    hidden: Cell<bool>,
    style: Cell<isize>,
    pub minimum: Cell<(f64, f64)>,
}
impl Chrome {
    pub fn attach(&self, root: HWND) -> Result<(), String> {
        self.apply(root, self.hidden.get())
    }
    pub fn new() -> Self {
        Self {
            root: Cell::new(HWND::default()),
            hidden: Cell::new(false),
            style: Cell::new(0),
            minimum: Cell::new((480.0, 270.0)),
        }
    }
    pub fn apply(&self, root: HWND, hidden: bool) -> Result<(), String> {
        unsafe {
            if self.root.get() != root {
                if !SetWindowSubclass(
                    root,
                    Some(subclass),
                    SUBCLASS_ID,
                    self as *const Self as usize,
                )
                .as_bool()
                {
                    return Err("cannot attach window chrome bridge".into());
                }
                self.root.set(root);
                self.style.set(GetWindowLongPtrW(root, GWL_STYLE));
            }
            let style = GetWindowLongPtrW(root, GWL_STYLE);
            let caption = WS_CAPTION.0 as isize;
            let desired = if hidden {
                style & !caption
            } else {
                style | (self.style.get() & caption)
            };
            if self.hidden.replace(hidden) != hidden || style != desired {
                SetWindowLongPtrW(root, GWL_STYLE, desired);
                SetWindowPos(
                    root,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                )
                .map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }
}
impl Drop for Chrome {
    fn drop(&mut self) {
        if !self.root.get().0.is_null() {
            unsafe {
                let _ = RemoveWindowSubclass(self.root.get(), Some(subclass), SUBCLASS_ID);
            }
        }
    }
}

unsafe extern "system" fn subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let chrome = unsafe { &*(data as *const Chrome) };
    if message == WM_GETMINMAXINFO && lparam.0 != 0 {
        let min = chrome.minimum.get();
        let scale = f64::from(unsafe { GetDpiForWindow(hwnd) }) / 96.0;
        let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
        info.ptMinTrackSize.x = (min.0 * scale).round() as i32;
        info.ptMinTrackSize.y = (min.1 * scale).round() as i32;
        return result;
    }
    if chrome.hidden.get() {
        if message == WM_NCCALCSIZE {
            return LRESULT(0);
        }
        if message == WM_NCHITTEST {
            let x = (lparam.0 as u16 as i16) as i32;
            let y = ((lparam.0 >> 16) as u16 as i16) as i32;
            let mut rect = RECT::default();
            if unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok() {
                let edge = (RESIZE_EDGE_DIP * f64::from(unsafe { GetDpiForWindow(hwnd) }) / 96.0)
                    .round() as i32;
                let left = x < rect.left + edge;
                let right = x >= rect.right - edge;
                let top = y < rect.top + edge;
                let bottom = y >= rect.bottom - edge;
                let hit = match (left, right, top, bottom) {
                    (true, _, true, _) => HTTOPLEFT,
                    (_, true, true, _) => HTTOPRIGHT,
                    (true, _, _, true) => HTBOTTOMLEFT,
                    (_, true, _, true) => HTBOTTOMRIGHT,
                    (true, _, _, _) => HTLEFT,
                    (_, true, _, _) => HTRIGHT,
                    (_, _, true, _) => HTTOP,
                    (_, _, _, true) => HTBOTTOM,
                    _ => HTCLIENT,
                };
                return LRESULT(hit as isize);
            }
        }
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}
