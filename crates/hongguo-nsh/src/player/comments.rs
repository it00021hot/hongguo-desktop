//! 播放页评论区（右侧滑出面板，`comment-panel.tsx` 同流）。
//!
//! 数据流 = 单集维度评论流（group=vid / group_type=30，2026-10-16 官方
//! 实包）：按页拉取（20 条/页）+ 底部「加载更多」续拉；回复原地内联
//! 展开、二级回复、回复点赞。发送类接口的响应是**完整对象回显**（头像/
//! 昵称/uid/时间齐全），直接插列表——不手拼条目（抓包三点一线定规）。
//! 社交态不缓存：面板每次打开整页重拉。@提及（小果 AI）与评论删除随
//! P2 账号域一起接。
//!
//! 响应式口径：列表走 `Each`（条目闭包读信号，数据变即重建）；回复块
//! 的条目/翻页钮同样从 `replies` 信号里读，写回后原地翻面。渲染半边
//! （评论行/回复块/头像）在 [`super::comments_view`]。

use std::collections::HashMap;

use nsh_core::ui::prelude::*;

use hongguo_api::danmaku::{CommentItem, ReplyItem};

use crate::i18n::t;

use super::comments_view::comment_row;
use super::icons;

/// 一条评论的回复块。
#[derive(Clone, Default)]
pub(super) struct ReplyBlock {
    pub(super) items: Vec<ReplyItem>,
    cursor: String,
    pub(super) has_more: bool,
    loading: bool,
}

/// 评论面板的全部状态（`Copy` 句柄束）。视图半边读的字段/方法对
/// `player/` 内可见（`pub(super)`）。
#[derive(Clone, Copy)]
pub(crate) struct CommentsState {
    pub open: Signal<bool>,
    items: Signal<Vec<CommentItem>>,
    total: Signal<i64>,
    cursor: Stored<String>,
    has_more: Stored<bool>,
    loading: Signal<bool>,
    error: Signal<String>,
    /// 展开了回复区的评论 id 集。
    expanded: Signal<Vec<String>>,
    /// 各评论的回复块（key = comment_id）。
    pub(super) replies: Signal<HashMap<String, ReplyBlock>>,
    /// 回复框定位：挂在哪条评论下。
    pub(super) reply_to_comment: Signal<Option<String>>,
    /// 二级回复目标（被回复的那条回复的 id 与昵称）。
    pub(super) reply_to_reply: Signal<Option<(String, String)>>,
    pub(super) reply_text: Signal<String>,
    /// 底部主输入框草稿。
    draft: Signal<String>,
    sending: Signal<bool>,
}

impl CommentsState {
    pub(crate) fn new() -> Self {
        Self {
            open: signal(false),
            items: signal_with(Vec::new(), SignalEquals::Never),
            total: signal(0),
            cursor: stored(String::new()),
            has_more: stored(false),
            loading: signal(false),
            error: signal(String::new()),
            expanded: signal_with(Vec::new(), SignalEquals::Never),
            replies: signal_with(HashMap::new(), SignalEquals::Never),
            reply_to_comment: signal(None),
            reply_to_reply: signal(None),
            reply_text: signal(String::new()),
            draft: signal(String::new()),
            sending: signal(false),
        }
    }

    /// 打开并整页重拉（社交态不缓存；vid 未就绪时只开面板等档案）。
    pub(crate) fn open(&self, vid: Signal<String>, series_id: Signal<String>) {
        self.open.set(true);
        if vid.get().is_empty() || self.loading.get() {
            return;
        }
        self.reset_data();
        self.fetch_page(vid, series_id, true);
    }

    /// 清数据（每次打开都重拉）。
    fn reset_data(&self) {
        batch(|| {
            self.items.set(Vec::new());
            self.total.set(0);
            self.error.set(String::new());
            self.expanded.set(Vec::new());
            self.replies.set(HashMap::new());
            self.reply_to_comment.set(None);
            self.reply_to_reply.set(None);
            self.reply_text.set(String::new());
            self.draft.set(String::new());
        });
        self.cursor.replace(String::new());
        self.has_more.replace(false);
    }

    /// 拉一页评论（首页 or 翻页）。
    fn fetch_page(&self, vid: Signal<String>, series_id: Signal<String>, first: bool) {
        self.loading.set(true);
        let state = crate::state::state();
        let cursor = if first {
            String::new()
        } else {
            self.cursor.with(|v| v.clone())
        };
        let s = *self;
        spawn(async move {
            let result = background(async move {
                super::feed::fetch_comments(&vid.get(), &series_id.get(), &cursor, &state).await
            })
            .await;
            batch(|| {
                s.loading.set(false);
                match result {
                    Ok(Ok(page)) => {
                        if first {
                            s.items.set(page.items);
                        } else {
                            s.items.update(|list| list.extend(page.items));
                        }
                        s.total.set(page.total);
                        s.cursor.replace(page.next_cursor);
                        s.has_more.replace(page.has_more);
                        s.error.set(String::new());
                    }
                    Ok(Err(e)) => s
                        .error
                        .set(format!("{}（{e}）", t("player.comments.loadFailed"))),
                    Err(e) => s
                        .error
                        .set(format!("{}（{e}）", t("player.comments.loadFailed"))),
                }
            });
        });
    }

    /// 底部「加载更多」。
    pub(super) fn load_more(&self, vid: Signal<String>, series_id: Signal<String>) {
        if self.loading.get() || !self.has_more.with(|v| *v) {
            return;
        }
        self.fetch_page(vid, series_id, false);
    }

    /// 展开/收起一条评论的回复区（首开拉第一页）。
    pub(super) fn toggle_replies(
        &self,
        vid: Signal<String>,
        series_id: Signal<String>,
        comment_id: String,
    ) {
        let was_open = self.expanded.get().contains(&comment_id);
        self.expanded.update(|list| {
            if was_open {
                list.retain(|id| id != &comment_id);
            } else {
                list.push(comment_id.clone());
            }
        });
        if was_open || self.replies.get().contains_key(&comment_id) {
            return;
        }
        self.fetch_replies(vid, series_id, comment_id, true);
    }

    pub(super) fn fetch_replies(
        &self,
        vid: Signal<String>,
        series_id: Signal<String>,
        comment_id: String,
        first: bool,
    ) {
        let (cursor, has) = self.replies.with(|map| {
            map.get(&comment_id)
                .map(|b| (b.cursor.clone(), b.has_more))
                .unwrap_or((String::new(), false))
        });
        if !first && !has {
            return;
        }
        self.replies.update(|map| {
            let block = map.entry(comment_id.clone()).or_default();
            block.loading = true;
        });
        let s = *self;
        let state = crate::state::state();
        spawn(async move {
            let start = if first { String::new() } else { cursor };
            let id = comment_id.clone();
            let result = background(async move {
                super::feed::fetch_replies(&vid.get(), &series_id.get(), &id, &start, &state).await
            })
            .await;
            s.replies.update(|map| {
                let block = map.entry(comment_id.clone()).or_default();
                block.loading = false;
                if let Ok(Ok(page)) = result {
                    if first {
                        block.items = page.items;
                    } else {
                        block.items.extend(page.items);
                    }
                    block.cursor = page.next_cursor;
                    block.has_more = page.has_more;
                }
            });
        });
    }

    pub(super) fn is_expanded(&self, comment_id: &str) -> bool {
        self.expanded.get().iter().any(|id| id == comment_id)
    }

    /// 发评论：服务端回显的完整对象插顶，计数 +1。
    pub(crate) fn send_comment(
        &self,
        vid: Signal<String>,
        series_id: Signal<String>,
        logged_in: Signal<bool>,
    ) {
        if !gate_login(logged_in) {
            return;
        }
        let text = self.draft.with(|v| v.trim().to_owned());
        if text.is_empty() || self.sending.get() || !vid.get().contains(':') {
            return;
        }
        self.sending.set(true);
        let s = *self;
        let state = crate::state::state();
        spawn(async move {
            let result = background(async move {
                hongguo_api::interact::send_comment(
                    &vid.get(),
                    &series_id.get(),
                    &text,
                    &state.api_env(),
                )
                .await
            })
            .await;
            s.sending.set(false);
            match result {
                Ok(Ok(comment)) => {
                    batch(|| {
                        s.items.update(|list| list.insert(0, comment));
                        s.total.update(|v| *v += 1);
                        s.draft.set(String::new());
                    });
                }
                Ok(Err(e)) => log::warn!("[Comments] 发送失败: {e}"),
                Err(e) => log::warn!("[Comments] 发送任务失败: {e}"),
            }
        });
    }

    /// 发回复（含二级）：回显对象进回复区，父评论 replyCount +1，
    /// 强制展开回复区（hgplayer 同款）。
    pub(super) fn send_reply(
        &self,
        vid: Signal<String>,
        series_id: Signal<String>,
        parent_id: String,
        logged_in: Signal<bool>,
    ) {
        if !gate_login(logged_in) {
            return;
        }
        let text = self.reply_text.with(|v| v.trim().to_owned());
        if text.is_empty() || self.sending.get() {
            return;
        }
        let to_reply = self.reply_to_reply.get();
        self.sending.set(true);
        let s = *self;
        let state = crate::state::state();
        spawn(async move {
            let pid = parent_id.clone();
            let result = background(async move {
                hongguo_api::interact::send_reply(
                    &vid.get(),
                    &series_id.get(),
                    &pid,
                    to_reply.as_ref().map(|(id, _)| id.as_str()),
                    &text,
                    &state.api_env(),
                )
                .await
            })
            .await;
            s.sending.set(false);
            if let Ok(Ok(reply)) = result {
                batch(|| {
                    s.replies.update(|map| {
                        let block = map.entry(parent_id.clone()).or_default();
                        block.items.push(reply);
                    });
                    s.items.update(|list| {
                        if let Some(item) = list.iter_mut().find(|c| c.comment_id == parent_id) {
                            item.reply_count += 1;
                        }
                    });
                    s.reply_text.set(String::new());
                    s.reply_to_reply.set(None);
                    s.reply_to_comment.set(None);
                    let id = parent_id.clone();
                    s.expanded.update(|list| {
                        if !list.contains(&id) {
                            list.push(id);
                        }
                    });
                });
            }
        });
    }

    /// 评论点赞（do_action 8/9）：乐观翻面 ±1，失败回滚。
    pub(super) fn digg_comment(&self, comment_id: String, liked: bool) {
        let next = !liked;
        self.items.update(|list| {
            if let Some(item) = list.iter_mut().find(|c| c.comment_id == comment_id) {
                item.user_digg = next;
                item.digg_count = (item.digg_count + if next { 1 } else { -1 }).max(0);
            }
        });
        let state = crate::state::state();
        let s = *self;
        spawn(async move {
            let id = comment_id.clone();
            let result = background(async move {
                hongguo_api::interact::digg_comment(&id, next, &state.api_env()).await
            })
            .await;
            if let Ok(Err(e)) = result {
                s.items.update(|list| {
                    if let Some(item) = list.iter_mut().find(|c| c.comment_id == comment_id) {
                        item.user_digg = liked;
                        item.digg_count = (item.digg_count + if liked { 1 } else { -1 }).max(0);
                    }
                });
                log::warn!("[Comments] 评论点赞失败: {e}");
            }
        });
    }

    /// 回复点赞（回复同形态传 reply_id）。
    pub(super) fn digg_reply(&self, comment_id: String, reply_id: String, liked: bool) {
        let next = !liked;
        self.replies.update(|map| {
            if let Some(block) = map.get_mut(&comment_id)
                && let Some(reply) = block.items.iter_mut().find(|r| r.reply_id == reply_id)
            {
                reply.user_digg = next;
                reply.digg_count = (reply.digg_count + if next { 1 } else { -1 }).max(0);
            }
        });
        let state = crate::state::state();
        let s = *self;
        spawn(async move {
            let rid = reply_id.clone();
            let result = background(async move {
                hongguo_api::interact::digg_comment(&rid, next, &state.api_env()).await
            })
            .await;
            if let Ok(Err(e)) = result {
                s.replies.update(|map| {
                    if let Some(block) = map.get_mut(&comment_id)
                        && let Some(reply) = block.items.iter_mut().find(|r| r.reply_id == reply_id)
                    {
                        reply.user_digg = liked;
                        reply.digg_count = (reply.digg_count + if liked { 1 } else { -1 }).max(0);
                    }
                });
                log::warn!("[Comments] 回复点赞失败: {e}");
            }
        });
    }

    /// 面板视图（右侧滑出，压控制栏上方）。收 `self`（Copy）而非
    /// `&self`——Rust 2024 的 RPIT 会捕获引用生命周期。
    pub(crate) fn panel(
        self,
        vid: Signal<String>,
        series_id: Signal<String>,
        logged_in: Signal<bool>,
    ) -> impl IntoElement {
        let s = self;
        view()
            .absolute()
            .right(px(0.0))
            .bottom(px(super::info::BOTTOM_BAR_H))
            .w(px(380.0))
            .h(px(480.0))
            .flex_col()
            .rounded_tl(px(12.0))
            .border_1()
            .border_color(rgba(0xffffff1a))
            .bg(rgba(0x0a0a0af2))
            .on_press(move |_: &()| {}) // 吞面板内点击
            // 头部：标题 + 总数 + 关闭。
            .child(
                view()
                    .h(px(48.0))
                    .px(px(16.0))
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgba(0xffffff1a))
                    .child(
                        view().flex_row().items_center().gap(px(8.0)).child(
                            text(move || t("player.comments.title"))
                                .text_size(px(14.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(0xffffff)),
                        ),
                    )
                    .child(
                        icons::close(16.0, || rgba(0xffffff99))
                            .on_press(move |_: &()| s.open.set(false)),
                    ),
            )
            // 列表。
            .child(
                scroll()
                    .flex_1()
                    .px(px(16.0))
                    .py(px(12.0))
                    .child(If::new(
                        move || vid.get().is_empty(),
                        move || centered_note(move || t("player.preparingShort")),
                    ))
                    .child(If::new(
                        move || s.loading.get() && s.items.get().is_empty(),
                        move || {
                            view()
                                .py(px(40.0))
                                .items_center()
                                .child(icons::spinner(20.0, || rgba(0xffffff99)).animate_spin())
                        },
                    ))
                    .child(If::new(
                        move || !s.error.get().is_empty(),
                        move || centered_note(move || s.error.get()),
                    ))
                    .child(Each::new(
                        move || s.items.get(),
                        |c: &CommentItem| c.comment_id.clone(),
                        move |item: ReadSignal<CommentItem>, _| {
                            comment_row(s, vid, series_id, logged_in, item)
                        },
                    ))
                    .child(If::new(
                        move || {
                            !s.items.get().is_empty()
                                && !s.loading.get()
                                && s.error.get().is_empty()
                                && s.has_more.with(|v| *v)
                        },
                        move || {
                            text(move || {
                                t("player.comments.loadMore")
                                    .replace("{n}", &s.total.get().to_string())
                            })
                            .text_size(px(12.0))
                            .text_color(rgba(0xffffff99))
                            .py(px(10.0))
                            .on_press(move |_: &()| s.load_more(vid, series_id))
                        },
                    ))
                    .child(If::new(
                        move || {
                            !s.loading.get()
                                && s.error.get().is_empty()
                                && s.items.get().is_empty()
                                && !vid.get().is_empty()
                        },
                        move || centered_note(move || t("player.comments.empty")),
                    )),
            )
            // 底部发评论。
            .child(
                view()
                    .px(px(12.0))
                    .py(px(10.0))
                    .border_t_1()
                    .border_color(rgba(0xffffff1a))
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(composer_input(
                        move || s.draft.get(),
                        move |v| s.draft.set(v),
                        move || t("player.comments.placeholder"),
                        move |_: String| s.send_comment(vid, series_id, logged_in),
                    ))
                    .child(
                        icons::send(16.0, || rgb(0xffffff))
                            .on_press(move |_: &()| s.send_comment(vid, series_id, logged_in)),
                    ),
            )
    }
}

/// 居中的提示文字。
fn centered_note(label: impl Fn() -> String + 'static) -> impl IntoElement {
    view()
        .w_full()
        .py(px(40.0))
        .items_center()
        .child(text(label).text_size(px(12.0)).text_color(rgba(0xffffff80)))
}

/// 面板里的输入框（主输入框与回复框共用外观）。
pub(super) fn composer_input(
    value: impl Fn() -> String + 'static,
    on_change: impl Fn(String) + 'static,
    placeholder: impl Fn() -> String + 'static,
    on_submit: impl Fn(String) + 'static,
) -> impl IntoElement {
    input()
        .flex_1()
        .h(px(32.0))
        .px(px(12.0))
        .rounded(px(8.0))
        .bg(rgba(0xffffff14))
        .text_size(px(12.0))
        .text_color(rgb(0xffffff))
        .placeholder(placeholder)
        .placeholder_color(rgba(0xffffff66))
        .value(value)
        .on_change_text(move |v: &str| on_change(v.to_owned()))
        .on_submit_editing(move |v: &str| on_submit(v.to_owned()))
}

/// 未登录点发送：指路提示，不放行重试。
fn gate_login(logged_in: Signal<bool>) -> bool {
    if logged_in.get() {
        return true;
    }
    toast(t("player.interact.loginRequired"), ToastDuration::Long);
    false
}
