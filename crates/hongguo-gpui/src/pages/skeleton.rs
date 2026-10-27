//! 加载骨架（对齐 Tauri `components/ui/skeletons.tsx`）。
//!
//! 形态三种，都是 Tauri 侧现成件的对齐物：
//! - 封面网格（收藏 / 喜欢 / 创作者）：只出封面，`aspect-[3/4] rounded-xl`；
//! - 卡片网格（找剧 / 新剧推荐）：封面 + 标题行 + 副标题行
//!   （Tauri `SkeletonCardGrid`）；
//! - 横向行（历史 / 追更 / 榜单 / 预约 / 日历）：整行占位
//!   （Tauri `SkeletonRows`）。
//!
//! 组件本体用组件库的 `Skeleton`：它渲染 `bg(theme.skeleton)` + 2 秒
//! opacity 脉冲，与 Tauri 的 `bg-accent animate-pulse` 同构（`animate-pulse`
//! 就是 2s、opacity 1→0.5）。
//!
//! 网格骨架沿用真实卡片的几何（封面网格按调用页给的卡宽/封面高；卡片网格
//! 与真实网格同列数、同间距、同 3:4 封面），数据到位时不会跳版。

use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, px};

/// 封面网格骨架：只出封面（Tauri 收藏/喜欢/创作者的 `aspect-[3/4]` 网格）。
pub(crate) fn cover_grid(
    cx: &App,
    count: usize,
    card_w: f32,
    cover_h: f32,
) -> gpui_kit::AnyElement {
    let radius = cx.theme().radius_lg;
    h_flex()
        .w_full()
        .flex_wrap()
        .items_start()
        .gap_3()
        .children((0..count).map(|_| Skeleton::new().w(px(card_w)).h(px(cover_h)).rounded(radius)))
        .into_any_element()
}

/// 卡片网格骨架：封面 + 标题行 + 副标题行（Tauri `SkeletonCardGrid`）。
///
/// 网格与真实卡片网格同几何（同列数断点、同 gap-4、封面同 3:4），数据到位
/// 时不跳版。
pub(crate) fn card_grid(cx: &App, count: usize, cols: usize) -> gpui_kit::AnyElement {
    let radius = cx.theme().radius_lg;
    let cell = || {
        v_flex()
            .gap_2()
            // 封面：比例盒里放满格骨架（比例盒给 3:4，骨架铺满——骨架自身
            // 带固定行高，直接上比例会被行高顶掉）。
            .child(
                crate::pages::grid::cover_box().child(Skeleton::new().size_full().rounded(radius)),
            )
            // 标题行 h-4 w-3/4、副标题行 h-3 w-1/2（Tauri 同参）
            .child(Skeleton::new().w(gpui_kit::relative(0.75)).h(px(16.0)))
            .child(Skeleton::new().w(gpui_kit::relative(0.5)).h(px(12.0)))
    };
    v_flex()
        .w_full()
        .gap_4()
        .children((0..count.div_ceil(cols)).map(|row_ix| {
            let start = row_ix * cols;
            let end = (start + cols).min(count);
            crate::pages::grid::row((start..end).map(|_| cell()).collect(), cols)
        }))
        .into_any_element()
}

/// 横向行骨架（Tauri `SkeletonRows`）：整行占位。
///
/// `height` 是**行本体高**（不含行间距），与真实行同值；`gap` 是该页真实
/// 列表的行间距——两者都对上，骨架与真实内容才能逐行对齐，数据到位不跳版。
pub(crate) fn rows(cx: &App, count: usize, height: f32, gap: f32) -> gpui_kit::AnyElement {
    let radius = cx.theme().radius_lg;
    v_flex()
        .w_full()
        .gap(gpui_kit::px(gap))
        .children((0..count).map(|_| Skeleton::new().w_full().h(px(height)).rounded(radius)))
        .into_any_element()
}
