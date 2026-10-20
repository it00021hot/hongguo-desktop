//! 窗口系统原语的门面：业务模块（小屏 / 竖屏 / 隐身 / 老板键 / 单实例）
//! 只调这里，不写任何 `#[cfg(target_os)]`——平台实现的差异全部落在
//! [`windows`] / [`macos`] 两个子树里。
//!
//! 与既有分层的关系：
//! - [`crate::win32`]：Win32 裸 FFI（user32 / dwmapi / 全局快捷键 / 命名
//!   互斥体），Windows 平台实现的底座；
//! - [`crate::macos`]：AppKit 裸 FFI（objc2），macOS 平台实现的底座；
//! - 本模块：**业务语义层**——小屏落位、竖屏几何、置顶、藏显、快照还原，
//!   两端各自的坐标/边框换算都在各自实现里，调用方看到的是一套统一 API。

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub(crate) use windows::*;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(crate) use macos::*;

/// 窗口外框矩形。坐标空间随平台（Windows：物理像素、左上原点；macOS：
/// AppKit 点、左下原点），只在快照/还原这一对操作里往返，业务不解读。
#[derive(Clone, Copy, Debug)]
pub(crate) struct OuterRect {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) w: f64,
    pub(crate) h: f64,
}

impl OuterRect {
    pub(crate) const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
}

/// 进入紧凑形态前的窗口快照。
#[derive(Clone, Copy, Debug)]
pub(crate) struct WindowSnapshot {
    pub(crate) rect: OuterRect,
    pub(crate) maximized: bool,
}

/// 小屏尺寸（Tauri MINI_W/MINI_H 同参）。
pub(crate) const MINI_W: f64 = 480.0;
pub(crate) const MINI_H: f64 = 270.0;

/// 小屏落位边距（工作区右下角，Tauri MINI_MARGIN 同参）。
pub(crate) const MINI_MARGIN: f64 = 16.0;

/// 当前所有显示器的矩形 `(x, y, w, h)`（逻辑像素，与 gpui 窗口 bounds 同一
/// 坐标空间）——只有 Windows 的记忆槽位用这套 gpui 坐标；macOS 走
/// [`macos::memory_display_rects`] 的 AppKit 全局外框（gpui 在 macOS 丢了
/// 显示器原点，多屏必错位）。
#[cfg(target_os = "windows")]
pub(crate) fn display_rects(cx: &gpui_kit::App) -> Vec<(f32, f32, f32, f32)> {
    cx.displays()
        .iter()
        .map(|display| {
            let b = display.bounds();
            (
                b.origin.x.into(),
                b.origin.y.into(),
                b.size.width.into(),
                b.size.height.into(),
            )
        })
        .collect()
}
