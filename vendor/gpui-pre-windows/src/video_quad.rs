//! 系统播放器的视频 quad：MediaEngine frame-server 帧的落点。
//!
//! 渲染器在每帧 clear 之后、场景批次之前，把注册的视频纹理按目标矩形画一个
//! instance（复用 polychrome sprite 管线，只换 SRV，见
//! [`DirectXRenderer::draw_video_quad`]）。视频与 GPUI 内容因此共用**同一张
//! 渲染目标**：上一代 DComp 视觉位方案（独立合成视觉）的整类坑——跨画布半像
//! 素白线、两 swapchain 错帧、透明洞漏桌面、z 序反直觉——在单画布下结构性
//! 不存在。
//!
//! 线程纪律：注册/建纹理/换帧/取帧全在 GPUI 主线程（壳 render 与播放器泵；
//! vsync 线程只发 `RedrawWindow`，不碰这里的锁持有的 GPU 对象）。设备丢失后
//! 渲染器重建设备并 bump 代际（[`device_rebuilt`]），播放器据此重建纹理、
//! 把设备管理器 `ResetDevice` 到新设备。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gpui::{
    AtlasTile, AtlasTextureId, AtlasTextureKind, Bounds, ContentMask, Corners, DevicePixels, Point,
    PolychromeSprite, ScaledPixels, Size, TileId,
};
use windows::Win32::Foundation::{E_FAIL, HWND};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11ShaderResourceView, ID3D11Texture2D, D3D11_BIND_RENDER_TARGET,
    D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::core::Interface;

/// 视频帧在窗口里的落点（物理像素、窗口客户区左上原点）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// 一个窗口的视频帧状态：纹理 + 它的 SRV + 原生尺寸 + 落点。
struct Frame {
    texture: ID3D11Texture2D,
    view: ID3D11ShaderResourceView,
    size: (u32, u32),
    dest: VideoRect,
}

/// 一个窗口的视频注册表项：渲染器设备（播放器建纹理、挂设备管理器用）+
/// 当前帧。`generation` 与设备重建挂钩，进程级单调。
struct Registry {
    device: ID3D11Device,
    frame: Option<Frame>,
}

fn registry() -> &'static Mutex<(u64, HashMap<isize, Registry>)> {
    static REGISTRY: OnceLock<Mutex<(u64, HashMap<isize, Registry>)>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new((0, HashMap::new())))
}

fn hwnd_key(hwnd: HWND) -> isize {
    hwnd.0 as isize
}

/// 渲染器创建时登记：把设备交给视频层（frame-server 引擎直接在它上面解码）。
pub(crate) fn register(hwnd: HWND, device: &ID3D11Device) {
    registry()
        .lock()
        .expect("视频注册表锁 poisoned")
        .1
        .insert(hwnd_key(hwnd), Registry {
            device: device.clone(),
            frame: None,
        });
}

/// 渲染器丢弃时注销。
pub(crate) fn unregister(hwnd: HWND) {
    registry()
        .lock()
        .expect("视频注册表锁 poisoned")
        .1
        .remove(&hwnd_key(hwnd));
}

/// 渲染器重建设备后调用：作废全部旧帧（旧设备的纹理/SRV 不能再画），
/// 代际 +1。播放器看到代际变化即重建纹理并 `ResetDevice` 设备管理器。
pub(crate) fn device_rebuilt() {
    let mut registry = registry().lock().expect("视频注册表锁 poisoned");
    registry.0 += 1;
    for entry in registry.1.values_mut() {
        entry.frame = None;
    }
}

/// 渲染器每帧取用：当前帧的 SRV 与「画这个 quad 所需的全部参数」。
pub(crate) fn frame_sprite_for(
    hwnd: HWND,
) -> Option<(ID3D11ShaderResourceView, PolychromeSprite)> {
    video_quad(hwnd)?.frame_sprite()
}

/// 外部（播放器 crate）持有的视频落点句柄。
#[derive(Clone, Copy)]
pub struct VideoQuad {
    key: isize,
}

/// 取窗口的视频落点句柄。渲染器重建后句柄继续有效（按 HWND 查最新状态）。
pub fn video_quad(hwnd: HWND) -> Option<VideoQuad> {
    let key = hwnd_key(hwnd);
    registry()
        .lock()
        .expect("视频注册表锁 poisoned")
        .1
        .get(&key)?;
    Some(VideoQuad { key })
}

/// 当前设备代际（设备丢失重建后变化；没有登记窗口时恒 0）。
pub fn generation() -> u64 {
    registry().lock().expect("视频注册表锁 poisoned").0
}

impl VideoQuad {
    /// 当前设备代际（渲染器设备丢失重建后变化；纹理与设备管理器按它失效）。
    pub fn generation(&self) -> u64 {
        generation()
    }

    /// 渲染器的 D3D11 设备（frame-server 引擎经设备管理器用它解码）。
    pub fn device(&self) -> Option<ID3D11Device> {
        registry()
            .lock()
            .expect("视频注册表锁 poisoned")
            .1
            .get(&self.key)
            .map(|entry| entry.device.clone())
    }

    /// 一块 `BIND_RENDER_TARGET | BIND_SHADER_RESOURCE` 的 BGRA 纹理：
    /// `TransferVideoFrame` 要求落点是 render target，渲染器采样要求
    /// shader resource。
    pub fn create_frame_texture(&self, width: u32, height: u32) -> windows::core::Result<ID3D11Texture2D> {
        let device = self
            .device()
            .ok_or_else(|| windows::core::Error::from_hresult(E_FAIL))?;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        unsafe {
            let mut texture = None;
            device.CreateTexture2D(&desc, None, Some(&mut texture))?;
            texture.ok_or_else(|| windows::core::Error::from_hresult(E_FAIL))
        }
    }

    /// 提交一帧：纹理（原生尺寸）+ 落点。渲染器下一帧把它画出来。
    ///
    /// SRV 在这里建并按纹理身份复用：泵每帧都会带着同一块纹理来改落点，
    /// 不复用就是每帧一次 SRV 创建的 COM 往返。
    pub fn set_frame(&self, texture: ID3D11Texture2D, size: (u32, u32), dest: VideoRect) {
        let mut registry = registry().lock().expect("视频注册表锁 poisoned");
        let Some(entry) = registry.1.get_mut(&self.key) else {
            return;
        };
        let view = match entry.frame.as_ref() {
            Some(frame) if frame.texture.as_raw() == texture.as_raw() => {
                frame.view.clone()
            }
            _ => unsafe {
                let mut view = None;
                if entry
                    .device
                    .CreateShaderResourceView(&texture, None, Some(&mut view))
                    .is_err()
                {
                    return;
                }
                view.expect("CreateShaderResourceView 返回空")
            },
        };
        entry.frame = Some(Frame {
            texture,
            view,
            size,
            dest,
        });
    }

    /// 收起画面（下帧渲染器不再画视频）。
    pub fn clear_frame(&self) {
        if let Some(entry) = registry()
            .lock()
            .expect("视频注册表锁 poisoned")
            .1
            .get_mut(&self.key)
        {
            entry.frame = None;
        }
    }

    /// 渲染器每帧取用：当前帧的 SRV 与「画这个 quad 所需的全部参数」。
    pub(crate) fn frame_sprite(&self) -> Option<(ID3D11ShaderResourceView, PolychromeSprite)> {
        let registry = registry().lock().expect("视频注册表锁 poisoned");
        let entry = registry.1.get(&self.key)?;
        let frame = entry.frame.as_ref()?;
        let (width, height) = frame.size;
        // tile = 整张纹理：`to_tile_position` 用绑定纹理自身尺寸算 UV，
        // origin(0,0) + size(宽,高) 即 0..1 全幅采样。
        let sprite = PolychromeSprite {
            order: 0,
            pad: 0,
            grayscale: false.into(),
            opacity: 1.0,
            bounds: Bounds {
                origin: Point {
                    x: ScaledPixels(frame.dest.x),
                    y: ScaledPixels(frame.dest.y),
                },
                size: Size {
                    width: ScaledPixels(frame.dest.width),
                    height: ScaledPixels(frame.dest.height),
                },
            },
            // 视频画在场景之下，且落点就是视频矩形本身：mask 收成自身，
            // 形状由落点控制，溢出不存在。
            content_mask: ContentMask {
                bounds: Bounds {
                    origin: Point {
                        x: ScaledPixels(frame.dest.x),
                        y: ScaledPixels(frame.dest.y),
                    },
                    size: Size {
                        width: ScaledPixels(frame.dest.width),
                        height: ScaledPixels(frame.dest.height),
                    },
                },
            },
            corner_radii: Corners {
                top_left: ScaledPixels(0.0),
                top_right: ScaledPixels(0.0),
                bottom_right: ScaledPixels(0.0),
                bottom_left: ScaledPixels(0.0),
            },
            tile: AtlasTile {
                texture_id: AtlasTextureId {
                    index: 0,
                    kind: AtlasTextureKind::Polychrome,
                },
                tile_id: TileId(0),
                padding: 0,
                bounds: Bounds {
                    origin: Point {
                        x: DevicePixels(0),
                        y: DevicePixels(0),
                    },
                    size: Size {
                        width: DevicePixels(width as i32),
                        height: DevicePixels(height as i32),
                    },
                },
            },
        };
        Some((frame.view.clone(), sprite))
    }
}
