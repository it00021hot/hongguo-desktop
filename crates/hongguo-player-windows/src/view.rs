//! 原生视频宿主：合成视觉树里、GPUI 内容之下的一层 DirectComposition 视觉。
//!
//! 透明洞机制（与 macOS 实现的 `view.rs` 同构）：
//! 1. 系统后端时**壳**把窗口背景设成透明 —— DComp 的清屏 alpha=0，窗口类背景
//!    刷仍是黑，所以舞台外不会漏出桌面；
//! 2. GPUI 壳根/舞台在播放时只画透明背景（`home/stage.rs` 的 `video_hole`），
//!    黑边由 GPUI 自己画；
//! 3. 视频位（vendored `gpui-windows` 补丁）是渲染器视觉树里内容之下的那个
//!    `IDCompositionVisual`，引擎把画面渲染进去，舞台几何由 [`submit`] 每帧提交。
//!
//! 线程纪律：`prepare_window` / `submit` 只在主线程（GPUI render/回调）调用。
//! 窗口背景那一步不在这里：那是 UI 框架的窗口语义，不属于播放器实现。

use std::sync::Mutex;

use gpui_pre_windows::system_video::{SystemVideoLayer, SystemVideoStage};
use hongguo_player_api::HostWindow;
use raw_window_handle::RawWindowHandle;
use windows::Win32::Foundation::HWND;

/// 壳窗口的 HWND（系统后端启动时登记；引擎创建要用它取视频位）。
static WINDOW: Mutex<Option<isize>> = Mutex::new(None);

fn hwnd(host: &HostWindow) -> Option<HWND> {
    let RawWindowHandle::Win32(handle) = host.handle else {
        return None;
    };
    Some(HWND(handle.hwnd.get() as *mut core::ffi::c_void))
}

/// 系统后端启动时调用一次：登记宿主窗口 HWND（透明背景由壳负责）。
pub(super) fn prepare_window(host: &HostWindow) {
    let Some(hwnd) = hwnd(host) else {
        log::error!("[WmPresenter] 宿主窗口不是 Win32 句柄，系统后端不可用");
        return;
    };
    *WINDOW.lock().expect("窗口句柄锁 poisoned") = Some(hwnd.0 as isize);
    log::info!(
        "[WmPresenter] 视频位就绪（HWND {:?}，scale={:.2}，视频位={}）",
        hwnd,
        host.scale_factor,
        current_layer().is_some()
    );
}

/// 当前窗口的视频位。渲染器重建（设备丢失）后这里拿到的是新一层。
pub(super) fn current_layer() -> Option<SystemVideoLayer> {
    let hwnd = (*WINDOW.lock().expect("窗口句柄锁 poisoned"))?;
    gpui_pre_windows::system_video::system_video_layer(HWND(hwnd as *mut core::ffi::c_void))
}

/// 提交舞台几何；返回 false 表示这一层已失效（调用方下一帧重取）。
pub(super) fn submit(layer: &SystemVideoLayer, stage: Option<&SystemVideoStage>) -> bool {
    match layer.set_stage(stage) {
        Ok(()) => true,
        Err(error) => {
            log::warn!("[WmPresenter] 视频位提交失败（渲染器可能重建过）: {error}");
            false
        }
    }
}
