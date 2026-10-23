//! 单条评论卡片：正文富文本、点赞、回复入口与删除入口。

use super::*;

use gpui_kit::component::Icon;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, SharedString, div};
use hongguo_api::danmaku::CommentItem;

use crate::emoji_scan::{MentionPiece, split_mention_segments};

impl HomeView {
    /// AI bot 的用户名（官方 mention 列表里的「小果」）。
    ///
    /// 提及候选列表加载前是 `None`——此时正文里的 `@名字` 拿不到权威名字，
    /// 认不出哪个是 AI bot（Tauri 侧同样用列表里的 userName 判定）。
    pub(super) fn ai_bot_name(&self) -> Option<String> {
        self.mention_users
            .iter()
            .find(|u| u.is_ai_bot)
            .map(|u| u.user_name.clone())
    }

    /// 评论列表一行：头像占位（昵称首字圆形）/昵称/文本/相对时间/♡。
    /// 评论正文：`@名字\u{FFFC}` 提及前缀染蓝（AI bot 名字补琥珀「Ai」
    /// 后缀——Tauri 的渐变文字在 GPUI 用单色近似），其余按 `[名字]`
    /// 表情分段内联（与弹幕共用一套 parse）。面板属 hgplayer 皮肤域，
    /// 固定色值属硬红线 11 的播放器豁免。
    fn render_comment_text(&self, text: &str) -> gpui_kit::AnyElement {
        let ai_name = self
            .ai_bot_name()
            .unwrap_or_else(|| crate::i18n::t("player.comments.aiName"));
        let (prefix, rest) = match parse_mention_prefix(text) {
            Some((name, rest)) => (Some(name), rest),
            None => (None, text),
        };
        let mut row = div().flex().flex_wrap().items_center();
        if let Some(name) = prefix {
            row = row.child(
                h_flex()
                    .items_center()
                    .mr_1()
                    .text_color(gpui_kit::rgb(0x38bdf8))
                    .child(SharedString::from(format!("@{name}")))
                    .children((name == ai_name).then(|| {
                        div()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(gpui_kit::rgb(0xfbbf24))
                            .child("Ai")
                    })),
            );
        }
        for el in self.rich_children(rest) {
            row = row.child(el);
        }
        row.into_any_element()
    }

    /// 富文本子元素（Tauri `EmojiText` 同口径）：认识的 `[名字]` 换表情图、
    /// 文本中段的 `@名字\u{FFFC}` 染蓝、其余原样。评论正文与回复行共用
    /// ——回复行（含小果AI 的回复）此前是纯文本，表情码全裸奔。
    pub(in crate::home) fn rich_children(&self, text: &str) -> Vec<gpui_kit::AnyElement> {
        let mut out = Vec::new();
        for seg in crate::danmaku_emoji::parse_emoji_segments(text) {
            match seg {
                crate::danmaku_emoji::EmojiSegment::Text(t) => {
                    for piece in split_mention_segments(&t) {
                        out.push(match piece {
                            MentionPiece::Text(s) => {
                                div().child(SharedString::from(s)).into_any_element()
                            }
                            MentionPiece::Mention(name) => div()
                                .mr_1()
                                .text_color(gpui_kit::rgb(0x38bdf8))
                                .child(SharedString::from(format!("@{name}")))
                                .into_any_element(),
                        });
                    }
                }
                crate::danmaku_emoji::EmojiSegment::Emoji(img) => {
                    out.push(
                        gpui_kit::img(gpui_kit::ImageSource::Render(img))
                            .size(gpui_kit::px(18.0))
                            .object_fit(gpui_kit::ObjectFit::Contain)
                            .into_any_element(),
                    );
                }
            }
        }
        out
    }

    pub(super) fn render_comment_row(
        &self,
        c: &CommentItem,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let cid = c.comment_id.clone();
        let is_mine = !self.my_user_id().is_empty() && c.user_id == self.my_user_id();
        let digged = self
            .comment_digg_override
            .get(&cid)
            .copied()
            .unwrap_or(c.user_digg);
        let digg_count = c.digg_count;
        let replies_open = self.replies_open.contains(&cid);
        let reply_count = c.reply_count;
        let my_reply_target = self.reply_to.as_ref().is_some_and(|(rc, _, _)| rc == &cid);
        let _ = my_reply_target;

        v_flex()
            .gap_1p5()
            .child(
                h_flex()
                    .items_start()
                    .gap_2()
                    .child(self.render_avatar(&c.avatar, &c.user_name, 32.0))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(gpui_kit::rgba(0xffffff99))
                                    .child(SharedString::from(if c.user_name.is_empty() {
                                        crate::i18n::t("player.comments.anon")
                                    } else {
                                        c.user_name.clone()
                                    })),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(gpui_kit::rgba(0xffffffe6))
                                    .child(self.render_comment_text(&c.text)),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_size(gpui_kit::px(11.0))
                                            .text_color(gpui_kit::rgba(0xffffff73))
                                            .child(crate::home::fmt::relative_time(c.create_time)),
                                    )
                                    .child(
                                        // 回复本条评论（打开底部回复输入框；
                                        // Tauri 评论行「回复」同款）
                                        div()
                                            .id(ElementId::from(format!("comment-reply-{cid}")))
                                            .text_size(gpui_kit::px(11.0))
                                            .cursor_pointer()
                                            .text_color(gpui_kit::rgba(0xffffff73))
                                            .hover(|s| s.text_color(gpui_kit::rgba(0xd4d4d8ff)))
                                            .on_click({
                                                let cid = cid.clone();
                                                let name = c.user_name.clone();
                                                cx.listener(move |this, _, _, cx| {
                                                    // 只展开底部回复输入框（Tauri
                                                    // setReplyTarget 同款，不拉回复区）
                                                    this.reply_to =
                                                        Some((cid.clone(), None, name.clone()));
                                                    cx.notify();
                                                })
                                            })
                                            .child(crate::i18n::t("player.comments.reply")),
                                    )
                                    .children(is_mine.then(|| {
                                        div()
                                            .id(ElementId::from(format!("comment-del-{cid}")))
                                            .text_size(gpui_kit::px(11.0))
                                            .cursor_pointer()
                                            .text_color(gpui_kit::rgba(0xffffff73))
                                            .hover(|s| s.text_color(gpui_kit::rgb(0xf87171)))
                                            .on_click({
                                                let cid = cid.clone();
                                                cx.listener(move |this, _, _, cx| {
                                                    this.request_delete(cid.clone(), None, cx);
                                                })
                                            })
                                            .child(crate::i18n::t("common.delete"))
                                    }))
                                    .children((reply_count > 0).then({
                                        let cid = cid.clone();
                                        move || {
                                            div()
                                                .id(ElementId::from(format!("reply-toggle-{cid}")))
                                                .text_size(gpui_kit::px(11.0))
                                                .cursor_pointer()
                                                .text_color(gpui_kit::rgba(0x60a5faff))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.toggle_replies(
                                                        cid.clone(),
                                                        reply_count,
                                                        cx,
                                                    );
                                                }))
                                                .child(if replies_open {
                                                    crate::i18n::t(
                                                        "player.comments.collapseReplies",
                                                    )
                                                } else {
                                                    crate::i18n::tf(
                                                        "player.comments.expandReplies",
                                                        &[("n", &reply_count.to_string())],
                                                    )
                                                })
                                        }
                                    })),
                            ),
                    )
                    .child(
                        // ♡ 点赞（乐观 ±1）
                        div()
                            .id(ElementId::from(format!("comment-digg-{cid}")))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_0p5()
                            .cursor_pointer()
                            .text_color(if digged {
                                gpui_kit::rgb(0xef4444)
                            } else {
                                gpui_kit::rgba(0xffffff99)
                            })
                            .on_click({
                                let cid = cid.clone();
                                cx.listener(move |this, _, _, cx| {
                                    this.toggle_comment_digg(cid.clone(), cx);
                                })
                            })
                            .child(Icon::new(gpui_kit::assets::IconName::Heart).size_4())
                            .children((digg_count > 0).then(|| {
                                div()
                                    .text_size(gpui_kit::px(10.0))
                                    .child(format!("{digg_count}"))
                            })),
                    ),
            )
            .children(self.render_comment_replies(cid, cx))
            .into_any_element()
    }
}

/// 提取 `@名字\u{FFFC}` 提及前缀（Tauri CommentText 的
/// `/^@([^\s\u{FFFC}]+)\u{FFFC}\s*/` 同口径），返回（名字, 余下正文）。
fn parse_mention_prefix(text: &str) -> Option<(String, &str)> {
    let rest = text.strip_prefix('@')?;
    let end = rest.find('\u{FFFC}')?;
    let name = &rest[..end];
    if name.is_empty() || name.chars().any(|c| c.is_whitespace() || c == '@') {
        return None;
    }
    let after = &rest[end + '\u{FFFC}'.len_utf8()..];
    Some((name.to_string(), after.trim_start()))
}
