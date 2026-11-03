//! 剧集卡片网格的共用几何。列宽交给布局引擎不手算（「每行固定 N 个等宽槽」，
//! flex 分剩余宽度，与 CSS grid 的 `1fr` 列同解）；末行缺卡的槽补空槽（纯
//! flex 行会把剩下的卡拉成巨卡）；行内 `items_stretch` 等高。只依赖组件库，
//! 几何规则可被集成测试直接引入断言。

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

/// 一行结果：卡片按序入槽，缺卡的槽补空槽（末行右侧留空）。槽宽 =（容器宽
/// − 间隙）/ 列数，与 CSS grid 的 `1fr` 列同解；必须用非零 flex 基值——
/// `flex_1`（基值 0）会因卡有边框使末行宽出 1px、列对不齐。
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
