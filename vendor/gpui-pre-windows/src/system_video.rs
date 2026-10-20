//! 系统播放器的 DirectComposition 视频位（本仓补丁）。
//!
//! **为什么需要这个模块**：GPUI 在 Windows 上把整窗内容画进一个
//! `CreateSwapChainForComposition` 的 swapchain（AlphaMode 为
//! `DXGI_ALPHA_MODE_PREMULTIPLIED`），合成的视觉树根是渲染器私有的一个
//! `IDCompositionVisual`。系统播放器（Media Foundation Media Engine 的
//! rendering mode）要求应用提供 `IDCompositionVisual` 当画面落点，而这个视觉
//! 必须位于 GPUI 内容视觉**之下**：GPUI 在舞台区清成 alpha=0 时视频才露得
//! 出来，控制条/弹幕/浮层仍由 GPUI 画在上面。上游 0.3.9 没有对外挂点，只能
//! 在这里补一层——与 macOS 侧把 `AVPlayerLayer` 插在 `GPUIView` 之下同一套
//! 结构。
//!
//! 视觉树（容器为根，内容在上、视频位在下）：
//!
//! ```text
//! root(容器)
//!  ├── 内容视觉（GPUI 的 swapchain）
//!  └── 裁剪视觉（舞台矩形）→ 缩放视觉 → 引擎渲染目标
//! ```
//!
//! 裁剪与缩放拆两层：「`SetClip` 作用在视觉自身坐标系、会被本视觉的变换缩放」，
//! 所以裁剪层不承载缩放，缩放层单独做 video → 舞台的映射，`cover` 溢出照裁。
//!
//! **线程纪律**：本模块的调用只发生在 GPUI 主线程（渲染器创建时注册、每帧
//! 提交舞台几何）。MediaEngine 自己往渲染目标里写内容走的是它内部线程，那是
//! DirectComposition 的线程语义，不经过这里。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use windows::core::Result;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::DirectComposition::{
    IDCompositionDevice, IDCompositionRectangleClip, IDCompositionVisual,
};
use windows_numerics::Matrix3x2;

/// 舞台几何（物理像素，窗口客户区左上原点）。
///
/// `scale_*` 与 `offset_*` 把视频从它的原生尺寸映射进舞台：`contain` 时取较小
/// 缩放并居中（留黑边），`cover` 时取较大缩放并居中（溢出的部分由裁剪视觉
/// 裁掉）。换算由调用方做——这里只管摆。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SystemVideoStage {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

/// 某个窗口的视频位句柄（按窗口渲染器注册，设备丢失重建后刷新）。
pub struct SystemVideoLayer {
    device: IDCompositionDevice,
    clip: IDCompositionVisual,
    clip_rect: IDCompositionRectangleClip,
    scale: IDCompositionVisual,
    target: IDCompositionVisual,
    applied: Mutex<Option<SystemVideoStage>>,
}

// SAFETY: 所有方法只在 GPUI 主线程调用（渲染器创建、每帧提交舞台几何），
// 与渲染器本身的线程约束一致；`applied` 是普通值锁。MediaEngine 在内部线程
// 写渲染目标属于 DirectComposition 自己的线程语义，不经本结构。
unsafe impl Send for SystemVideoLayer {}
unsafe impl Sync for SystemVideoLayer {}

impl Clone for SystemVideoLayer {
    fn clone(&self) -> Self {
        Self {
            device: self.device.clone(),
            clip: self.clip.clone(),
            clip_rect: self.clip_rect.clone(),
            scale: self.scale.clone(),
            target: self.target.clone(),
            applied: Mutex::new(*self.applied.lock().expect("视频位锁 poisoned")),
        }
    }
}

impl SystemVideoLayer {
    /// MediaEngine 的渲染目标（`MF_MEDIA_ENGINE_PLAYBACK_VISUAL`）。
    pub fn render_target(&self) -> &IDCompositionVisual {
        &self.target
    }

    /// 提交舞台几何；`None` 表示收起视频位（裁剪成空矩形）。
    ///
    /// 幂等：几何没变就不动 DComp（每帧都会从这里过，重复 Commit 是白花）。
    pub fn set_stage(&self, stage: Option<&SystemVideoStage>) -> Result<()> {
        let Some(stage) = stage else {
            // 视觉没有 hidden 属性；把裁剪收成空矩形最省，且不碰引擎的内容。
            unsafe {
                self.clip_rect.SetRight2(0.0)?;
                self.clip_rect.SetBottom2(0.0)?;
                self.device.Commit()?;
            }
            *self.applied.lock().expect("视频位锁 poisoned") = None;
            return Ok(());
        };
        {
            let mut applied = self.applied.lock().expect("视频位锁 poisoned");
            if applied.as_ref() == Some(stage) {
                return Ok(());
            }
            *applied = Some(*stage);
        }
        let matrix = Matrix3x2 {
            M11: stage.scale_x,
            M12: 0.0,
            M21: 0.0,
            M22: stage.scale_y,
            M31: stage.offset_x,
            M32: stage.offset_y,
        };
        unsafe {
            self.clip.SetOffsetX2(stage.x)?;
            self.clip.SetOffsetY2(stage.y)?;
            self.clip_rect.SetLeft2(0.0)?;
            self.clip_rect.SetTop2(0.0)?;
            self.clip_rect.SetRight2(stage.width)?;
            self.clip_rect.SetBottom2(stage.height)?;
            self.clip.SetClip(&self.clip_rect)?;
            self.scale.SetTransform2(&matrix)?;
            self.device.Commit()?;
        }
        Ok(())
    }
}

fn layers() -> &'static Mutex<HashMap<isize, SystemVideoLayer>> {
    static LAYERS: OnceLock<Mutex<HashMap<isize, SystemVideoLayer>>> = OnceLock::new();
    LAYERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 取窗口的视频位。渲染器重建（设备丢失）后拿到的是新一层。
pub fn system_video_layer(hwnd: HWND) -> Option<SystemVideoLayer> {
    layers()
        .lock()
        .expect("视频位表锁 poisoned")
        .get(&(hwnd.0 as isize))
        .cloned()
}

/// 渲染器建好容器视觉后调用：在内容之下挂出「裁剪 → 缩放 → 渲染目标」三层。
pub(crate) fn install(
    hwnd: HWND,
    device: &IDCompositionDevice,
    container: &IDCompositionVisual,
) -> Result<()> {
    // SAFETY: 全部是同一台合成设备上创建的视觉，调用点在同一线程。
    unsafe {
        let clip = device.CreateVisual()?;
        let scale = device.CreateVisual()?;
        let target = device.CreateVisual()?;
        let clip_rect = device.CreateRectangleClip()?;
        // 初始收起：空裁剪，等第一帧舞台几何提交再打开。
        clip_rect.SetLeft2(0.0)?;
        clip_rect.SetTop2(0.0)?;
        clip_rect.SetRight2(0.0)?;
        clip_rect.SetBottom2(0.0)?;
        clip.SetClip(&clip_rect)?;
        scale.SetTransform2(&Matrix3x2::identity())?;
        scale.AddVisual(&target, true, None)?;
        clip.AddVisual(&scale, true, None)?;
        // 最底层先垫窗口黑底：内容与视频位是两张画布，改尺寸时错开一帧露出的
        // 缝必须露黑色而不是桌面（与 macOS 的窗口黑底同语义）。
        let backdrop = install_backdrop(device, container)?;
        // referenceVisual = 黑底时 `insertAbove = TRUE` ⇒ 紧贴在黑底之上；
        // 视频位仍必须在 GPUI 内容之下，否则进度条/弹幕会被画面盖住。
        container.AddVisual(&clip, true, &backdrop)?;
        layers()
            .lock()
            .expect("视频位表锁 poisoned")
            .insert(
                hwnd.0 as isize,
                SystemVideoLayer {
                    device: device.clone(),
                    clip,
                    clip_rect,
                    scale,
                    target,
                    applied: Mutex::new(None),
                },
            );
    }
    Ok(())
}

/// 窗口黑底：1×1 黑表面拉伸到覆盖整个窗口，垫在**所有**子视觉之下。
///
/// 为什么需要：视频位与 GPUI 内容是两张画布（各自 swapchain），快速改窗口
/// 尺寸时必然错开一帧；窗口是 `DXGI_ALPHA_MODE_PREMULTIPLIED` 的透明合成窗，
/// 任何没被内容或视频覆盖的像素会**直接漏出桌面**（实测拖快一点就在画面边缘
/// 看到一条浅色细线）。macOS 从未出现这个问题，因为它的窗口背景是黑的——
/// 这里补同一个语义：缝里露出来的一定是黑色。
///
/// 1×1 表面 + 8192 倍变换覆盖 ±4096 物理像素，任何窗口尺寸/DPI 都够用，
/// 且不需要跟随 resize 更新（零每帧开销，显存占用 4 字节）。
unsafe fn install_backdrop(
    device: &IDCompositionDevice,
    container: &IDCompositionVisual,
) -> Result<IDCompositionVisual> {
  unsafe {
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct3D11::{ID3D11Resource, ID3D11RenderTargetView,
        ID3D11Texture2D};
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM};

    let surface = device.CreateSurface(1, 1, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_ALPHA_MODE_IGNORE)?;
    let mut offset = windows::Win32::Foundation::POINT::default();
    let texture: ID3D11Texture2D = surface.BeginDraw(None, &mut offset)?;
    if let Ok(d3d) = texture.GetDevice()
        && let Ok(context) = d3d.GetImmediateContext()
        && let Ok(resource) = texture.cast::<ID3D11Resource>()
    {
        let mut view: Option<ID3D11RenderTargetView> = None;
        if d3d
            .CreateRenderTargetView(&resource, None, Some(&mut view))
            .is_ok()
            && let Some(view) = view
        {
            context.ClearRenderTargetView(&view, &[0.0, 0.0, 0.0, 1.0]);
        }
    }
    surface.EndDraw()?;

    let backdrop = device.CreateVisual()?;
    backdrop.SetContent(&surface)?;
    // 1×1 表面 → 覆盖 [-4096, 4096)² 物理像素。
    backdrop.SetTransform2(&Matrix3x2 {
        M11: 8192.0,
        M22: 8192.0,
        M31: -4096.0,
        M32: -4096.0,
        ..Matrix3x2::identity()
    })?;
    // insertAbove = TRUE + referenceVisual = NULL ⇒ 排在所有兄弟之后（最底层）。
    container.AddVisual(&backdrop, true, None)?;
    Ok(backdrop)
  }
}