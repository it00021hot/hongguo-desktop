//! Phase 0 可行性 spike：`AVPlayer` + `AVPlayerLayer` 放进普通 GPUI 窗口。
//!
//! 门控：`HONGGUO_AV_SPIKE=1` 时 `main` 在装配任何业务之前转入本模块，
//! 默认启动路径完全不经过这里。样片路径可用 `HONGGUO_AV_SPIKE_SAMPLE`
//! 覆盖，默认 `target/av-spike/sample-fmp4.mp4`（生成脚本见
//! `scripts/make-av-spike-sample.swift`）。
//!
//! 本模块只做 macos：`AVFoundation` 是系统框架，不引入任何外部 crate。

mod avf;
mod loader;
mod measure;

use std::time::Duration;

use gpui_kit::div;
use gpui_kit::prelude::*;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSScreen, NSView};
use objc2_foundation::NSString;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

const ENABLE_ENV: &str = "HONGGUO_AV_SPIKE";
const SAMPLE_ENV: &str = "HONGGUO_AV_SPIKE_SAMPLE";
const DEFAULT_SAMPLE: &str = "target/av-spike/sample-fmp4.mp4";

pub(crate) fn requested() -> bool {
    std::env::var(ENABLE_ENV).is_ok_and(|value| value == "1")
}

pub(crate) fn run() {
    let path = std::env::var(SAMPLE_ENV).unwrap_or_else(|_| DEFAULT_SAMPLE.to_owned());
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            log::error!("[AvSpike] 读不到样片 {path}: {error}（可用 {SAMPLE_ENV} 指定路径）");
            std::process::exit(2);
        }
    };
    log::info!("[AvSpike] 样片 {path}（{} 字节）", bytes.len());
    loader::set_bytes(bytes);

    let player = avf::player();
    let player_addr = Retained::as_ptr(&player) as usize;
    let layer = avf::player_layer(&player);
    // 图层强引用 player；泄漏我们这份 Retained 只是为了地址在整个进程内有效
    // （spike 一次性进程，退出即回收）。
    std::mem::forget(player);

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let refresh_hz = objc2::MainThreadMarker::new()
                .and_then(NSScreen::mainScreen)
                .map_or(100.0, |screen| screen.maximumFramesPerSecond() as f64);
            log::info!("[AvSpike] 主屏刷新率 {refresh_hz}Hz");

            let (window, _view) = gpui_kit::open_window(
                gpui_kit::WindowOptions {
                    window_bounds: Some(gpui_kit::WindowBounds::Windowed(
                        gpui_kit::Bounds::centered(
                            None,
                            gpui_kit::size(gpui_kit::px(1280.), gpui_kit::px(720.)),
                            cx,
                        ),
                    )),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| SpikeView),
            )
            .expect("spike 窗口创建失败");

            // 首帧之后再挂原生图层：GPUI 的 NSView 在首帧前可能还没有 layer。
            let layer_addr = Retained::as_ptr(&layer) as usize;
            let (attached_tx, attached_rx) = std::sync::mpsc::channel::<bool>();
            let layer_for_window = layer.clone();
            let _ = window.update(cx, |_, window, _| {
                window.on_next_frame(move |window, _| {
                    let attached = attach_layer(window, &layer_for_window);
                    let _ = attached_tx.send(attached);
                });
            });

            std::thread::Builder::new()
                .name("hongguo-av-spike".to_owned())
                .spawn(
                    move || match attached_rx.recv_timeout(Duration::from_secs(5)) {
                        Ok(true) => measure::run(player_addr, layer_addr, refresh_hz),
                        _ => {
                            log::error!("[AvSpike] AVPlayerLayer 未能挂到窗口，spike 中止");
                            std::process::exit(3);
                        }
                    },
                )
                .expect("spike 测量线程创建失败");
        });
}

struct SpikeView;

impl gpui_kit::Render for SpikeView {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        _cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        div()
            .size_full()
            .bg(gpui_kit::rgb(0x000000))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_color(gpui_kit::rgb(0xffffff))
                    .child("HONGGUO_AV_SPIKE=1 — AVPlayer / AVPlayerLayer"),
            )
    }
}

/// 把 `AVPlayerLayer` 作为子层挂到 GPUI 窗口的 `NSView` 上。
///
/// 用 `msg_send!` 而不是 typed binding：`CALayer` 属于 objc2-quartz-core，
/// 本 crate 目前没有这个依赖，原样保留「零新依赖」约束。
fn attach_layer(window: &gpui_kit::Window, layer: &AnyObject) -> bool {
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        log::error!("[AvSpike] 取不到窗口句柄");
        return false;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        log::error!("[AvSpike] 窗口句柄不是 AppKit");
        return false;
    };
    // SAFETY: gpui-pre-macos 的 AppKitWindowHandle.ns_view 是窗口自身持有的
    // GPUIView；本函数在 gpui 主线程回调里执行。
    let view = unsafe { &*(handle.ns_view.as_ptr() as *const NSView) };
    let mut view_layer: *mut AnyObject = unsafe { msg_send![view, layer] };
    if view_layer.is_null() {
        let _: () = unsafe { msg_send![view, setWantsLayer: true] };
        view_layer = unsafe { msg_send![view, layer] };
    }
    if view_layer.is_null() {
        log::error!("[AvSpike] GPUI 的 NSView 没有 layer");
        return false;
    }

    let bounds = view.bounds();
    let gravity = NSString::from_str("AVLayerVideoGravityResizeAspect");
    // SAFETY: layer/view_layer 都是有效 Objective-C 对象；frame 用 AppKit 的
    // NSRect（CGRect 布局），autoresizingMask 18 = Width|Height。
    unsafe {
        let _: () = msg_send![layer, setFrame: bounds];
        let _: () = msg_send![layer, setAutoresizingMask: 18u32];
        let _: () = msg_send![layer, setZPosition: 1.0f64];
        let _: () = msg_send![layer, setVideoGravity: &*gravity];
        let _: () = msg_send![view_layer, addSublayer: layer];
    }
    log::info!(
        "[AvSpike] AVPlayerLayer 已挂到窗口（{:.0}×{:.0}pt）",
        bounds.size.width,
        bounds.size.height
    );
    true
}
