//! 宿主窗口的跨平台描述：壳解出原生句柄后交给系统播放器实现。

use raw_window_handle::RawWindowHandle;

/// 壳交给系统播放器的窗口信息。
///
/// 壳负责从 GPUI 的窗口解出原生句柄（`HasWindowHandle`）；系统播放器实现只认
/// 这个结构——所以实现 crate 既不依赖 gpui，也不依赖壳的任何类型。
#[derive(Clone, Copy, Debug)]
pub struct HostWindow {
    /// 原生窗口句柄（Windows: HWND；macOS: GPUIView 的 NSView）。
    pub handle: RawWindowHandle,
    /// 窗口 DPI 缩放：逻辑像素 → 物理像素（Windows 的合成坐标要物理像素；
    /// macOS 走 AppKit 点，直接乘 1）。
    pub scale_factor: f32,
}
