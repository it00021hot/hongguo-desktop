//! 评论富文本的换行与回复前缀断言。
//!
//! 1. 长正文在固定宽度列里**必须折行**：GPUI 的文本只有拿到确定宽度才折行
//!    （`TextLayout::layout` 的 `wrap_width` 取 `known_dimensions.width`），
//!    文本被拆成多个 flex item 时每个 item 按 max-content 测量，整段中文
//!    不折行、横向撑出面板右边界；
//! 2. 回复行在被回复人昵称为空时**不加**「回复 @某人：」前缀（官方手机 App 与
//!    Tauri 壳同款——回复评论本条时只显示正文）。
//!
//! 需要 gpui 的 test-support（`screenshot` feature 已连带开启），运行方式：
//! `cargo test -p hongguo-gpui --features screenshot --test comment_text`

#[path = "../src/comment_text.rs"]
mod comment_text;
// 整个模块引进来（comment_text 依赖它的解析），本测试只用其中一部分——
// 没用到的项在本测试 crate 里是死的。用 `expect` 而不是 `allow`：lint 不再
// 触发时编译失败，过期放行会被发现。
#[expect(
    dead_code,
    reason = "本测试只用模块的一部分，未用项在本 crate 内是死的"
)]
#[expect(unused_imports, reason = "模块整体引入带来的未用重导出")]
#[path = "../src/danmaku_emoji.rs"]
mod danmaku_emoji;
#[path = "../src/emoji_files.rs"]
mod emoji_files;
#[path = "../src/emoji_scan.rs"]
mod emoji_scan;
#[expect(
    dead_code,
    reason = "本测试只用 Locale 的少数字段，其余在本 crate 内是死的"
)]
#[path = "../src/i18n.rs"]
mod i18n;
#[path = "../src/image_upload.rs"]
mod image_upload;

use comment_text::Segment;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{IntoElement, Render, px};

/// 正文宽度（px）——与评论面板 380px 减去头像、留白后的可用宽度同量级。
const BODY_WIDTH: f32 = 220.0;

/// 一段正文渲染后的纯文本（表情图段不计）。
fn plain_text(segments: &[Segment]) -> String {
    segments
        .iter()
        .filter_map(|segment| match segment {
            Segment::Text(part) => Some(part.text.as_str()),
            Segment::Emoji(_) => None,
        })
        .collect()
}

struct Host {
    short_text: String,
    long_text: String,
}

impl Render for Host {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        _cx: &mut gpui_kit::Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .w(px(BODY_WIDTH))
            .p_4()
            .child(
                v_flex()
                    .id("short")
                    .debug_selector(|| "short".to_string())
                    .text_sm()
                    .child(comment_text::render(&comment_text::comment_body(
                        &self.short_text,
                        None,
                    ))),
            )
            .child(
                v_flex()
                    .id("long")
                    .debug_selector(|| "long".to_string())
                    .text_sm()
                    .child(comment_text::render(&comment_text::comment_body(
                        &self.long_text,
                        None,
                    ))),
            )
    }
}

/// 长正文必须折行（此前整段按 max-content 排版，横向飞出面板）。
#[gpui_kit::test]
async fn long_comment_wraps_inside_fixed_width(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|_, _cx| Host {
        short_text: "短评论".to_string(),
        long_text: "这是一条很长的评论".repeat(12),
    });
    let mut vx = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    vx.update(|window, cx| {
        let _ = window.draw(cx);
    });

    let short = vx
        .debug_bounds("short")
        .expect("短评论应渲染出可观测的文本节点");
    let long = vx
        .debug_bounds("long")
        .expect("长评论应渲染出可观测的文本节点");

    assert!(
        short.size.height > px(0.),
        "短评论应占一行高度，实际 {:?}",
        short.size.height
    );
    assert!(
        long.size.height > short.size.height * 1.5,
        "长评论必须折行（单行高度 {:?}，长文本高度 {:?}）",
        short.size.height,
        long.size.height
    );
    assert!(
        long.size.width <= px(BODY_WIDTH),
        "文本宽度不得超出容器（容器 {:?}，文本 {:?}）",
        px(BODY_WIDTH),
        long.size.width
    );
}

/// 被回复人昵称为空 → 只显示正文，不加任何前缀。
#[test]
fn reply_without_target_has_no_prefix() {
    i18n::set_locale(i18n::Locale::ZhCn);
    let segments = comment_text::reply_body(
        "回复正文",
        "",
        "player.comments.replyToPrefixName",
        Some(comment_text::mention_color()),
    );
    assert_eq!(plain_text(&segments), "回复正文");
}

/// 被回复人昵称非空 → 前缀「回复 @某人：」+ 正文（官方同款）。
#[test]
fn reply_with_target_has_prefix() {
    i18n::set_locale(i18n::Locale::ZhCn);
    let segments = comment_text::reply_body(
        "回复正文",
        "小果",
        "player.comments.replyToPrefixName",
        Some(comment_text::mention_color()),
    );
    let text = plain_text(&segments);
    assert!(
        text.starts_with("回复 @小果："),
        "回复前缀应带被回复人名字，实际 {text:?}"
    );
    assert!(text.ends_with("回复正文"), "前缀后应接正文，实际 {text:?}");
    assert!(
        !text.contains("原评论"),
        "不得出现自造的「原评论」回落，实际 {text:?}"
    );
}

/// 提及前缀与正文合成同一段文本（否则前缀会被 flex 拆到另一行）。
#[test]
fn comment_body_keeps_mention_prefix_inline() {
    i18n::set_locale(i18n::Locale::ZhCn);
    let segments = comment_text::comment_body("@小果\u{FFFC}这是一条回复", Some("小果"));
    assert_eq!(segments.len(), 1, "提及前缀与正文应合成一个文本段");
    let text = plain_text(&segments);
    assert_eq!(text, "@小果 Ai 这是一条回复");
}
