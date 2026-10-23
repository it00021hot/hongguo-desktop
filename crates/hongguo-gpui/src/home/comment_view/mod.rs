//! 评论/回复面板的渲染（列表、回复行、删除确认浮层）：数据操作在
//! super::comments，本文件只管画。

mod card;
mod composer;
#[cfg(feature = "screenshot")]
mod debug;
mod delete_dialog;
mod list;
mod mention;

use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::prelude::*;

use super::HomeView;

impl HomeView {
    /// 评论面板（Tauri comment-panel 对齐：右侧滑出，列表 + 发送框；
    /// 回复内联展开；点赞乐观 ±1；表情代码随表情基建对齐补）。
    pub(super) fn render_comment_panel(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .id("comment-panel")
            .absolute()
            .right_0()
            // Tauri comment-panel 的几何同款：bottom-14（56px，让开控制条）
            // + h-[68%]。此前铺到舞台底、输入行被控制条压住点不到。
            .bottom(gpui_kit::px(56.0))
            .h(gpui_kit::relative(0.68))
            .w(gpui_kit::px(380.0))
            // Tauri 同款：bg-neutral-950/95 + rounded-tl-xl + 左/上白 10% 描边
            .bg(gpui_kit::rgba(0x0a0a0af2))
            .rounded_tl(gpui_kit::px(12.0))
            .border_t_1()
            .border_l_1()
            .border_color(gpui_kit::rgba(0xffffff1a))
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            // 滚轮拦下（Tauri comment-panel data-wheel-block 同款）：面板内
            // 滚动是列表自己在滚，不冒泡成舞台切剧
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
            .child(self.render_comment_panel_header(cx))
            .child(self.render_comment_list(cx))
            .child(self.render_comment_composer(cx))
            .children(self.render_emoji_backdrop(cx))
            .children(self.render_emoji_picker(cx))
            .children(self.render_delete_dialog(cx))
            .into_any_element()
    }

    // 头部：评论 · N + 关闭（Tauri h-12 + border-b 白 10% 同款）
    fn render_comment_panel_header(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        h_flex()
            .items_center()
            .justify_between()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(gpui_kit::rgba(0xffffff1a))
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(gpui_kit::rgb(0xffffff))
                    .child(format!(
                        "{} · {}",
                        crate::i18n::t("player.comments.title"),
                        self.comments_total
                    )),
            )
            .child(
                Button::new("comment-close")
                    .icon(gpui_kit::assets::IconName::X)
                    .custom(super::controls::chrome_variant(cx))
                    .cursor_pointer()
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.comments_panel_open = false;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}

/// 输入以 `@词` 结尾 = 提及候选弹层开着（Tauri 同款触发：打 @ 即弹；
/// `\u{FFFC}` 是提及段结尾占位符，含它说明已插入、不再触发）。手写匹配
/// （不引 regex）：`(^|\s)@[^\s@\u{FFFC}]*$` 同口径。
fn mention_query_open(text: &str) -> bool {
    strip_trailing_mention_trigger(text) != text
}

/// 剥掉末尾未成段的 `@词` 触发词（与 [`mention_query_open`] 同一套判定
/// 的剥离面）：点选候选后插入提及段前先吞掉触发词，否则 `@词` 残留在
/// 提及段前（Tauri insertMention 光标处吞词同语义）。无触发词原样返回。
pub(super) fn strip_trailing_mention_trigger(text: &str) -> &str {
    let Some(at) = text.rfind('@') else {
        return text;
    };
    let after = &text[at + 1..];
    if after.contains('\u{FFFC}') {
        return text;
    }
    // @ 前必须是串首或空白
    if at > 0
        && !text[..at]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
    {
        return text;
    }
    // @ 之后到串尾不许再有空白 / @ / 占位符
    if after
        .chars()
        .any(|c| c.is_whitespace() || c == '@' || c == '\u{FFFC}')
    {
        return text;
    }
    &text[..at]
}
