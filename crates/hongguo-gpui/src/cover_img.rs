//! 封面元素：远端 URL → 数据目录里的成品图 → gpui 的 `img()` 负责解码、
//! 内存缓存、GPU 纹理与「加载完成后重绘」。
//!
//! **分工**：壳这边只做业务那一步（下载 + HEIC 转码 + 缩放 + 落盘，见
//! [`crate::cover_asset`]），解码与缓存全交给组件库——同一张封面在首页、
//! 找剧、历史、收藏里共用一份解码结果。
//!
//! **渲染期是纯函数**（只查文件在不在，不碰 `cx`）：成品图由页面的数据
//! 到达处 [`prewarm`] 排好转码，转码完成后 gpui 刷新窗口，文件在了自然
//! 就画得出来。这样各页面的渲染代码不用为「要不要可变借用 cx」操心。

use std::path::PathBuf;

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ImageSource, ObjectFit, Resource, div, img};
use tokio::sync::oneshot;

use hongguo_protocol::cover::{EDGE_AVATAR, EDGE_CARD};

use crate::cover_asset::CoverAsset;

/// 舞台全屏占位用的长边上限（像素）：占位按窗口像素铺，1080p 屏上有一千
/// 多 px 宽。
const FULL_MAX_EDGE: u32 = 1024;

/// 封面字节 → 原图尺寸的 RenderImage（舞台全屏占位用）。
///
/// **为什么这一处自己解**：gpui 的资产加载接口要 `&mut Window`，而舞台渲染
/// 拿到的是 `&Window`；全屏占位也只有当前这一集（图），不值得为它改一整条
/// 渲染链的借用。
pub(crate) fn decode_full(bytes: &[u8]) -> Option<std::sync::Arc<gpui_kit::RenderImage>> {
    let decoded = match image::load_from_memory(bytes) {
        Ok(decoded) => decoded,
        Err(error) => {
            log::warn!("[Cover] 全屏占位解码失败: {error}");
            return None;
        }
    };
    let resized = if decoded.width().max(decoded.height()) > FULL_MAX_EDGE {
        decoded.resize(
            FULL_MAX_EDGE,
            FULL_MAX_EDGE,
            image::imageops::FilterType::Triangle,
        )
    } else {
        decoded
    };
    Some(std::sync::Arc::new(crate::image_upload::render_image(
        resized.into_rgba8(),
    )))
}

/// 取一张封面字节（同步管线投阻塞池；oneshot 回传给 GPUI 执行器 await）。
///
/// **给谁用**：账号头像（壳侧栏 / 设置页）不在封面链路里，且要跟账号状态
/// 同步，走「自己取字节 + 立刻出图」比等资产缓存快半拍。
pub(crate) fn fetch(remote: String) -> oneshot::Receiver<Result<Vec<u8>, String>> {
    crate::bridge::spawn(async move {
        tokio::task::spawn_blocking(move || {
            hongguo_protocol::cover::acquire_renderable(&remote).map(|(bytes, _)| bytes)
        })
        .await
        .map_err(|e| format!("封面下载任务失败: {e}"))?
    })
}

/// 封面字节 → RenderImage（账号头像用；列表卡与舞台占位走 gpui 的图片缓存）。
///
/// **必须按显示尺寸预缩**：gpui 的渲染器只建一级 mip（`mip_level_count: 1`，
/// 过滤走 Linear，见 gpui-pre-wgpu `wgpu_renderer.rs`），几百像素的原图直接
/// 缩到 20px 画出来只剩采样噪声——Tauri 侧走浏览器的高质量缩放，所以那边
/// 清晰、这边糊。缩放必须在进 GPU 之前做完。
///
/// `display_px` 是头像在界面上的显示边长（物理像素；gpui 的 rem 不随 DPI
/// 缩放，20px 就是 20 物理像素）。按**短边**缩到这个尺寸：头像源图多为
/// 非正方形，`object_fit: Cover` 会裁切，短边到位才保证裁完不糊。
pub(crate) fn decode(
    bytes: &[u8],
    display_px: u32,
) -> Option<std::sync::Arc<gpui_kit::RenderImage>> {
    let decoded = image::load_from_memory(bytes).ok()?;
    let (w, h) = (decoded.width(), decoded.height());
    let short = w.min(h);
    let resized = if short > display_px {
        let scale = display_px as f32 / short as f32;
        decoded.resize(
            ((w as f32 * scale).round() as u32).max(1),
            ((h as f32 * scale).round() as u32).max(1),
            // 大比例缩小用 Lanczos3：双线性在 20 倍缩小时只剩噪点。
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        decoded
    };
    Some(std::sync::Arc::new(crate::image_upload::render_image(
        resized.into_rgba8(),
    )))
}

/// 封面圆角档（与 Tauri 的 `rounded-*` 一一对应）由令牌层给，
/// [`crate::cover_img::card`] 的调用方只管选档。
pub(crate) use crate::tokens::CoverShape;

/// 列表卡封面（卡片档，长边 320）。成品图还没落盘时给占位。
///
/// 预热用 [`prewarm_covers`]（同一档）。
pub(crate) fn card(url: &str, shape: CoverShape) -> AnyElement {
    element(url, EDGE_CARD, shape)
}

/// 评论/回复头像（头像档，长边 64；圆形由本函数自己裁——gpui 的父容器不裁
/// 子件，圆角写在图上才生效）。
///
/// 预热用 [`prewarm_avatars`]（同一档）。
pub(crate) fn avatar(url: &str) -> AnyElement {
    element(url, EDGE_AVATAR, CoverShape::Circle)
}

/// 排封面转码（卡片档 320）：远端封面 → 数据目录里的成品图。
///
/// 交给 gpui 的资产机制（[`CoverAsset`]）去重：同一张图在途时不会起第二份
/// 加载，转码完成后 gpui 刷新窗口，渲染期的 [`card`] 自然就画得出来。
pub(crate) fn prewarm_covers(urls: &[String], cx: &mut App) {
    prewarm(urls, EDGE_CARD, cx);
}

/// 排头像转码（头像档 64）：远端头像 → 数据目录里的成品图。
///
/// **档位必须与渲染侧一致**（[`avatar`] 也用 [`EDGE_AVATAR`]）：成品图按
/// 「URL 哈希 + 档位」落盘（[`hongguo_protocol::cover::sized_cache_path`]），
/// 预热 320、渲染找 64 → 文件永远不存在 → **头像整块空白**（2026-10-25 详情页
/// 与创作者页头像不显示的根因：创作者头像混在封面批次里按 320 预热，
/// 创作者页干脆没排头像）。
///
/// 档位由这两个入口**在模块内固定**，调用方不传——档位写错这类问题就不会再
/// 从页面上冒出来。
pub(crate) fn prewarm_avatars(urls: &[String], cx: &mut App) {
    prewarm(urls, EDGE_AVATAR, cx);
}

/// 排转码（内部用：档位由 [`prewarm_covers`] / [`prewarm_avatars`] 固定）。
fn prewarm(urls: &[String], edge: u32, cx: &mut App) {
    for url in urls {
        if url.is_empty() {
            continue;
        }
        // 预览场景是合成图，不走网络也不落盘。
        #[cfg(feature = "screenshot")]
        if crate::preview::fixtures::preview_image(url).is_some() {
            continue;
        }
        let source = (url.clone(), edge);
        cx.fetch_asset::<CoverAsset>(&source);
    }
}

/// 封面元素：成品图在盘上就交给 gpui 的 `img`，否则先占位。
fn element(url: &str, edge: u32, shape: CoverShape) -> AnyElement {
    if url.is_empty() {
        return div().size_full().into_any_element();
    }
    // 预览场景（screenshot feature）：合成图旁路，不走网络也不落盘。
    #[cfg(feature = "screenshot")]
    if let Some(image) = crate::preview::fixtures::preview_image(url) {
        return shape
            .apply(
                img(ImageSource::Render(image))
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
            .into_any_element();
    }
    let path: PathBuf = hongguo_protocol::cover::sized_cache_path(url, edge);
    if !path.exists() {
        return div().size_full().into_any_element();
    }
    shape
        .apply(
            img(ImageSource::Resource(Resource::Path(path.into())))
                .size_full()
                .object_fit(ObjectFit::Cover),
        )
        .into_any_element()
}
