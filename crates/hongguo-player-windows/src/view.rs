//! 原生视频宿主：把壳窗口解成视频 quad 句柄（frame-server 帧的落点）。
//!
//! 视频 quad 在渲染器里与 GPUI 内容共用同一张渲染目标（vendor 补丁
//! `video_quad`），不再有独立合成视觉：跨画布的缝/错帧/z 序问题结构性消失。
//!
//! 线程纪律：`prepare_window` / 落点操作只在主线程（GPUI render/回调）调用。
//! 窗口背景那一步不在这里：那是 UI 框架的窗口语义，不属于播放器实现。

use std::sync::Mutex;

use gpui_pre_windows::video_quad::{self, VideoQuad};
use hongguo_player_api::HostWindow;
use raw_window_handle::RawWindowHandle;
use windows::Win32::Foundation::HWND;

/// 壳窗口的 HWND（系统后端启动时登记；引擎与视频 quad 都按它取）。
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
        "[WmPresenter] 视频落点就绪（HWND {:?}，scale={:.2}，quad={})",
        hwnd,
        host.scale_factor,
        video().is_some()
    );
}

/// 当前窗口的视频 quad。窗口重建（HWND 变了）后取到的是新窗口的句柄。
pub(super) fn video() -> Option<VideoQuad> {
    let hwnd = (*WINDOW.lock().expect("窗口句柄锁 poisoned"))?;
    video_quad::video_quad(HWND(hwnd as *mut core::ffi::c_void))
}
