//! 单条评论卡片：正文富文本、点赞、回复入口与删除入口。

use super::*;

use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::Icon;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, SharedString, div};
use hongguo_api::danmaku::CommentItem;

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
    ///
    /// 评论正文（提及前缀、正文提及段、表情码）交给
    /// [`crate::comment_text`]：整段合成一个文本节点渲染，才能在面板宽度内
    /// 换行（拆成多个 flex item 会按 max-content 撑开、飞出面板）。
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
                                // 昵称：列容器 + 截断（行容器里文本按内容宽度
                                // 排版，长昵称会顶到右侧的 ♡ 按钮上）。
                                v_flex()
                                    .text_xs()
                                    .text_color(gpui_kit::rgba(0xffffff99))
                                    .truncate()
                                    .child(SharedString::from(if c.user_name.is_empty() {
                                        crate::i18n::t("player.comments.anon")
                                    } else {
                                        c.user_name.clone()
                                    })),
                            )
                            .child(
                                // 正文用列容器：列会 stretch 子件，文本节点因此
                                // 拿到确定宽度而折行（行容器里按 max-content 排版）。
                                v_flex()
                                    .text_sm()
                                    .text_color(gpui_kit::rgba(0xffffffe6))
                                    .child(crate::comment_text::render(
                                        &crate::comment_text::comment_body(
                                            &c.text,
                                            self.ai_bot_name().as_deref(),
                                        ),
                                    )),
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
                                        //
                                        // base::Button 强制注入 flex/居中/紧行高，
                                        // 这里逐条改回原来的 block + 默认行高，
                                        // 否则文字行盒变矮、位置偏移。
                                        BaseButton::new(ElementId::from(format!(
                                            "comment-reply-{cid}"
                                        )))
                                        .accessibility_label(crate::i18n::t(
                                            "player.comments.reply",
                                        ))
                                        .justify_start()
                                        .line_height(gpui_kit::phi())
                                        .text_size(gpui_kit::px(11.0))
                                        .cursor_pointer()
                                        .text_color(gpui_kit::rgba(0xffffff73))
                                        .hover(|s| s.text_color(gpui_kit::rgba(0xd4d4d8ff)))
                                        .focus_visible(|s| {
                                            s.shadow(crate::home::skin_focus::skin_focus_ring())
                                        })
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
                                        BaseButton::new(ElementId::from(format!(
                                            "comment-del-{cid}"
                                        )))
                                        .accessibility_label(crate::i18n::t("common.delete"))
                                        .justify_start()
                                        .line_height(gpui_kit::phi())
                                        .text_size(gpui_kit::px(11.0))
                                        .cursor_pointer()
                                        .text_color(gpui_kit::rgba(0xffffff73))
                                        .hover(|s| s.text_color(gpui_kit::rgb(0xf87171)))
                                        .focus_visible(|s| {
                                            s.shadow(crate::home::skin_focus::skin_focus_ring())
                                        })
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
                                            BaseButton::new(ElementId::from(format!(
                                                "reply-toggle-{cid}"
                                            )))
                                            .accessibility_label(if replies_open {
                                                crate::i18n::t("player.comments.collapseReplies")
                                            } else {
                                                crate::i18n::tf(
                                                    "player.comments.expandReplies",
                                                    &[("n", &reply_count.to_string())],
                                                )
                                            })
                                            .aria_expanded(replies_open)
                                            .justify_start()
                                            .line_height(gpui_kit::phi())
                                            .text_size(gpui_kit::px(11.0))
                                            .cursor_pointer()
                                            .text_color(gpui_kit::rgba(0x60a5faff))
                                            .focus_visible(|s| {
                                                s.shadow(crate::home::skin_focus::skin_focus_ring())
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.toggle_replies(cid.clone(), reply_count, cx);
                                            }))
                                            .child(
                                                if replies_open {
                                                    crate::i18n::t(
                                                        "player.comments.collapseReplies",
                                                    )
                                                } else {
                                                    crate::i18n::tf(
                                                        "player.comments.expandReplies",
                                                        &[("n", &reply_count.to_string())],
                                                    )
                                                },
                                            )
                                        }
                                    })),
                            ),
                    )
                    .child(
                        // ♡ 点赞（乐观 ±1）
                        //
                        // 竖排（图标在上、数字在下）：库注入的是 flex row，
                        // 这里补 flex_col + justify_start 改回竖排；数字是
                        // 子件里的文字，行高也要改回默认，否则数字位置偏。
                        BaseButton::new(ElementId::from(format!("comment-digg-{cid}")))
                            .accessibility_label(if digged {
                                crate::i18n::t("player.interact.liked")
                            } else {
                                crate::i18n::t("player.interact.like")
                            })
                            .aria_toggled(if digged {
                                gpui_kit::Toggled::True
                            } else {
                                gpui_kit::Toggled::False
                            })
                            .flex_col()
                            .justify_start()
                            .items_center()
                            .gap_0p5()
                            .line_height(gpui_kit::phi())
                            .cursor_pointer()
                            .text_color(if digged {
                                gpui_kit::rgb(0xef4444)
                            } else {
                                gpui_kit::rgba(0xffffff99)
                            })
                            .focus_visible(|s| s.shadow(crate::home::skin_focus::skin_focus_ring()))
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
