//! 原生视频宿主视图：挂在 GPUI 视图**下面**的一层 NSView + AVPlayerLayer。
//!
//! 透明洞机制：
//! 1. 系统后端时**壳**把窗口背景设成透明（Metal 层 clear alpha=0），这里再把
//!    窗口铺黑，桌面不会从洞里透出来；黑边由 GPUI 自己画（两平台同一段代码）；
//! 2. GPUI 壳根/舞台在播放时只画透明背景，叠层（控制条/弹幕/浮层）仍由
//!    GPUI 画在视频层之上；
//! 3. 视频 NSView 作为 contentView 的子视图插在 GPUIView 之下，rect 跟随
//!    `stage_bounds`（GPUI 逻辑像素、左上原点），这里翻成 AppKit 左下原点。
//!
//! 线程纪律：`prepare_window` / `ensure` / `submit` / `hide` 只在主线程
//! （GPUI render/回调）调用；全局宿主跨集复用，避免每集重建视图。

use std::ffi::CStr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use hongguo_player_api::{HostWindow, PresenterBounds, PresenterFit};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSView;
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::RawWindowHandle;

use super::avf;

/// 全局唯一的视频宿主（GPUI 壳单窗口）。裸指针由 contentView/父 layer 持有，
/// 进程内不回收——窗口关闭即随 AppKit 树释放。
struct Host {
    content: *mut AnyObject,
    view: *mut AnyObject,
    player_layer: *mut AnyObject,
    last_frame: Option<(f64, f64, f64, f64)>,
}

// SAFETY: 只在主线程访问，且指针指向的对象由 AppKit 视图树持有。
unsafe impl Send for Host {}

static HOST: Mutex<Option<Host>> = Mutex::new(None);
/// 自检只打一次；排障时看这一组就能判断是「层被盖住」还是「窗口不透明」。
static SELF_CHECKED: AtomicBool = AtomicBool::new(false);

fn class_name(obj: *mut AnyObject) -> String {
    if obj.is_null() {
        return "<nil>".to_owned();
    }
    // SAFETY: object_getClassName 对有效 ObjC 对象返回静态 C 字符串。
    unsafe { CStr::from_ptr(objc2::ffi::object_getClassName(obj)) }
        .to_string_lossy()
        .into_owned()
}

fn rect_text(rect: NSRect) -> String {
    format!(
        "{:.0},{:.0} {:.0}×{:.0}",
        rect.origin.x, rect.origin.y, rect.size.width, rect.size.height
    )
}

/// 打印合成关系：窗口 opaque、GPUIView  backing layer、contentView 子视图顺序、
/// 宿主/播放层 frame/opacity/hidden。只在系统后端首次可见提交时打一次。
fn log_composition(tag: &str) {
    let guard = HOST.lock().expect("视频宿主锁 poisoned");
    let Some(host) = guard.as_ref() else {
        return;
    };
    let Some(window) = (unsafe { (&*(host.content as *const NSView)).window() }) else {
        return;
    };
    let window_opaque: bool = unsafe { msg_send![&*window, isOpaque] };
    let window_bg: *mut AnyObject = unsafe { msg_send![&*window, backgroundColor] };
    let content = host.content;
    let content_layer: *mut AnyObject = unsafe { msg_send![content, layer] };
    let content_layer_opaque: bool = unsafe {
        if content_layer.is_null() {
            false
        } else {
            msg_send![content_layer, isOpaque]
        }
    };
    let content_wants_layer: bool = unsafe { msg_send![content, wantsLayer] };
    let content_frame: NSRect = unsafe { msg_send![content, bounds] };
    let subviews: *mut AnyObject = unsafe { msg_send![content, subviews] };
    let count: usize = unsafe { msg_send![subviews, count] };
    let mut order = String::new();
    for i in 0..count {
        let sv: *mut AnyObject = unsafe { msg_send![subviews, objectAtIndex: i] };
        let frame: NSRect = unsafe { msg_send![sv, frame] };
        let hidden: bool = unsafe { msg_send![sv, isHidden] };
        let layer: *mut AnyObject = unsafe { msg_send![sv, layer] };
        let layer_opaque: bool = unsafe {
            if layer.is_null() {
                false
            } else {
                msg_send![layer, isOpaque]
            }
        };
        if i > 0 {
            order.push_str(" | ");
        }
        order.push_str(&format!(
            "#{i} {} {}{} layer_opaque={}",
            class_name(sv),
            rect_text(frame),
            if hidden { " hidden" } else { "" },
            layer_opaque
        ));
    }
    let mut gpui = std::ptr::null_mut();
    for i in 0..count {
        let sv: *mut AnyObject = unsafe { msg_send![subviews, objectAtIndex: i] };
        if sv != host.view {
            gpui = sv;
            break;
        }
    }
    let gpui_layer: *mut AnyObject = unsafe { msg_send![gpui, layer] };
    let gpui_layer_opaque: bool = unsafe {
        if gpui_layer.is_null() {
            false
        } else {
            msg_send![gpui_layer, isOpaque]
        }
    };
    let gpui_layer_class = class_name(gpui_layer);
    let host_frame: NSRect = unsafe { msg_send![host.view, frame] };
    let host_hidden: bool = unsafe { msg_send![host.view, isHidden] };
    let host_alpha: f64 = unsafe { msg_send![host.view, alphaValue] };
    let host_layer: *mut AnyObject = unsafe { msg_send![host.view, layer] };
    let host_layer_opaque: bool = unsafe {
        if host_layer.is_null() {
            false
        } else {
            msg_send![host_layer, isOpaque]
        }
    };
    let player_frame: NSRect = unsafe { msg_send![host.player_layer, frame] };
    let player_hidden: bool = unsafe { msg_send![host.player_layer, isHidden] };
    let player_opacity: f32 = unsafe { msg_send![host.player_layer, opacity] };
    let player_ready: bool = unsafe { msg_send![host.player_layer, isReadyForDisplay] };
    log::info!(
        "[AvPresenter][selfcheck:{tag}] window opaque={window_opaque} bg={} content {} layer={} layer_opaque={} wantsLayer={} subviews=[{order}] gpui={} layer_class={} layer_opaque={} host={} hidden={host_hidden} alpha={host_alpha:.2} layer_opaque={host_layer_opaque} player={} hidden={player_hidden} opacity={player_opacity:.2} ready={player_ready}",
        class_name(window_bg),
        rect_text(content_frame),
        class_name(content_layer),
        content_layer_opaque,
        content_wants_layer,
        class_name(gpui),
        gpui_layer_class,
        gpui_layer_opaque,
        rect_text(host_frame),
        rect_text(player_frame),
    );
}

/// 宿主窗口背后的 NSView。
fn ns_view(host: &HostWindow) -> Option<&NSView> {
    let RawWindowHandle::AppKit(handle) = host.handle else {
        return None;
    };
    // SAFETY: gpui-pre-macos 的 AppKitWindowHandle.ns_view 是窗口持有的 GPUIView。
    Some(unsafe { &*(handle.ns_view.as_ptr() as *const NSView) })
}

/// 系统后端启动时调用一次：窗口背景铺黑。
///
/// 透明是由**壳**设的（GPUI 的 `set_background_appearance(Transparent)`，Metal
/// 层清成 alpha=0 才有洞）——窗口外观属于 UI 框架语义，不在播放器实现里。
/// 这里铺黑：视频 contain 的黑边/尚未出画的区域仍然是黑，而不是桌面。
#[cfg(target_os = "macos")]
pub fn prepare_window(host: &HostWindow) {
    let Some(view) = ns_view(host) else { return };
    let Some(ns_window) = view.window() else {
        return;
    };
    // SAFETY: NSColor 是公开类；setBackgroundColor: 只读引用。
    let black: *mut AnyObject = unsafe {
        msg_send![
            objc2::runtime::AnyClass::get(c"NSColor").expect("NSColor"),
            blackColor
        ]
    };
    let _: () = unsafe { msg_send![&*ns_window, setBackgroundColor: black] };
    let opaque: bool = unsafe { msg_send![&*ns_window, isOpaque] };
    let background: *mut AnyObject = unsafe { msg_send![&*ns_window, backgroundColor] };
    let view_layer: *mut AnyObject = unsafe { msg_send![view, layer] };
    let view_layer_opaque: bool = unsafe {
        if view_layer.is_null() {
            false
        } else {
            msg_send![view_layer, isOpaque]
        }
    };
    log::info!(
        "[AvPresenter] 窗口透明层就绪（背景铺黑）：isOpaque={opaque} bg={} GPUIView layer={} opaque={view_layer_opaque}",
        class_name(background),
        class_name(view_layer)
    );
    log_composition("prepare");
}

/// 挂上/切换当前播放器；返回 false 表示窗口不可用。
pub fn ensure(host: &HostWindow, player: &AnyObject) -> bool {
    let Some(view) = ns_view(host) else {
        log::warn!("[AvPresenter] ensure: 取不到 NSView");
        return false;
    };
    let Some(ns_window) = view.window() else {
        log::warn!("[AvPresenter] ensure: NSView 没有 window");
        return false;
    };
    let Some(content) = ns_window.contentView() else {
        log::warn!("[AvPresenter] ensure: window 没有 contentView");
        return false;
    };
    let content_ptr = Retained::as_ptr(&content) as *mut AnyObject;
    let mut host = HOST.lock().expect("视频宿主锁 poisoned");
    if let Some(host) = host.as_ref() {
        // 复用宿主，换播放器即可——切集不重建 NSView。
        let _: () = unsafe { msg_send![host.player_layer, setPlayer: player] };
        let _: () = unsafe { msg_send![host.view, setHidden: false] };
        return true;
    }

    let content_frame = content.bounds();
    // SAFETY: 公开 NSView 初始化；frame 用 contentView 的 bounds。
    let alloc: *mut AnyObject = unsafe {
        msg_send![
            objc2::runtime::AnyClass::get(c"NSView").expect("NSView"),
            alloc
        ]
    };
    let host_view: *mut AnyObject = unsafe { msg_send![alloc, initWithFrame: content_frame] };
    if host_view.is_null() {
        return false;
    }
    let _: () = unsafe { msg_send![host_view, setWantsLayer: true] };
    let _: () = unsafe { msg_send![host_view, setAutoresizingMask: 18usize] };
    let host_layer: *mut AnyObject = unsafe { msg_send![host_view, layer] };
    if host_layer.is_null() {
        log::error!("[AvPresenter] 视频宿主 NSView 没有 layer");
        return false;
    }

    let player_layer = avf::player_layer(player);
    let bounds = NSRect {
        origin: NSPoint { x: 0.0, y: 0.0 },
        size: NSSize {
            width: content_frame.size.width,
            height: content_frame.size.height,
        },
    };
    let gravity = NSString::from_str("AVLayerVideoGravityResizeAspect");
    // SAFETY: 两个 layer 都是有效 ObjC 对象；sublayer 增加引用。
    unsafe {
        let _: () = msg_send![&*player_layer, setFrame: bounds];
        let _: () = msg_send![&*player_layer, setAutoresizingMask: 18u32];
        let _: () = msg_send![&*player_layer, setVideoGravity: &*gravity];
        let _: () = msg_send![host_layer, addSublayer: &*player_layer];
        // GPUIView 是 contentView 的 subview；视频视图插在它下面（-1 = below）。
        let _: () = msg_send![
            content_ptr,
            addSubview: host_view,
            positioned: -1isize,
            relativeTo: view
        ];
    }
    *host = Some(Host {
        content: content_ptr,
        view: host_view,
        player_layer: Retained::as_ptr(&player_layer) as *mut AnyObject,
        last_frame: None,
    });
    // player_layer 由 host_layer 的 sublayers 持有；我们的 Retained 可安全释放。
    drop(player_layer);
    drop(host);
    log::info!("[AvPresenter] 视频宿主已插入 contentView（GPUIView 之下）");
    true
}

/// 提交舞台矩形/适配/可见性（每帧调用，幂等）。
pub fn submit(bounds: Option<PresenterBounds>, fit: PresenterFit, visible: bool) {
    if visible && !SELF_CHECKED.swap(true, Ordering::Relaxed) {
        log_composition("visible");
    }
    let mut host = HOST.lock().expect("视频宿主锁 poisoned");
    let Some(host) = host.as_mut() else { return };
    let content_bounds: NSRect = unsafe { msg_send![host.content, bounds] };
    let content_height = content_bounds.size.height;
    if let Some(bounds) = bounds
        && !bounds.is_empty()
    {
        // GPUI 逻辑像素左上原点 → AppKit 左下原点。
        let height = f64::from(bounds.height);
        let y = (content_height - f64::from(bounds.y) - height).max(0.0);
        let frame = NSRect {
            origin: NSPoint {
                x: f64::from(bounds.x),
                y,
            },
            size: NSSize {
                width: f64::from(bounds.width),
                height,
            },
        };
        let inner = NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: frame.size,
        };
        let gravity = NSString::from_str(match fit {
            PresenterFit::Contain => "AVLayerVideoGravityResizeAspect",
            PresenterFit::Cover => "AVLayerVideoGravityResizeAspectFill",
        });
        // SAFETY: 宿主对象由视图树持有；frame/gravity 是常规属性。
        unsafe {
            let _: () = msg_send![host.view, setFrame: frame];
            let _: () = msg_send![host.player_layer, setFrame: inner];
            let _: () = msg_send![host.player_layer, setVideoGravity: &*gravity];
            let _: () = msg_send![host.view, setHidden: !visible];
            let key = (
                frame.origin.x,
                frame.origin.y,
                frame.size.width,
                frame.size.height,
            );
            if host.last_frame != Some(key) {
                host.last_frame = Some(key);
                let ready: bool = msg_send![host.player_layer, isReadyForDisplay];
                log::info!(
                    "[AvPresenter] 舞台 {:.0},{:.0} {:.0}×{:.0}（content h={content_height:.0}，fit={fit:?}，visible={visible}，layer_ready={ready}）",
                    key.0,
                    key.1,
                    key.2,
                    key.3
                );
            }
        }
    } else {
        // SAFETY: 只更新可见性。
        let _: () = unsafe { msg_send![host.view, setHidden: !visible] };
    }
}

/// 收起视频层（离开首页/播放失败）。
pub fn hide() {
    let host = HOST.lock().expect("视频宿主锁 poisoned");
    if let Some(host) = host.as_ref() {
        // SAFETY: 宿主对象由视图树持有。
        let _: () = unsafe { msg_send![host.view, setHidden: true] };
        let nil: *mut AnyObject = std::ptr::null_mut();
        let _: () = unsafe { msg_send![host.player_layer, setPlayer: nil] };
    }
}
