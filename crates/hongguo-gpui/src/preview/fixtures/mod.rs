//! 预览假数据（`screenshot` feature）：合成封面与「字段齐平的 API 模型快照」。
//!
//! 全部离线、确定性（无网络、无时钟、无随机）：封面是按 seed 生成的渐变
//! 位图，模型快照的手填值对齐真实接口的形态（热度文案、档位、收藏数等），
//! 保证预览图和线上数据渲染走的是同一段布局代码。首字头像占位、进度条
//! 00:00 这类「引擎/网络缺席」的形态是预览的已知边界，见 `preview/mod.rs` 头注释。

mod browse;
mod feed;
mod library;
mod player;
mod rank;

pub(crate) use browse::{browse_feed_items, browse_panel_rows, search_results, suggest_items};
pub(crate) use feed::{
    calendar_items, creator_profile, creator_works, follow_items, follow_notices, new_drama_items,
    reservation_items, stream_tags,
};
pub(crate) use library::{
    bookshelf_entries, download_tasks, liked_items, merge_candidates, merge_preflight, merge_tasks,
    storage_series, storage_usage, watch_history_items,
};
pub(crate) use player::{comments, definitions, related_seasons};
pub(crate) use rank::{rank_items, rank_tabs};

use std::sync::Arc;

use gpui_kit::RenderImage;
use hongguo_api::calendar::CalendarItem;
use hongguo_api::danmaku::{CommentItem, CreatorProfile, CreatorWork};
use hongguo_api::detail::RelatedItem;
use hongguo_api::discover::{FeedItem, SelectorItem, SelectorRow};
use hongguo_api::history::WatchHistoryItem;
use hongguo_api::interact::{BookshelfEntry, FollowUpdateItem, InteractionItem};
use hongguo_api::rank::RankItem;
use hongguo_api::search::{SearchResult, SuggestItem};
use hongguo_common::model::{
    DownloadTask, FollowNotice, MergeCandidate, MergeMode, MergePreflight, MergeStatus, MergeTask,
    NOTICE_KIND_FOLLOW, NOTICE_KIND_ONLINE, TaskStatus, VideoDefinition,
};
use hongguo_service::storage_service::usage::{StorageSeriesUsage, StorageUsage};

/// 剧目池：(series_id, 剧名, 热度文案)。在播剧取第一条，其余铺满三个
/// tab 源（沉浸流滚轮/切 tab 的数据面）。
pub(crate) const SERIES: [(&str, &str, &str); 6] = [
    ("7100001", "命运契约", "3182万热度"),
    ("7100002", "重生之千金归来", "2751万热度"),
    ("7100003", "闪婚老公是豪门", "2290万热度"),
    ("7100004", "我在长安当神探", "1876万热度"),
    ("7100005", "末日庇护所", "1544万热度"),
    ("7100006", "助理大人请自重", "1208万热度"),
];

/// 合成封面：seed 决定渐变双色，横向条纹位置随 seed 变化——肉眼可分辨
/// 「每行封面各不相同」即可，不追求内容语义。走 [`crate::image_upload::
/// render_image`]（crate 内唯一合法的 `RenderImage` 出口，BGRA 转换在那里）。
pub(crate) fn cover(seed: usize) -> Arc<RenderImage> {
    let (w, h) = (480u32, 270u32);
    // 双色从 seed 推导：低位置出基色，高位色相错开 97（素数，避免相邻
    // seed 撞相近色相）。
    let (r0, g0, b0) = pastel(seed);
    let (r1, g1, b1) = pastel(seed + 97);
    let mut img = image::RgbaImage::new(w, h);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let t = (x + y) as f32 / (w + h) as f32;
        // 中段叠一条浅色横带（封面构图占位），其余线性渐变
        let band: f32 = if (y as f32 / h as f32 - 0.62).abs() < 0.05 {
            0.25
        } else {
            0.
        };
        let mix = |a: u8, b: u8| (a as f32 * (1. - t) + b as f32 * t + band * 64.) as u8;
        *px = image::Rgba([mix(r0, r1), mix(g0, g1), mix(b0, b1), 255]);
    }
    Arc::new(crate::image_upload::render_image(img))
}

/// seed → 柔和 RGB 三元组（低饱和高明度，浅深主题都不刺眼；47° 素数
/// 步进让相邻 seed 色相拉开，肉眼可分辨每张封面不同）。
fn pastel(seed: usize) -> (u8, u8, u8) {
    let hue = ((seed * 47) % 360) as f32;
    let (r, g, b) = hsv(hue, 0.35, 0.85);
    (r, g, b)
}

/// HSV → RGB（无外部依赖的最小实现，只服务假封面）。
fn hsv(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1. - ((h / 60.).rem_euclid(2.)) - 1.).abs();
    let (r, g, b) = match h as u32 / 60 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    let m = v - c;
    (
        ((r + m) * 255.) as u8,
        ((g + m) * 255.) as u8,
        ((b + m) * 255.) as u8,
    )
}
