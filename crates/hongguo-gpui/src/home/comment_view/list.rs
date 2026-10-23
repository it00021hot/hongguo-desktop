//! 评论列表：加载、空态、分页，以及评论下子回复的展开渲染。

use super::*;

use gpui_kit::component::Icon;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, SharedString, div};

impl HomeView {
    /// 评论列表滚动区：加载态、空态、条目与「加载更多」。
    pub(super) fn render_comment_list(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let loading = self.comments_loading && self.comments.is_empty();
        div()
            .id("comments-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_4()
            .child(if loading {
                div()
                    .py_8()
                    .text_center()
                    .text_sm()
                    .text_color(gpui_kit::rgba(0xffffff99))
                    .child(crate::i18n::t("common.loading"))
                    .into_any_element()
            } else if self.comments.is_empty() {
                div()
                    .py_8()
                    .text_center()
                    .text_sm()
                    .text_color(gpui_kit::rgba(0xffffff99))
                    .child(crate::i18n::t("player.comments.empty"))
                    .into_any_element()
            } else {
                v_flex()
                    .gap_4()
                    .children(self.comments.iter().map(|c| self.render_comment_row(c, cx)))
                    .children(self.comments_has_more.then(|| {
                        div()
                            .id("comments-more")
                            .py_2()
                            .text_center()
                            .text_xs()
                            .cursor_pointer()
                            .text_color(gpui_kit::rgba(0x60a5faff))
                            .hover(|s| s.text_color(gpui_kit::rgb(0x93c5fd)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.load_comments_next(cx);
                            }))
                            .child(if self.comments_loading {
                                crate::i18n::t("common.loading")
                            } else {
                                crate::i18n::tf(
                                    "player.comments.loadMore",
                                    &[(
                                        "n",
                                        &self
                                            .comments_total
                                            .saturating_sub(self.comments.len() as i64)
                                            .to_string(),
                                    )],
                                )
                            })
                    }))
                    .into_any_element()
            })
            .into_any_element()
    }

    /// 评论下子回复区：展开态、「思考中…」占位、加载占位与回复行。
    pub(super) fn render_comment_replies(
        &self,
        cid: String,
        cx: &Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        let replies = self.replies.get(&cid).cloned();
        let replies_open = self.replies_open.contains(&cid);
        let loading = self.replies_loading.contains(&cid);
        replies_open.then(|| {
            let items = replies.map(|v| (*v).clone()).unwrap_or_default();
            v_flex()
                .pl_10()
                .gap_2()
                // 「思考中…」占位（Tauri pendingAi 同款）：@小果 提及评论
                // 发出后回复区强制展开，轮询到 AI 回复即撤
                .children(self.ai_pending.iter().any(|(id, _)| id == &cid).then(|| {
                    div()
                        .text_xs()
                        .text_color(gpui_kit::rgba(0xffffff73))
                        .child(crate::i18n::t("player.comments.aiThinking"))
                }))
                .children((loading && items.is_empty()).then(|| {
                    div()
                        .text_xs()
                        .text_color(gpui_kit::rgba(0xffffff73))
                        .child(crate::i18n::t("common.loading"))
                }))
                .children(items.iter().map(|r| {
                    let digged = self
                        .comment_digg_override
                        .get(&r.reply_id)
                        .copied()
                        .unwrap_or(r.user_digg);
                    let digg_count = r.digg_count;
                    let rid = r.reply_id.clone();
                    let rcid = cid.clone();
                    let rname = r.user_name.clone();
                    let reply_mine =
                        !self.my_user_id().is_empty() && r.user_id == self.my_user_id();
                    // AI bot 回复：作者名带「Ai」后缀 + 「内容由AI生成」标记
                    // （Tauri ReplyThread 的 isAi 同款——回复作者对上
                    // mention 列表里的 AI bot 才算）。
                    let is_ai = self
                        .ai_bot_name()
                        .is_some_and(|ai_name| ai_name == r.user_name);
                    h_flex()
                        .items_start()
                        .gap_2()
                        .child(self.render_avatar(&r.avatar, &r.user_name, 24.0))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_0p5()
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(gpui_kit::rgba(0xffffff99))
                                                .child(SharedString::from(
                                                    if r.user_name.is_empty() {
                                                        crate::i18n::t("player.comments.anon")
                                                    } else {
                                                        r.user_name.clone()
                                                    },
                                                )),
                                        )
                                        .children(is_ai.then(|| {
                                            div()
                                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                                .text_color(gpui_kit::rgb(0xfbbf24))
                                                .child("Ai")
                                        })),
                                )
                                .child({
                                    let original =
                                        crate::i18n::t("player.comments.originalComment");
                                    let name = if r.reply_to_name.is_empty() {
                                        original.as_str()
                                    } else {
                                        r.reply_to_name.as_str()
                                    };
                                    let summary = crate::i18n::tf(
                                        "player.comments.replyToSummary",
                                        &[("name", name), ("text", r.text.as_str())],
                                    );
                                    div()
                                        .text_xs()
                                        .text_color(gpui_kit::rgba(0xffffffd9))
                                        .child({
                                            let mut row = div().flex().flex_wrap().items_center();
                                            for el in self.rich_children(&summary) {
                                                row = row.child(el);
                                            }
                                            row
                                        })
                                })
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_3()
                                        .child(
                                            div()
                                                .text_size(gpui_kit::px(10.0))
                                                .text_color(gpui_kit::rgba(0xffffff73))
                                                .child(crate::home::fmt::relative_time(
                                                    r.create_time,
                                                )),
                                        )
                                        // AI 生成标记（Tauri ReplyRow 的 isAi
                                        // 同款：时间右侧跟一句「内容由AI生成」）
                                        .children(is_ai.then(|| {
                                            div()
                                                .text_size(gpui_kit::px(10.0))
                                                .text_color(gpui_kit::rgba(0xffffff73))
                                                .child(crate::i18n::t(
                                                    "player.comments.aiGenerated",
                                                ))
                                        }))
                                        .child(
                                            div()
                                                .id(ElementId::from(format!("reply-btn-{rid}")))
                                                .text_size(gpui_kit::px(10.0))
                                                .cursor_pointer()
                                                .text_color(gpui_kit::rgba(0xffffff99))
                                                .hover(|s| s.text_color(gpui_kit::rgb(0xffffff)))
                                                .on_click({
                                                    let rcid = rcid.clone();
                                                    let rid = rid.clone();
                                                    let rname = rname.clone();
                                                    cx.listener(move |this, _, _, cx| {
                                                        this.reply_to = Some((
                                                            rcid.clone(),
                                                            Some(rid.clone()),
                                                            rname.clone(),
                                                        ));
                                                        cx.notify();
                                                    })
                                                })
                                                .child(crate::i18n::t("player.comments.reply")),
                                        )
                                        .children(reply_mine.then(|| {
                                            div()
                                                .id(ElementId::from(format!("reply-del-{rid}")))
                                                .text_size(gpui_kit::px(10.0))
                                                .cursor_pointer()
                                                .text_color(gpui_kit::rgba(0xffffff73))
                                                .hover(|s| s.text_color(gpui_kit::rgb(0xf87171)))
                                                .on_click({
                                                    let rid = rid.clone();
                                                    let rcid = rcid.clone();
                                                    cx.listener(move |this, _, _, cx| {
                                                        this.request_delete(
                                                            rid.clone(),
                                                            Some(rcid.clone()),
                                                            cx,
                                                        );
                                                    })
                                                })
                                                .child(crate::i18n::t("common.delete"))
                                        }))
                                        .child(
                                            // ♡ 回复点赞（乐观 ±1）
                                            div()
                                                .id(ElementId::from(format!("reply-digg-{rid}")))
                                                .flex()
                                                .items_center()
                                                .gap_0p5()
                                                .cursor_pointer()
                                                .text_color(if digged {
                                                    gpui_kit::rgb(0xef4444)
                                                } else {
                                                    gpui_kit::rgba(0xffffff99)
                                                })
                                                .on_click({
                                                    let rid = rid.clone();
                                                    cx.listener(move |this, _, _, cx| {
                                                        this.toggle_reply_digg(rid.clone(), cx);
                                                    })
                                                })
                                                .child(
                                                    Icon::new(gpui_kit::assets::IconName::Heart)
                                                        .size_3(),
                                                )
                                                .children(
                                                    (digg_count > 0).then(|| {
                                                        div().child(format!("{digg_count}"))
                                                    }),
                                                ),
                                        ),
                                ),
                        )
                }))
                .into_any_element()
        })
    }
}
