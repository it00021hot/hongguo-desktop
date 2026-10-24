//! 剧集卡片网格的几何断言：找剧页与 Tauri `SeriesCardGrid` 对齐的守卫。
//!
//! 断言的是**规则**（不是某一屏的像素）：
//! 1. 响应式列数 = Tauri 断点（sm 640 / md 768 / lg 1024 / 2xl 1536）；
//! 2. 行内每张卡等宽 = （容器宽 − 间隙）/ 列数（与 CSS grid 的 `1fr` 列同解）；
//! 3. 封面盒高 = 宽 ÷ 3:4（Tauri `aspect-[3/4]`）；
//! 4. 末行不满一排时真实卡保持列宽、右侧留空（不把残卡拉成巨卡）；
//! 5. 同行卡片等高（Tauri grid 默认 `align-items: stretch`）。
//!
//! 需要 gpui 的 test-support（`screenshot` feature 已连带开启），运行方式：
//! `cargo test -p hongguo-gpui --features screenshot --test browse_grid_geometry`

#[path = "../src/pages/grid.rs"]
mod grid;

use gpui_kit::base::Button as BaseButton;
use gpui_kit::prelude::*;
use gpui_kit::{Bounds, Context, IntoElement, Pixels, Render, Window, div, px, rgb};

/// 容器宽（模拟内容区宽）。
const WIDTH: f32 = 500.0;
/// 列数。
const COLS: usize = 4;
/// 列间距（Tauri `gap-4`）。
const GAP: f32 = grid::GAP;
/// 卡片边框（Tauri 卡片 1px 边框）。
const BORDER: f32 = 1.0;

const CARD_SELECTORS: [&str; COLS] = ["card-0", "card-1", "card-2", "card-3"];
const COVER_SELECTORS: [&str; COLS] = ["cover-0", "cover-1", "cover-2", "cover-3"];

/// 列宽 = （容器宽 − 间隙）/ 列数。
fn column_width() -> f32 {
    (WIDTH - (COLS as f32 - 1.0) * GAP) / COLS as f32
}

/// 网格宿主：N 列的卡片行。奇数位的卡多一行文字，制造行内高度差。
struct Host {
    cards: usize,
}

impl Render for Host {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let cards: Vec<BaseButton> = (0..self.cards)
            .map(|i| {
                BaseButton::new(CARD_SELECTORS[i])
                    .debug_selector(|| CARD_SELECTORS[i].to_string())
                    .flex_col()
                    .items_stretch()
                    .justify_start()
                    .border_1()
                    .border_color(rgb(0x333333))
                    .child(
                        grid::cover_box()
                            .debug_selector(|| COVER_SELECTORS[i].to_string())
                            .child(div().size_full()),
                    )
                    .children((i % 2 == 1).then(|| div().w_full().h(px(30.0))))
            })
            .collect();
        div().w(px(WIDTH)).child(grid::row(cards, COLS))
    }
}

fn bounds(vx: &mut gpui_kit::VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    vx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} 应渲染出可观测的元素"))
}

/// 列数断点 = Tauri `grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5
/// 2xl:grid-cols-8`。
#[test]
fn cols_follow_tauri_breakpoints() {
    assert_eq!(grid::cols(400.0), 2);
    assert_eq!(grid::cols(639.0), 2);
    assert_eq!(grid::cols(640.0), 3);
    assert_eq!(grid::cols(767.0), 3);
    assert_eq!(grid::cols(768.0), 4);
    assert_eq!(grid::cols(1023.0), 4);
    assert_eq!(grid::cols(1024.0), 5);
    assert_eq!(grid::cols(1535.0), 5);
    assert_eq!(grid::cols(1536.0), 8);
}

/// 满行：每张卡等宽 = （容器宽 − 间隙）/ 列数。
#[gpui_kit::test]
async fn full_row_gives_every_card_the_column_width(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|_, _cx| Host { cards: COLS });
    let mut vx = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    vx.update(|window, cx| {
        let _ = window.draw(cx);
    });

    let expected = column_width();
    for (i, selector) in CARD_SELECTORS.iter().enumerate() {
        let card = bounds(&mut vx, selector);
        assert!(
            (card.size.width.as_f32() - expected).abs() < 0.5,
            "第 {i} 张卡宽 {:?}，应为列宽 {expected}",
            card.size.width
        );
    }
}

/// 末行不满：真实卡保持列宽，右侧留空（不把残卡拉成巨卡）。
#[gpui_kit::test]
async fn partial_row_keeps_column_width_and_leaves_the_rest_empty(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|_, _cx| Host { cards: 2 });
    let mut vx = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    vx.update(|window, cx| {
        let _ = window.draw(cx);
    });

    let expected = column_width();
    for (i, selector) in CARD_SELECTORS.iter().enumerate().take(2) {
        let card = bounds(&mut vx, selector);
        assert!(
            (card.size.width.as_f32() - expected).abs() < 0.5,
            "末行第 {i} 张卡宽 {:?}，应为列宽 {expected}",
            card.size.width
        );
    }
    // 末行两张卡占满前两列，右侧留空（不是被拉成整行宽）
    let second = bounds(&mut vx, CARD_SELECTORS[1]);
    let right_edge = second.origin.x.as_f32() + second.size.width.as_f32();
    assert!(
        right_edge < WIDTH - column_width(),
        "末行右侧应留空（实际右缘 {right_edge}，容器 {WIDTH}）"
    );
}

/// 封面盒按 3:4（Tauri `aspect-[3/4]`）：高 = 宽 ÷ 3:4。
#[gpui_kit::test]
async fn cover_box_keeps_three_by_four_ratio(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|_, _cx| Host { cards: COLS });
    let mut vx = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    vx.update(|window, cx| {
        let _ = window.draw(cx);
    });

    let cover = bounds(&mut vx, COVER_SELECTORS[0]);
    let cover_w = cover.size.width.as_f32();
    let cover_h = cover.size.height.as_f32();
    let expected_w = column_width() - 2.0 * BORDER;
    let expected_h = cover_w / grid::COVER_ASPECT;
    // 封面宽 = 卡片内容宽（卡有 1px 边框）
    assert!(
        (cover_w - expected_w).abs() < 0.5,
        "封面宽 {:?}，应为卡内容宽 {expected_w}",
        cover.size.width
    );
    assert!(
        (cover_h - expected_h).abs() < 0.5,
        "封面高 {:?}，应为宽 ÷ {} = {expected_h}",
        cover.size.height,
        grid::COVER_ASPECT
    );
}

/// 同行卡片等高（Tauri grid 默认 `align-items: stretch`）。
#[gpui_kit::test]
async fn cards_in_one_row_share_the_same_height(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|_, _cx| Host { cards: COLS });
    let mut vx = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    vx.update(|window, cx| {
        let _ = window.draw(cx);
    });

    // card-1 比 card-0 多一行文字（30px），拉伸后两者应等高
    let short = bounds(&mut vx, CARD_SELECTORS[0]);
    let tall = bounds(&mut vx, CARD_SELECTORS[1]);
    assert!(
        (short.size.height.as_f32() - tall.size.height.as_f32()).abs() < 0.5,
        "同行卡片应等高：{:?} vs {:?}",
        short.size.height,
        tall.size.height
    );
}
