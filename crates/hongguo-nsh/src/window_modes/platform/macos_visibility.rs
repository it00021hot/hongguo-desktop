use crate::window_modes::{geometry::Rect, occlusion};
use nsh_platform_macos::{
    objc2::{rc::Retained, runtime::AnyObject},
    objc2_app_kit::{NSScreen, NSWindow},
    objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString},
};

type WindowInfo = NSDictionary<AnyObject, AnyObject>;
const ON_SCREEN_ONLY: u32 = 1 << 0;
const EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    // CFArray/CFDictionary are toll-free bridged to NSArray/NSDictionary (+1 ownership).
    fn CGWindowListCopyWindowInfo(options: u32, relative_to: u32) -> *mut NSArray<WindowInfo>;
}

fn number(info: &WindowInfo, key: &str) -> Result<f64, String> {
    info.objectForKey(&NSString::from_str(key))
        .and_then(|value| value.downcast_ref::<NSNumber>().map(NSNumber::doubleValue))
        .ok_or_else(|| format!("window metadata is missing {key}"))
}

fn bounds(info: &WindowInfo) -> Result<Rect, String> {
    let value = info
        .objectForKey(&NSString::from_str("kCGWindowBounds"))
        .ok_or("window metadata is missing bounds")?;
    let bounds = value
        .downcast_ref::<WindowInfo>()
        .ok_or("invalid window bounds")?;
    Ok(Rect {
        x: number(bounds, "X")?,
        y: number(bounds, "Y")?,
        width: number(bounds, "Width")?,
        height: number(bounds, "Height")?,
    })
}

pub(super) fn exposed(window: &NSWindow) -> Result<bool, String> {
    // The window server returns front-to-back metadata; no image capture or window titles.
    let windows = unsafe {
        Retained::from_raw(CGWindowListCopyWindowInfo(
            ON_SCREEN_ONLY | EXCLUDE_DESKTOP_ELEMENTS,
            0,
        ))
    }
    .ok_or("window server did not return window metadata")?;
    let id = f64::from(window.windowNumber() as u32);
    let mut covers = Vec::new();
    let mut target = None;
    for info in windows.iter() {
        if number(&info, "kCGWindowNumber")? == id {
            target = Some(bounds(&info)?);
            break;
        }
        if number(&info, "kCGWindowOwnerPID")? == f64::from(std::process::id())
            || number(&info, "kCGWindowAlpha")? < 1.0
        {
            continue;
        }
        covers.push(bounds(&info)?);
    }
    let Some(target) = target else {
        return Ok(false);
    };
    let screens = NSScreen::screens(
        nsh_platform_macos::objc2::MainThreadMarker::new().expect("AppKit thread"),
    );
    let primary = screens
        .firstObject()
        .ok_or("no display is available")?
        .frame();
    let top = primary.origin.y + primary.size.height;
    let mut remaining: Vec<_> = screens
        .iter()
        .filter_map(|screen| {
            let frame = screen.frame();
            occlusion::intersect(
                target,
                Rect {
                    x: frame.origin.x,
                    y: top - frame.origin.y - frame.size.height,
                    width: frame.size.width,
                    height: frame.size.height,
                },
            )
        })
        .collect();
    for cover in covers {
        occlusion::subtract(&mut remaining, cover);
        if remaining.is_empty() {
            return Ok(false);
        }
    }
    Ok(!remaining.is_empty())
}
