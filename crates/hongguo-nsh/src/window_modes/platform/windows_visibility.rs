use nsh_platform_windows::windows::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT},
        Graphics::{
            Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
            Gdi::{
                CombineRgn, CreateRectRgn, CreateRectRgnIndirect, DeleteObject,
                EnumDisplayMonitors, GDI_REGION_TYPE, GetWindowRgn, HDC, HGDIOBJ, HMONITOR, HRGN,
                NULLREGION, OffsetRgn, RGN_AND, RGN_DIFF, RGN_OR,
            },
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GWL_EXSTYLE, GetLayeredWindowAttributes, GetWindowLongPtrW, GetWindowRect,
            IsIconic, IsWindowVisible, LAYERED_WINDOW_ATTRIBUTES_FLAGS, LWA_ALPHA, LWA_COLORKEY,
            WS_EX_LAYERED, WS_EX_TRANSPARENT,
        },
    },
    core::BOOL,
};

pub(super) fn exposed(window: HWND) -> Result<bool, String> {
    if !shown(window) {
        return Ok(false);
    }
    let remaining = window_region(window)?;
    let screens = Region::empty()?;
    let mut monitors = Vec::<RECT>::new();
    unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(monitor),
            LPARAM(&mut monitors as *mut _ as isize),
        )
        .ok()
        .map_err(|error| error.to_string())?;
    }
    for rect in monitors {
        screens.combine(&Region::rect(rect)?, RGN_OR)?;
    }
    if remaining.combine(&screens, RGN_AND)? == NULLREGION {
        return Ok(false);
    }
    let mut windows = Vec::<HWND>::new();
    unsafe { EnumWindows(Some(top_level), LPARAM(&mut windows as *mut _ as isize)) }
        .map_err(|error| error.to_string())?;
    for above in windows {
        if above == window {
            return Ok(true);
        }
        if !opaque(above) {
            continue;
        }
        // Another process may destroy its window between enumeration and this query.
        let Ok(cover) = window_region(above) else {
            continue;
        };
        if remaining.combine(&cover, RGN_DIFF)? == NULLREGION {
            return Ok(false);
        }
    }
    Ok(false)
}

fn shown(window: HWND) -> bool {
    let mut cloaked = 0_u32;
    unsafe {
        IsWindowVisible(window).as_bool()
            && !IsIconic(window).as_bool()
            && DwmGetWindowAttribute(
                window,
                DWMWA_CLOAKED,
                (&mut cloaked as *mut u32).cast(),
                size_of::<u32>() as u32,
            )
            .is_ok()
            && cloaked == 0
    }
}

fn opaque(window: HWND) -> bool {
    if !shown(window) {
        return false;
    }
    unsafe {
        let style = GetWindowLongPtrW(window, GWL_EXSTYLE) as u32;
        if style & WS_EX_TRANSPARENT.0 != 0 {
            return false;
        }
        if style & WS_EX_LAYERED.0 == 0 {
            return true;
        }
        let mut alpha = u8::MAX;
        let mut flags = LAYERED_WINDOW_ATTRIBUTES_FLAGS::default();
        GetLayeredWindowAttributes(window, None, Some(&mut alpha), Some(&mut flags)).is_ok()
            && !flags.contains(LWA_COLORKEY)
            && (!flags.contains(LWA_ALPHA) || alpha == u8::MAX)
    }
}

fn window_region(window: HWND) -> Result<Region, String> {
    let mut frame = RECT::default();
    unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut frame as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        )
    }
    .map_err(|error| error.to_string())?;
    let region = Region::empty()?;
    if unsafe { GetWindowRgn(window, region.0) }.0 == 0 {
        return Region::rect(frame);
    }
    let mut outer = RECT::default();
    unsafe { GetWindowRect(window, &mut outer) }.map_err(|error| error.to_string())?;
    if unsafe { OffsetRgn(region.0, outer.left, outer.top) }.0 == 0 {
        return Err("Cannot position window visibility region".into());
    }
    region.combine(&Region::rect(frame)?, RGN_AND)?;
    Ok(region)
}

struct Region(HRGN);
impl Region {
    fn empty() -> Result<Self, String> {
        Self::from_handle(unsafe { CreateRectRgn(0, 0, 0, 0) })
    }
    fn rect(rect: RECT) -> Result<Self, String> {
        Self::from_handle(unsafe { CreateRectRgnIndirect(&rect) })
    }
    fn from_handle(region: HRGN) -> Result<Self, String> {
        if region.is_invalid() {
            return Err("Cannot allocate window visibility region".into());
        }
        Ok(Self(region))
    }
    fn combine(
        &self,
        other: &Self,
        mode: nsh_platform_windows::windows::Win32::Graphics::Gdi::RGN_COMBINE_MODE,
    ) -> Result<GDI_REGION_TYPE, String> {
        let result = unsafe { CombineRgn(Some(self.0), Some(self.0), Some(other.0), mode) };
        if result.0 == 0 {
            return Err("Cannot combine window visibility regions".into());
        }
        Ok(result)
    }
}
impl Drop for Region {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.0.0));
        }
    }
}

unsafe extern "system" fn top_level(window: HWND, data: LPARAM) -> BOOL {
    unsafe {
        (&mut *(data.0 as *mut Vec<HWND>)).push(window);
    }
    BOOL(1)
}
unsafe extern "system" fn monitor(_: HMONITOR, _: HDC, rect: *mut RECT, data: LPARAM) -> BOOL {
    unsafe {
        (&mut *(data.0 as *mut Vec<RECT>)).push(*rect);
    }
    BOOL(1)
}
