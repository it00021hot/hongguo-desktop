//! 剧集卡片网格的共用几何（Tauri `SeriesCardGrid` / `NewDramaCard` /
//! `SkeletonCardGrid` 三处同款网格）。
//!
//! **列宽交给布局引擎，不手算**：CSS grid 的列宽是「容器宽 − 间隙」除以列数，
//! 而容器宽由布局给出（内容区宽随侧栏折叠、窗口缩放变）。手算就得把侧栏宽、
//! 页面内边距、滚动条宽全硬编码进页面——侧栏一展开就算错，卡片跟着走形。
//! 这里改成「每行固定 N 个等宽槽」：槽用 flex 分掉剩余宽度，列宽与
//! CSS grid 的 `1fr` 列同解。
//!
//! **末行不满一排**：CSS grid 里末行的卡片仍占自己的列宽（右侧留空），
//! 纯 flex 行会把剩下的卡拉成巨卡——所以缺卡的槽补一个空槽。
//!
//! **同行等高**：grid 默认 `align-items: stretch`，行内卡片一样高（短卡的
//! 边框拉到行高），这里用 `items_stretch` 对齐。
//!
//! 本模块只依赖组件库（不引壳内其它模块）：几何规则要能被集成测试直接
//! 引入断言（`tests/browse_grid_geometry.rs`）。

use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, div};

/// 网格列间距（Tauri `gap-4`）。
pub(crate) const GAP: f32 = 16.0;

/// 封面宽高比（Tauri `aspect-[3/4]`：宽/高 = 3/4）。
pub(crate) const COVER_ASPECT: f32 = 3.0 / 4.0;

/// 响应式列数（Tauri `grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5
/// 2xl:grid-cols-8`；断点按视口宽：sm 640 / md 768 / lg 1024 / 2xl 1536）。
pub(crate) fn cols(viewport_w: f32) -> usize {
    if viewport_w >= 1536.0 {
        8
    } else if viewport_w >= 1024.0 {
        5
    } else if viewport_w >= 768.0 {
        4
    } else if viewport_w >= 640.0 {
        3
    } else {
        2
    }
}

/// 封面盒（Tauri：卡内的 `aspect-[3/4]` 封面容器）。
///
/// 高度由 `aspect_ratio` 定，不手算：手算用的是**卡宽**，而封面实际比卡
/// 内容窄 2px（卡有边框）——按卡宽算出来的封面就不是 3:4 了。
pub(crate) fn cover_box() -> gpui_kit::Div {
    div()
        .relative()
        .w_full()
        .aspect_ratio(COVER_ASPECT)
        .flex_shrink_0()
        .overflow_hidden()
}

/// 一行结果：卡片按序入槽，缺卡的槽补空槽（末行右侧留空）。
///
/// 卡片类型要能加 flex 样式（`Styled`）——等宽与同行等高由这里统一给；
/// 页面自己写死卡宽就会在侧栏展开时走形。
///
/// **槽宽怎么来的**：每槽的 flex 基值取「容器宽 / 列数」，再由 flex 收缩把
/// 间隙挤掉——最终每槽 = （容器宽 − 间隙）/ 列数，与 CSS grid 的 `1fr` 列
/// 同解。用 `flex_1`（基值 0）不行：卡有边框，基值与空槽不等，末行会宽出
/// 1px，列与列对不齐。
pub(crate) fn row<T>(cards: Vec<T>, cols: usize) -> AnyElement
where
    T: IntoElement + Styled,
{
    let basis = gpui_kit::relative(1.0 / cols as f32);
    let mut slots: Vec<AnyElement> = cards
        .into_iter()
        .map(|card| {
            card.flex_grow_0()
                .flex_shrink(1.0)
                .flex_basis(basis)
                .min_w_0()
                .into_any_element()
        })
        .collect();
    slots.reserve(cols);
    while slots.len() < cols {
        slots.push(
            div()
                .flex_grow_0()
                .flex_shrink(1.0)
                .flex_basis(basis)
                .min_w_0()
                .into_any_element(),
        );
    }
    h_flex()
        .w_full()
        .gap(gpui_kit::px(GAP))
        // 同行卡片等高（Tauri grid 默认 align-items: stretch）
        .items_stretch()
        .children(slots)
        .into_any_element()
}
