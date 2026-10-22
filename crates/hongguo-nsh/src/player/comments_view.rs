//! 评论区视图件：评论行 / 回复块 / 回复行 / 头像（`comments.rs` 的渲染半边）。
//!
//! 只做展示与转发：数据操作都在 [`super::comments::CommentsState`] 的方法
//! 上，这里读写它 `pub(super)` 的字段与方法拼装面板内容。

use nsh_core::ui::prelude::*;

use hongguo_api::danmaku::{CommentItem, ReplyItem};

use crate::i18n::t;
use crate::supply;

use super::comments::{CommentsState, composer_input};
use super::{icons, widgets};

/// 一条评论行（头像 / 昵称 / 内容 / 时间·回复 / 点赞）+ 内联回复区。
pub(super) fn comment_row(
    s: CommentsState,
    vid: Signal<String>,
    series_id: Signal<String>,
    logged_in: Signal<bool>,
    item: ReadSignal<CommentItem>,
) -> impl IntoElement {
    let c = item.get();
    let comment_id = c.comment_id.clone();
    // 每个内联 move 闭包吃一份独立克隆（String 不能被多个闭包共享所有权）。
    let replies_id = comment_id.clone();
    let composer_id = comment_id.clone();
    let digg_id = comment_id.clone();
    view()
        .w_full()
        .flex_row()
        .gap(px(10.0))
        .pb(px(16.0))
        .child(avatar(&c.avatar, &c.user_name))
        .child(
            view()
                .flex_1()
                .flex_col()
                .gap(px(2.0))
                .child(
                    text(move || {
                        let name = item.with(|c| c.user_name.clone());
                        if name.is_empty() {
                            t("player.comments.anon")
                        } else {
                            name
                        }
                    })
                    .text_size(px(12.0))
                    .text_color(rgba(0xffffff80)),
                )
                .child(
                    text(move || item.with(|c| c.text.clone()))
                        .text_size(px(14.0))
                        .text_color(rgb(0xffffff)),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .child(
                            text(move || widgets::relative_time(item.with(|c| c.create_time)))
                                .text_size(px(11.0))
                                .text_color(rgba(0xffffff59)),
                        )
                        .child(
                            text(move || t("player.comments.reply"))
                                .text_size(px(11.0))
                                .text_color(rgba(0xffffff66))
                                .on_press({
                                    let id = comment_id.clone();
                                    move |_: &()| {
                                        s.reply_to_comment.set(Some(id.clone()));
                                        s.reply_to_reply.set(None);
                                        s.reply_text.set(String::new());
                                    }
                                }),
                        ),
                )
                // 回复区（原地内联展开，其他评论不隐藏）。
                .child(If::new(
                    {
                        let id = comment_id.clone();
                        let count = c.reply_count;
                        move || count > 0 && s.is_expanded(&id)
                    },
                    move || replies_block(s, vid, series_id, replies_id.clone(), c.reply_count),
                ))
                // 回复输入框（定位到本条时出现）。
                .child(If::new(
                    {
                        let id = comment_id.clone();
                        move || s.reply_to_comment.get().as_deref() == Some(id.as_str())
                    },
                    move || {
                        let placeholder = s
                            .reply_to_reply
                            .with(|r| r.as_ref().map(|(_, name)| name.clone()).unwrap_or_default());
                        view()
                            .w_full()
                            .flex_row()
                            .items_center()
                            .gap(px(8.0))
                            .pt(px(6.0))
                            .child(composer_input(
                                move || s.reply_text.get(),
                                move |v| s.reply_text.set(v),
                                move || {
                                    if placeholder.is_empty() {
                                        t("player.comments.replyToComment")
                                    } else {
                                        t("player.comments.replyPlaceholder")
                                            .replace("{name}", &placeholder)
                                    }
                                },
                                {
                                    let id = composer_id.clone();
                                    move |_: String| {
                                        s.send_reply(vid, series_id, id.clone(), logged_in)
                                    }
                                },
                            ))
                            .child(
                                text(move || t("player.interact.send"))
                                    .text_size(px(12.0))
                                    .text_color(rgb(0xef4444))
                                    .on_press({
                                        let id = composer_id.clone();
                                        move |_: &()| {
                                            s.send_reply(vid, series_id, id.clone(), logged_in)
                                        }
                                    }),
                            )
                    },
                )),
        )
        .child(
            view()
                .flex_col()
                .items_center()
                .gap(px(2.0))
                .pt(px(4.0))
                .child(
                    view()
                        .flex_col()
                        .items_center()
                        .gap(px(2.0))
                        .child(heart(16.0, c.user_digg))
                        .child(If::new(
                            move || c.digg_count > 0,
                            move || {
                                text(move || c.digg_count.to_string())
                                    .text_size(px(10.0))
                                    .text_color(rgba(0xffffff73))
                                    .tabular_nums()
                            },
                        )),
                )
                .on_press(move |_: &()| s.digg_comment(digg_id.clone(), c.user_digg)),
        )
}

/// 点赞红心（激活红、常态灰白）。
fn heart(size: f32, liked: bool) -> CanvasView {
    icons::heart_fill(size, move || {
        if liked {
            rgb(0xef4444)
        } else {
            rgba(0xffffff73)
        }
    })
}

/// 回复块：「展开 n 条回复」→ 回复行列表 → 「更多回复」。
/// 条目与翻页钮都从 `replies` 信号里读，写回后原地翻面。
fn replies_block(
    s: CommentsState,
    vid: Signal<String>,
    series_id: Signal<String>,
    comment_id: String,
    reply_count: i64,
) -> impl IntoElement {
    // 每个内联 move 闭包吃一份独立克隆。
    let expand_id = comment_id.clone();
    let rows_id = comment_id.clone();
    let more_id = comment_id.clone();
    view()
        .w_full()
        .flex_col()
        .gap(px(8.0))
        .pt(px(6.0))
        // 「展开 n 条回复」：块里还没有数据时显示。
        .child(If::new(
            {
                let id = comment_id.clone();
                move || {
                    s.replies
                        .get()
                        .get(&id)
                        .map(|b| b.items.is_empty())
                        .unwrap_or(true)
                }
            },
            move || {
                text(move || {
                    t("player.comments.expandReplies").replace("{n}", &reply_count.to_string())
                })
                .text_size(px(11.0))
                .text_color(rgba(0xffffff80))
                .on_press({
                    let id = expand_id.clone();
                    move |_: &()| s.toggle_replies(vid, series_id, id.clone())
                })
            },
        ))
        .child(Each::new(
            {
                let id = comment_id.clone();
                move || {
                    s.replies
                        .get()
                        .get(&id)
                        .map(|b| b.items.clone())
                        .unwrap_or_default()
                }
            },
            |r: &ReplyItem| r.reply_id.clone(),
            move |item: ReadSignal<ReplyItem>, _| reply_row(s, rows_id.clone(), item),
        ))
        // 「更多回复」。
        .child(If::new(
            {
                let id = comment_id.clone();
                move || {
                    s.replies
                        .get()
                        .get(&id)
                        .map(|b| b.has_more)
                        .unwrap_or(false)
                }
            },
            move || {
                text(move || t("player.comments.moreReplies"))
                    .text_size(px(11.0))
                    .text_color(rgba(0xffffff99))
                    .on_press({
                        let id = more_id.clone();
                        move |_: &()| s.fetch_replies(vid, series_id, id.clone(), false)
                    })
            },
        ))
}

/// 一条回复行（二级回复带「回复 @xxx」前缀；可回复、可点赞）。
fn reply_row(
    s: CommentsState,
    comment_id: String,
    item: ReadSignal<ReplyItem>,
) -> impl IntoElement {
    let reply_id = item.with(|r| r.reply_id.clone());
    let liked = item.with(|r| r.user_digg);
    let digg_count = item.with(|r| r.digg_count);
    let name = item.with(|r| {
        if r.user_name.is_empty() {
            t("player.comments.anon")
        } else {
            r.user_name.clone()
        }
    });
    let name_for_press = name.clone();
    view()
        .w_full()
        .flex_row()
        .gap(px(8.0))
        .child(avatar(&item.with(|r| r.avatar.clone()), &name))
        .child(
            view()
                .flex_1()
                .flex_col()
                .gap(px(2.0))
                .child(
                    text(move || name.clone())
                        .text_size(px(11.0))
                        .text_color(rgba(0xffffff80)),
                )
                .child(
                    text(move || {
                        let to_name = item.with(|r| r.reply_to_name.clone());
                        let prefix = if to_name.is_empty() {
                            String::new()
                        } else {
                            format!("{} @{to_name}：", t("player.comments.replyToPrefix"))
                        };
                        format!("{prefix}{}", item.with(|r| r.text.clone()))
                    })
                    .text_size(px(13.0))
                    .text_color(rgb(0xffffff)),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .child(
                            text(move || widgets::relative_time(item.with(|r| r.create_time)))
                                .text_size(px(10.0))
                                .text_color(rgba(0xffffff59)),
                        )
                        .child(
                            text(move || t("player.comments.reply"))
                                .text_size(px(10.0))
                                .text_color(rgba(0xffffff66))
                                .on_press({
                                    let id = comment_id.clone();
                                    let rid = reply_id.clone();
                                    let name = name_for_press.clone();
                                    move |_: &()| {
                                        s.reply_to_comment.set(Some(id.clone()));
                                        s.reply_to_reply.set(Some((rid.clone(), name.clone())));
                                        s.reply_text.set(String::new());
                                    }
                                }),
                        ),
                ),
        )
        .child(
            view()
                .flex_col()
                .items_center()
                .gap(px(2.0))
                .child(heart(14.0, liked))
                .child(If::new(
                    move || digg_count > 0,
                    move || {
                        text(move || digg_count.to_string())
                            .text_size(px(10.0))
                            .text_color(rgba(0xffffff73))
                    },
                ))
                .on_press(move |_: &()| s.digg_reply(comment_id.clone(), reply_id.clone(), liked)),
        )
}

/// 头像：有 URL 走直连图（官方 CDN，非 HEIC），没有回落首字圆底。
fn avatar(url: &str, name: &str) -> impl IntoElement {
    let url = url.to_owned();
    let empty = url.is_empty();
    let initial = name.chars().next().unwrap_or('友').to_string();
    view()
        .w(px(32.0))
        .h(px(32.0))
        .rounded_full()
        .overflow_hidden()
        .bg(rgba(0xffffff26))
        .items_center()
        .justify_center()
        .child(If::new(
            move || !empty,
            move || {
                let url = url.clone();
                image(move || supply::cover_url(&url))
                    .w(px(32.0))
                    .h(px(32.0))
                    .resize_mode(ResizeMode::Cover)
            },
        ))
        .child(If::new(
            move || empty,
            move || {
                let initial = initial.clone();
                text(move || initial.clone())
                    .text_size(px(12.0))
                    .text_color(rgb(0xffffff))
            },
        ))
}
