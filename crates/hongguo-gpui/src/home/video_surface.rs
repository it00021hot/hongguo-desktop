//! 视频上屏的像素载体（平台差异的唯一归属）。
//!
//! 两端拿到的解码帧形态不同，渲染路径也不同：
//!
//! - **macOS**：解码出 `CVPixelBuffer`（NV12，IOSurface 背板）→ gpui 的
//!   `surface()` 元素零拷贝采样（Metal 直接读 Y/CbCr 平面）。没有 CPU 转换、
//!   没有纹理上传。
//! - **Windows**：解码出 NV12 系统内存 → 后台线程转 RGBA（BT.709 limited，
//!   按舞台显示尺寸缩放）→ 普通 `img` 上传进 sprite atlas。gpui 的
//!   `surface()` 只有 Apple 目标，这条是等价路径。
//!
//! 业务层（`home/mod.rs`、`home/stage.rs`）只认 [`VideoSurface`]：喂帧、
//! 设目标尺寸、取渲染层、每帧回收纹理。它不写任何 `#[cfg(target_os)]`。
//!
//! 纹理回收（Windows 专有）：gpui 的 sprite atlas 按 `RenderImage::id` 常驻，
//! `paint_image` 只增不删；视频每帧一个新 id，不显式 `drop_image` 就是无上限
//! 的显存泄漏（实测 5 分钟 5.5GB）。回收只能在 render 里做（那里才有 Window）。

use std::sync::Arc;

use gpui_kit::{ObjectFit, RenderImage, Window};
use hongguo_media_platform_api::video::VideoFrame;

use crate::home::video_surface::imp::{ConversionHandle, Imp};

/// 视频上屏状态：当前帧的渲染产物 + 平台专有的转换/回收状态。
pub(crate) struct VideoSurface {
    imp: Imp,
}

impl VideoSurface {
    pub(crate) fn new() -> Self {
        Self { imp: Imp::new() }
    }

    /// 后台转换用的句柄：只在起事件循环时取一次，后续每帧直接读句柄里的
    /// 原子量。**不要每帧回主线程取**——那是跨线程往返，主线程忙时反而
    /// 把喂帧节流成掉帧源。macOS 的句柄是零成本占位（本平台不转换）。
    pub(crate) fn conversion_handle(&self) -> ConversionHandle {
        self.imp.handle()
    }

    /// 接住转换产物并换上新图（主线程实体更新里调用）。
    pub(crate) fn adopt(&mut self, image: Option<Arc<RenderImage>>) {
        self.imp.adopt(image);
    }

    /// 设置转换目标尺寸（舞台显示区，CSS px）。仅 Windows 有意义。
    pub(crate) fn set_target(&self, width: usize, height: usize) {
        self.imp.set_target(width, height);
    }

    /// 设置适配模式（小屏铺满裁切 / 其余 contain）。仅 Windows 有意义。
    pub(crate) fn set_cover(&self, cover: bool) {
        self.imp.set_cover(cover);
    }

    /// 渲染层（占满父容器的绝对定位叶子）。
    pub(crate) fn layer(&self, frame: &VideoFrame, fit: ObjectFit) -> gpui_kit::AnyElement {
        self.imp.layer(frame, fit)
    }

    /// 每帧回收已下台的纹理（只在 render 里调用——那里有 Window）。
    pub(crate) fn drop_retired(&mut self, window: &mut Window) {
        self.imp.drop_retired(window);
    }

    /// 退场（切剧/连播）：渲染产物进回收队列，下一帧起换新。
    pub(crate) fn clear(&mut self) {
        self.imp.clear();
    }

    /// 诊断状态串（周期状态日志用）。
    pub(crate) fn status(&self) -> String {
        self.imp.status()
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

    use gpui_kit::prelude::*;
    use hongguo_media_platform_api::video::{FrameBuffer, VideoFrame};

    /// 回收队列上限：render 排不上队时也不能无界涨（被挤掉的旧图其纹理
    /// 可能滞留图集，代价有界：≤16 × 输出尺寸）。
    const RETIRED_KEEP: usize = 16;

    pub(super) struct Imp {
        /// 当前帧的 RGBA 渲染产物。
        image: Option<Arc<RenderImage>>,
        /// 已下台、等 render 里 `drop_image` 回收的图像。
        retired: Vec<Arc<RenderImage>>,
        /// 转换目标尺寸（CSS px，打包成 u64 跨线程共享——转换在 gpui 后台
        /// 线程池跑，Rc/Cell 过不去）。
        target: Arc<AtomicU64>,
        /// 转换产物是否按铺满裁切（cover）适配。
        cover: Arc<AtomicBool>,
    }

    /// 后台转换句柄：目标尺寸 + 适配模式的原子共享（事件循环开始时取一次）。
    #[derive(Clone)]
    pub(crate) struct ConversionHandle {
        target: Arc<AtomicU64>,
        cover: Arc<AtomicBool>,
    }

    pub(super) fn convert_with(
        handle: &ConversionHandle,
        frame: &VideoFrame,
    ) -> Option<Arc<RenderImage>> {
        let (w, h) = unpack(handle.target.load(Ordering::Relaxed));
        render_image_of(frame, (w, h), handle.cover.load(Ordering::Relaxed))
    }

    impl Imp {
        pub(super) fn new() -> Self {
            Self {
                image: None,
                retired: Vec::new(),
                target: Arc::new(AtomicU64::new(pack_size(1280, 720))),
                cover: Arc::new(AtomicBool::new(false)),
            }
        }

        pub(super) fn handle(&self) -> ConversionHandle {
            ConversionHandle {
                target: Arc::clone(&self.target),
                cover: Arc::clone(&self.cover),
            }
        }

        /// 换上新图，旧图进回收队列。
        pub(super) fn adopt(&mut self, image: Option<Arc<RenderImage>>) {
            let Some(image) = image else {
                return;
            };
            while self.retired.len() >= RETIRED_KEEP {
                self.retired.remove(0);
            }
            if let Some(old) = self.image.replace(image) {
                self.retired.push(old);
            }
        }

        pub(super) fn set_target(&self, width: usize, height: usize) {
            // 至少 2px（与转换端的下限一致）
            self.target
                .store(pack_size(width.max(2), height.max(2)), Ordering::Relaxed);
        }

        pub(super) fn set_cover(&self, cover: bool) {
            self.cover.store(cover, Ordering::Relaxed);
        }

        pub(super) fn layer(&self, _frame: &VideoFrame, fit: ObjectFit) -> gpui_kit::AnyElement {
            match &self.image {
                Some(image) => img(ImageSource::Render(Arc::clone(image)))
                    .absolute()
                    .size_full()
                    .object_fit(fit)
                    .into_any_element(),
                None => div().absolute().size_full().into_any_element(),
            }
        }

        pub(super) fn drop_retired(&mut self, window: &mut Window) {
            for old in std::mem::take(&mut self.retired) {
                if let Err(e) = window.drop_image(old) {
                    log::warn!("[Home] 纹理回收失败: {e}");
                }
            }
        }

        pub(super) fn clear(&mut self) {
            if let Some(old) = self.image.take() {
                self.retired.push(old);
            }
        }

        pub(super) fn status(&self) -> String {
            format!(
                "image={}（待回收 {}）",
                self.image.is_some(),
                self.retired.len()
            )
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::Arc;

    use gpui_kit::{ObjectFit, RenderImage, Window};
    use gpui_kit::prelude::*;
    use hongguo_media_platform_api::video::{FrameBuffer, VideoFrame};

    pub(super) struct Imp;

    /// macOS 不做 CPU 侧转换：句柄是零成本占位。
    #[derive(Clone, Copy)]
    pub(crate) struct ConversionHandle;

    impl Imp {
        pub(super) fn new() -> Self {
            Self
        }

        pub(super) fn handle(&self) -> ConversionHandle {
            ConversionHandle
        }

        pub(super) fn adopt(&mut self, _image: Option<Arc<RenderImage>>) {}

        pub(super) fn set_target(&self, _width: usize, _height: usize) {}

        pub(super) fn set_cover(&self, _cover: bool) {}

        pub(super) fn layer(&self, frame: &VideoFrame, fit: ObjectFit) -> gpui_kit::AnyElement {
            // Apple 目标上 FrameBuffer 只有 CVPixelBuffer 一个变体（可驳尽）。
            // 解码侧是 core-video 的 TCFType 包装，gpui 0.3.9 起要 objc2 的
            // CFRetained：借裸指针 CFRetain 一次（等价原先 clone 的 +1），
            // 所有权交给 surface。
            use core_foundation::base::TCFType as _;
            let FrameBuffer::CVPixelBuffer(buffer) = &frame.buffer;
            let pixel_buffer = unsafe {
                objc2_core_foundation::CFRetained::retain(std::ptr::NonNull::new_unchecked(
                    buffer.as_concrete_TypeRef() as *mut objc2_core_video::CVPixelBuffer,
                ))
            };
            gpui_kit::surface(pixel_buffer)
                .absolute()
                .size_full()
                .object_fit(fit)
                .into_any_element()
        }

        pub(super) fn drop_retired(&mut self, _window: &mut Window) {}

        pub(super) fn clear(&mut self) {}

        pub(super) fn status(&self) -> String {
            "image=n/a".to_string()
        }
    }
}

#[cfg(target_os = "windows")]
fn pack_size(w: usize, h: usize) -> u64 {
    ((w as u64) << 32) | h as u64
}

#[cfg(target_os = "windows")]
fn unpack(packed: u64) -> (usize, usize) {
    ((packed >> 32) as usize, (packed & 0xffff_ffff) as usize)
}

/// 后台转换入口（事件循环在后台线程池里 await）：Windows 把 NV12 转成
/// RGBA 并按句柄里的目标尺寸缩放。
///
/// **只有 Windows 有这个函数**：macOS 是零拷贝 surface，事件循环里不该出现
/// 任何「spawn 到后台再 await」的按帧等待——后台线程被占住时会把整条事件
/// 循环卡死（实测：首帧过后画面再不更新，而引擎侧仍在 30fps 上屏）。
#[cfg(target_os = "windows")]
pub(crate) fn convert_frame(
    handle: &imp::ConversionHandle,
    frame: &VideoFrame,
) -> Option<Arc<RenderImage>> {
    imp::convert_with(handle, frame)
}

/// Windows 渲染转换：NV12（BT.709 limited）→ RGBA 图像，按舞台显示尺寸缩放
/// （见 `target` 头注释）。转换跑在本事件任务线程。
#[cfg(target_os = "windows")]
fn render_image_of(
    frame: &VideoFrame,
    target: (usize, usize),
    cover: bool,
) -> Option<Arc<RenderImage>> {
    static CONVERTED: AtomicU32 = AtomicU32::new(0);
    let t0 = Instant::now();
    let FrameBuffer::Nv12(nv12) = &frame.buffer;
    let (tw, th) = if cover {
        cover_fit(nv12.width, nv12.height, target.0, target.1)
    } else {
        contain_fit(nv12.width, nv12.height, target.0, target.1)
    };
    let rgba = hongguo_media::nv12_rgba::nv12_to_rgba_bt709_scaled(
        &nv12.data,
        nv12.stride,
        nv12.width,
        nv12.height,
        tw,
        th,
    );
    let buf = image::RgbaImage::from_raw(tw as u32, th as u32, rgba)?;
    // 取证开关：HONGGUO_DUMP_NV12=<前缀> 时第 0/200/600 帧落盘 NV12 +
    // 我们的转换 PNG（与 ffmpeg 参照对拍用）。
    static DUMPED: AtomicU32 = AtomicU32::new(0);
    let n_dump = DUMPED.fetch_add(1, Ordering::Relaxed);
    if n_dump.is_multiple_of(500)
        && let Ok(prefix) = std::env::var("HONGGUO_DUMP_NV12")
    {
        let tag = if n_dump == 0 {
            "f0".to_string()
        } else {
            format!("f{n_dump}")
        };
        let _ = std::fs::write(
            std::path::PathBuf::from(&prefix).with_extension(format!("{tag}.nv12")),
            nv12.data.as_slice(),
        );
        let _ = buf
            .save(std::path::PathBuf::from(&prefix).with_extension(format!("{tag}.ours.png")));
        log::info!(
            "[Home] NV12 取证 #{n_dump} 已落盘：{prefix}.{tag}.nv12/.ours.png（{}×{} stride {}，转换目标 {tw}×{th}）",
            nv12.width,
            nv12.height,
            nv12.stride
        );
    }
    let image = Arc::new(crate::image_upload::render_image(buf));
    let n = CONVERTED.fetch_add(1, Ordering::Relaxed);
    if n < 3 || n.is_multiple_of(200) {
        log::info!(
            "[Home] NV12→RGBA 转换 #{n} 耗时 {:.1}ms（{tw}×{th}，源 {}×{}）",
            t0.elapsed().as_secs_f64() * 1000.0,
            nv12.width,
            nv12.height
        );
    }
    Some(image)
}

/// contain 适配：源 (w,h) 缩进目标盒（不放大、保宽高比、至少 2px）。
#[cfg(target_os = "windows")]
fn contain_fit(w: usize, h: usize, box_w: usize, box_h: usize) -> (usize, usize) {
    let scale = (box_w as f64 / w as f64)
        .min(box_h as f64 / h as f64)
        .min(1.0);
    (
        (w as f64 * scale).round().max(2.0) as usize,
        (h as f64 * scale).round().max(2.0) as usize,
    )
}

/// cover 适配（小屏形态）：源 (w,h) 铺满目标盒（保宽高比、允许放大；
/// 溢出部分由显示端的 `ObjectFit::Cover` 裁掉）——Tauri 小屏
/// `object-cover` 同款，竖屏剧在小窗里不留黑边。
#[cfg(target_os = "windows")]
fn cover_fit(w: usize, h: usize, box_w: usize, box_h: usize) -> (usize, usize) {
    let scale = (box_w as f64 / w as f64).max(box_h as f64 / h as f64);
    (
        (w as f64 * scale).round().max(2.0) as usize,
        (h as f64 * scale).round().max(2.0) as usize,
    )
}
