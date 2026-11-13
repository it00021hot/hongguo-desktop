//! GPUI 视图下的软解帧显示层；所有 AppKit 操作留在主线程。

use hongguo_player_api::{HostWindow, PresenterBounds, PresenterFit};
use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject},
};
use objc2_app_kit::NSView;
use objc2_foundation::{NSPoint, NSRect, NSString};
use objc2_quartz_core::CATransaction;
use raw_window_handle::RawWindowHandle;
use std::sync::Mutex;

struct Host {
    content: Retained<NSView>,
    view: Retained<NSView>,
    layer: Retained<AnyObject>,
    fit: PresenterFit,
}

// SAFETY: The global host is accessed only from GPUI's main-thread presenter calls.
unsafe impl Send for Host {}
static HOST: Mutex<Option<Host>> = Mutex::new(None);

fn without_implicit_actions(f: impl FnOnce()) {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    f();
    CATransaction::commit();
}

fn ns_view(host: &HostWindow) -> Option<&NSView> {
    let RawWindowHandle::AppKit(handle) = host.handle else {
        return None;
    };
    // SAFETY: The live HostWindow retains its GPUI NSView for the presenter call.
    Some(unsafe { &*(handle.ns_view.as_ptr() as *const NSView) })
}

pub fn prepare_window(host: &HostWindow) {
    let Some(window) = ns_view(host).and_then(NSView::window) else {
        return;
    };
    // SAFETY: Public NSColor/NSWindow selectors, called on the main thread.
    unsafe {
        let black: *mut AnyObject =
            msg_send![AnyClass::get(c"NSColor").expect("NSColor"), blackColor];
        let _: () = msg_send![&*window, setBackgroundColor: black];
    }
}

fn gravity(fit: PresenterFit) -> Retained<NSString> {
    NSString::from_str(match fit {
        PresenterFit::Contain => "AVLayerVideoGravityResizeAspect",
        PresenterFit::Cover => "AVLayerVideoGravityResizeAspectFill",
    })
}

pub fn ensure(window: &HostWindow, layer: &AnyObject) -> bool {
    let Some(gpui_view) = ns_view(window) else {
        return false;
    };
    let Some(content) = gpui_view.window().and_then(|window| window.contentView()) else {
        return false;
    };
    let mut slot = HOST.lock().expect("视频宿主锁 poisoned");
    if let Some(host) = slot.as_mut() {
        // SAFETY: Both layers are retained, and addSublayer retains the replacement.
        unsafe {
            without_implicit_actions(|| {
                if !std::ptr::eq(&*host.layer, layer) {
                    let _: () = msg_send![&*host.layer, removeFromSuperlayer];
                    let parent: *mut AnyObject = msg_send![&*host.view, layer];
                    let _: () = msg_send![layer, setFrame: host.view.bounds()];
                    let _: () = msg_send![layer, setVideoGravity: &*gravity(host.fit)];
                    let _: () = msg_send![parent, addSublayer: layer];
                    host.layer = Retained::retain(layer as *const AnyObject as *mut AnyObject)
                        .expect("retained video layer");
                }
                let _: () = msg_send![&*host.view, setHidden: false];
            });
        }
        return true;
    }
    // SAFETY: Public NSView/CALayer methods; the retained view is attached below GPUI.
    let view: Retained<NSView> =
        unsafe { msg_send![AnyClass::get(c"NSView").expect("NSView"), new] };
    unsafe {
        without_implicit_actions(|| {
            let _: () = msg_send![&*view, setFrame: content.bounds()];
            let _: () = msg_send![&*view, setWantsLayer: true];
            let _: () = msg_send![&*view, setAutoresizingMask: 18usize];
            let parent: *mut AnyObject = msg_send![&*view, layer];
            let _: () = msg_send![layer, setFrame: view.bounds()];
            let _: () = msg_send![layer, setVideoGravity: &*gravity(PresenterFit::Contain)];
            let _: () = msg_send![parent, addSublayer: layer];
            let _: () = msg_send![&*content, addSubview: &*view, positioned: -1isize, relativeTo: gpui_view];
        });
        *slot = Some(Host {
            content,
            view,
            layer: Retained::retain(layer as *const AnyObject as *mut AnyObject)
                .expect("retained video layer"),
            fit: PresenterFit::Contain,
        });
    }
    true
}

pub fn submit(bounds: Option<PresenterBounds>, fit: PresenterFit, visible: bool) {
    let mut slot = HOST.lock().expect("视频宿主锁 poisoned");
    let Some(host) = slot.as_mut() else {
        return;
    };
    // SAFETY: The retained view and sample-buffer layer belong to the main thread.
    unsafe {
        without_implicit_actions(|| {
            if let Some(bounds) = bounds.filter(|bounds| !bounds.is_empty()) {
                let height = f64::from(bounds.height);
                let frame = NSRect {
                    origin: NSPoint {
                        x: f64::from(bounds.x),
                        y: (host.content.bounds().size.height - f64::from(bounds.y) - height)
                            .max(0.0),
                    },
                    size: objc2_foundation::NSSize {
                        width: f64::from(bounds.width),
                        height,
                    },
                };
                let _: () = msg_send![&*host.view, setFrame: frame];
                let inner = NSRect {
                    origin: NSPoint { x: 0.0, y: 0.0 },
                    size: frame.size,
                };
                let _: () = msg_send![&*host.layer, setFrame: inner];
                let _: () = msg_send![&*host.layer, setVideoGravity: &*gravity(fit)];
                host.fit = fit;
            }
            let _: () = msg_send![&*host.view, setHidden: !visible];
        });
    }
}

pub fn hide() {
    if let Some(host) = HOST.lock().expect("视频宿主锁 poisoned").as_ref() {
        // SAFETY: Called on the main thread with a retained NSView.
        let _: () = unsafe { msg_send![&*host.view, setHidden: true] };
    }
}
