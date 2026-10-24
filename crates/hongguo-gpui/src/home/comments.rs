//! 评论/回复面板：拉取（首页/翻页/回复）、发送、点赞、删除（确认浮层）。
//! 面板渲染在 super::comment_view。

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::bridge;
use gpui_kit::prelude::*;
use hongguo_api::danmaku::{fetch_comment_replies, fetch_comments_page};
use hongguo_api::interact::{
    CommentMention, delete_comment, digg_comment, send_comment, send_comment_mention, send_reply,
};

/// `aibot_mention_context` 带的最近评论条数（官方实包 4–6 条，Tauri 同款）。
const MENTION_CONTEXT_LIMIT: usize = 6;
/// @小果 AI 回复轮询间隔与放弃时限（官方到达 10–40s，留足余量；Tauri 同参）。
const AI_REPLY_POLL_SECS: u64 = 5;
const AI_REPLY_WAIT_SECS: u64 = 120;

/// 剥掉首个提及段（`@名字\u{FFFC}` + 尾随空白），与 Tauri 发送前的
/// `content.replace(/@[^\s\uFFFC]*\u{FFFC}\s*/, '')` 同口径；正文可空
/// （官方允许纯 @ 提及）。返回 trim 后的正文。
fn strip_mention_segment(text: &str) -> String {
    for (i, _) in text.match_indices('@') {
        let rest = &text[i + '@'.len_utf8()..];
        let mut name_end = None;
        for (j, c) in rest.char_indices() {
            if c == '\u{FFFC}' {
                name_end = Some(j);
                break;
            }
            if c.is_whitespace() {
                break;
            }
        }
        let Some(j) = name_end else { continue };
        if j == 0 {
            continue; // 空名字不算提及段
        }
        let after = i + '@'.len_utf8() + j + '\u{FFFC}'.len_utf8();
        let trimmed = text[after..].trim_start();
        let mut out = String::with_capacity(text.len());
        out.push_str(&text[..i]);
        out.push_str(trimmed);
        return out.trim().to_string();
    }
    text.trim().to_string()
}

impl super::HomeView {
    /// 当前集的评论键就绪（vid 非空才可拉）。
    fn comments_ready(&self) -> bool {
        self.now.as_ref().is_some_and(|n| !n.vid.is_empty())
    }

    /// 拉 @提及候选（relation/list，含 AI bot「小果」）：按剧缓存一次；
    /// 失败静默留空表（弹层回落不可用提示）。
    pub(super) fn load_mention_users(&mut self, cx: &mut Context<Self>) {
        let Some(now) = self.now.clone() else {
            return;
        };
        let sid = now.series_id.clone();
        if self.mention_loaded_for.as_deref() == Some(sid.as_str()) || self.mention_pending {
            return;
        }
        self.mention_pending = true;
        cx.notify();
        let state = Arc::clone(&self.state);
        let key = sid.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::interact::fetch_mention_list(&key, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.mention_pending = false;
                match result {
                    Ok(users) => {
                        log::info!("[Home] @提及候选 {} 人", users.len());
                        // 候选头像走 gpui 的图片缓存（失败退回首字占位）
                        let urls: Vec<String> = users
                            .iter()
                            .map(|u| u.avatar.clone())
                            .filter(|a| !a.is_empty())
                            .collect();
                        crate::cover_img::prewarm_avatars(&urls, cx);
                        view.mention_users = Arc::new(users);
                        view.mention_loaded_for = Some(sid);
                    }
                    Err(e) => log::warn!("[Home] @提及候选拉取失败: {e}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 评论面板数据：拉首页（社交态不缓存复用；换集重置）。
    pub(super) fn load_comments_first_page(&mut self, cx: &mut Context<Self>) {
        let Some(now) = self.now.clone() else {
            return;
        };
        if self.comments_loading || !self.comments_ready() {
            return;
        }
        // 同集且已有数据：本会话内保留（面板重开不重拉）
        if self.comments_vid == now.vid && !self.comments.is_empty() {
            return;
        }
        self.comments_loading = true;
        self.comments_vid = now.vid.clone();
        self.comments_cursor.clear();
        self.comments_has_more = false;
        let sid = now.series_id.clone();
        let vid = now.vid.clone();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            fetch_comments_page(&vid, &sid, "", &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.comments_loading = false;
                match result {
                    Ok(page) => {
                        view.comments_total = page.total;
                        view.comments_has_more = page.has_more;
                        view.comments_cursor = page.next_cursor;
                        view.comments = Arc::new(page.items);
                        let urls: Vec<String> =
                            view.comments.iter().map(|c| c.avatar.clone()).collect();
                        for url in urls {
                            crate::cover_img::prewarm_avatars(std::slice::from_ref(&url), cx);
                        }
                    }
                    Err(e) => log::warn!("[Home] 评论拉取失败: {e}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 评论翻页（滚到尾部加载下一页；相邻页重叠条目按 comment_id 去重）。
    pub(super) fn load_comments_next(&mut self, cx: &mut Context<Self>) {
        if self.comments_loading || !self.comments_has_more || !self.comments_ready() {
            return;
        }
        let Some(now) = self.now.clone() else {
            return;
        };
        self.comments_loading = true;
        let sid = now.series_id.clone();
        let vid = now.vid.clone();
        let cursor = self.comments_cursor.clone();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            fetch_comments_page(&vid, &sid, &cursor, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.comments_loading = false;
                if let Ok(page) = result {
                    view.comments_has_more = page.has_more;
                    view.comments_cursor = page.next_cursor;
                    let mut items = (*view.comments).clone();
                    for c in page.items {
                        if !items.iter().any(|x| x.comment_id == c.comment_id) {
                            items.push(c);
                        }
                    }
                    view.comments = Arc::new(items);
                    let urls: Vec<String> =
                        view.comments.iter().map(|c| c.avatar.clone()).collect();
                    for url in urls {
                        crate::cover_img::prewarm_avatars(std::slice::from_ref(&url), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 发评论（comment/add；**服务端完整回显对象**直接插列表顶部——抓包
    /// 三点一线：头像/昵称/uid/时间齐全，不自拼）。草稿含提及段占位符
    /// `\u{FFFC}` 时按 **@提及评论**发送（rich_text tp=15 + 最近评论 id
    /// 的 aibot_mention_context，Tauri comment_send 同款），成功后启 AI
    /// 回复轮询并强制展开该条的回复区。
    pub(super) fn submit_comment(&mut self, cx: &mut Context<Self>) {
        let Some(input) = &self.comment_input else {
            return;
        };
        let raw = input.read(cx).value().to_string();
        let is_mention = raw.contains('\u{FFFC}');
        let text = strip_mention_segment(&raw);
        if (text.is_empty() && !is_mention) || self.comment_sending {
            return;
        }
        if !self.logged_in() {
            self.show_toast(crate::i18n::t("player.interact.loginRequired"), cx);
            return;
        }
        if !self.comments_ready() {
            return;
        }
        let Some(now) = self.now.clone() else {
            return;
        };
        let mention = is_mention
            .then(|| self.mention_user.clone())
            .flatten()
            .map(|(uid, name)| CommentMention {
                user_id: uid,
                user_name: name,
                comment_ids: self
                    .comments
                    .iter()
                    .take(MENTION_CONTEXT_LIMIT)
                    .map(|c| c.comment_id.clone())
                    .collect(),
            });
        let send_mention = mention.is_some();
        self.comment_sending = true;
        let state = Arc::clone(&self.state);
        let (vid, sid) = (now.vid.clone(), now.series_id.clone());
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            match mention {
                Some(m) => send_comment_mention(&vid, &sid, &m, &text, &env).await,
                None => send_comment(&vid, &sid, &text, &env).await,
            }
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.comment_sending = false;
                match result {
                    Ok(item) => {
                        let avatar = item.avatar.clone();
                        let cid = item.comment_id.clone();
                        let mut items = (*view.comments).clone();
                        items.insert(0, item);
                        view.comments = Arc::new(items);
                        view.comments_total += 1;
                        view.ep_comment_count += 1;
                        view.comment_clear_input = true;
                        // @提及评论：回复区强制展开 + 起 AI 回复轮询
                        // （Tauri setAiPending + setReplySectionOpen 同款）
                        if send_mention {
                            view.ai_pending.push((cid.clone(), Instant::now()));
                            view.replies_open.insert(cid);
                            view.start_ai_reply_poll(cx);
                        }
                        crate::cover_img::prewarm_avatars(std::slice::from_ref(&avatar), cx);
                    }
                    Err(e) => {
                        // 失败必须可见（风控/频控拒绝时静默会被当成发成功）
                        log::warn!("[Home] 评论发送失败: {e}");
                        let message = crate::i18n::app_error_text(&e);
                        view.show_toast(
                            crate::i18n::tf("common.failedWith", &[("message", message.as_str())]),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// @小果 提及评论的 AI 回复轮询（Tauri aiPending 同款）：每 5 秒拉一次
    /// 该评论的回复首页，等到回复即写进回复区（按 reply_id 去重 + 「展开
    /// N 条」计数增量，权威计数随下次重拉对齐服务端）并撤「思考中…」；
    /// 120 秒没等到就放弃。单实例循环，ai_pending 空即收工。
    fn start_ai_reply_poll(&mut self, cx: &mut Context<Self>) {
        if self.ai_polling {
            return;
        }
        self.ai_polling = true;
        let state = Arc::clone(&self.state);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(AI_REPLY_POLL_SECS))
                    .await;
                let probe = this
                    .update(cx, |view, _| {
                        let vids = view
                            .now
                            .as_ref()
                            .map(|n| (n.vid.clone(), n.series_id.clone()));
                        (view.ai_pending.clone(), vids)
                    })
                    .ok();
                let Some((mut pending, Some((vid, sid)))) = probe else {
                    break;
                };
                pending.retain(|(_, t)| t.elapsed() <= Duration::from_secs(AI_REPLY_WAIT_SECS));
                if pending.is_empty() {
                    break;
                }
                let _ = this.update(cx, |view, _| view.ai_pending = pending.clone());
                for (cid, _) in pending {
                    let state = Arc::clone(&state);
                    let (v, s, c) = (vid.clone(), sid.clone(), cid.clone());
                    let rx = bridge::spawn(async move {
                        let env = state.api_env();
                        fetch_comment_replies(&v, &s, &c, "", &env).await
                    });
                    let page = rx.await.expect("tokio 桥不会丢弃发送端");
                    let Ok(page) = page else { continue };
                    let _ = this.update(cx, |view, cx| {
                        let existing = view.replies.get(&cid).cloned().unwrap_or_default();
                        let mut list = (*existing).clone();
                        let mut added = 0i64;
                        for r in page.items {
                            if !list.iter().any(|x| x.reply_id == r.reply_id) {
                                list.push(r);
                                added += 1;
                            }
                        }
                        if added > 0 {
                            view.replies.insert(cid.clone(), Arc::new(list));
                            view.replies_open.insert(cid.clone());
                            let mut items = (*view.comments).clone();
                            if let Some(hit) = items.iter_mut().find(|c| c.comment_id == cid) {
                                hit.reply_count += added;
                            }
                            view.comments = Arc::new(items);
                            view.ai_pending.retain(|(id, _)| id != &cid);
                            cx.notify();
                        }
                    });
                }
            }
            let _ = this.update(cx, |view, _| view.ai_polling = false);
        })
        .detach();
    }

    /// 评论点赞（乐观 ±1 + 红心翻转，失败回滚到服务端态）。
    pub(super) fn toggle_comment_digg(&mut self, comment_id: String, cx: &mut Context<Self>) {
        if !self.logged_in() {
            self.show_toast(crate::i18n::t("player.interact.loginRequired"), cx);
            return;
        }
        let Some(item) = self
            .comments
            .iter()
            .find(|c| c.comment_id == comment_id)
            .cloned()
        else {
            return;
        };
        let current = self
            .comment_digg_override
            .get(&comment_id)
            .copied()
            .unwrap_or(item.user_digg);
        let next = !current;
        let delta = if next { 1 } else { -1 };
        self.comment_digg_override.insert(comment_id.clone(), next);
        let mut items = (*self.comments).clone();
        if let Some(hit) = items.iter_mut().find(|c| c.comment_id == comment_id) {
            hit.digg_count += delta;
        }
        self.comments = Arc::new(items);
        let state = Arc::clone(&self.state);
        let cid = comment_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            digg_comment(&cid, next, &env).await.map(|_| next)
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, _| {
                if let Err(e) = result {
                    log::warn!("[Home] 评论点赞失败: {e}");
                    view.comment_digg_override.remove(&comment_id);
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// 展开/收起回复（展开才拉首页；发送回复成功后强制展开让本地追加
    /// 的那条立刻可见——Tauri 同款）。
    pub(super) fn toggle_replies(
        &mut self,
        comment_id: String,
        reply_count: i64,
        cx: &mut Context<Self>,
    ) {
        if self.replies_open.remove(&comment_id) {
            cx.notify();
            return;
        }
        self.replies_open.insert(comment_id.clone());
        cx.notify();
        if reply_count <= 0 || self.replies.contains_key(&comment_id) {
            return;
        }
        self.fetch_replies(comment_id, String::new(), cx);
    }

    /// 拉一页回复（首页或翻页）。
    pub(super) fn fetch_replies(
        &mut self,
        comment_id: String,
        cursor: String,
        cx: &mut Context<Self>,
    ) {
        if self.replies_loading.contains(&comment_id) {
            return;
        }
        let Some(now) = self.now.clone() else {
            return;
        };
        self.replies_loading.insert(comment_id.clone());
        let sid = now.series_id.clone();
        let vid = now.vid.clone();
        let cid = comment_id.clone();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            fetch_comment_replies(&vid, &sid, &cid, &cursor, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.replies_loading.remove(&comment_id);
                match result {
                    Ok(page) => {
                        view.replies_cursor
                            .insert(comment_id.clone(), page.next_cursor);
                        let mut items = view
                            .replies
                            .get(&comment_id)
                            .map(|v| (**v).clone())
                            .unwrap_or_default();
                        for r in page.items {
                            if !items.iter().any(|x| x.reply_id == r.reply_id) {
                                items.push(r);
                            }
                        }
                        let urls: Vec<String> = items.iter().map(|r| r.avatar.clone()).collect();
                        view.replies.insert(comment_id, Arc::new(items));
                        for url in urls {
                            crate::cover_img::prewarm_avatars(std::slice::from_ref(&url), cx);
                        }
                    }
                    Err(e) => log::warn!("[Home] 回复拉取失败: {e}"),
                }
            });
        })
        .detach();
    }

    /// 发回复（本地追加一份——服务端列表有延迟；追加后强制展开）。
    pub(super) fn submit_reply(&mut self, cx: &mut Context<Self>) {
        let Some(input) = &self.comment_input else {
            return;
        };
        let text = input.read(cx).value().trim().to_string();
        let Some((comment_id, reply_to_reply_id, _name)) = self.reply_to.clone() else {
            return;
        };
        if text.is_empty() || self.comment_sending {
            return;
        }
        if !self.logged_in() {
            self.show_toast(crate::i18n::t("player.interact.loginRequired"), cx);
            return;
        }
        let Some(now) = self.now.clone() else {
            return;
        };
        self.comment_sending = true;
        let state = Arc::clone(&self.state);
        let (vid, sid) = (now.vid.clone(), now.series_id.clone());
        let cid = comment_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            send_reply(&vid, &sid, &cid, reply_to_reply_id.as_deref(), &text, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.comment_sending = false;
                match result {
                    Ok(reply) => {
                        let mut items = view
                            .replies
                            .get(&comment_id)
                            .map(|v| (**v).clone())
                            .unwrap_or_default();
                        if !items.iter().any(|r| r.reply_id == reply.reply_id) {
                            items.push(reply);
                        }
                        view.replies.insert(comment_id.clone(), Arc::new(items));
                        view.replies_open.insert(comment_id.clone());
                        let avatar = view
                            .replies
                            .get(&comment_id)
                            .and_then(|v| v.last().map(|r| r.avatar.clone()))
                            .unwrap_or_default();
                        // 父评论 replyCount +1（Tauri submitReply onSuccess
                        // 同款）——「展开 N 条回复」与实际条数一致，也是
                        // 删除回复 -1 的对称面
                        let mut parents = (*view.comments).clone();
                        if let Some(hit) = parents.iter_mut().find(|c| c.comment_id == comment_id) {
                            hit.reply_count += 1;
                        }
                        view.comments = Arc::new(parents);
                        view.reply_to = None;
                        view.comment_clear_input = true;
                        crate::cover_img::prewarm_avatars(std::slice::from_ref(&avatar), cx);
                    }
                    Err(e) => log::warn!("[Home] 回复发送失败: {e}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 点「删除」：记下待确认目标，弹确认浮层（Tauri setDeleteTarget 同款；
    /// 真删除在 confirm_delete）。
    pub(super) fn request_delete(
        &mut self,
        id: String,
        parent: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.comment_delete_target = Some((id, parent));
        cx.notify();
    }

    /// 确认浮层「取消」/点遮罩：只清目标态。
    pub(super) fn cancel_delete(&mut self, cx: &mut Context<Self>) {
        if self.comment_delete_target.take().is_some() {
            cx.notify();
        }
    }

    /// 确认删除：comment/del service_id=4（评论与回复同款，data_type=9）。
    /// 成功后**本地摘除**，不重取整列表（Tauri confirmDelete 同款）：
    /// 回复 → 回复列表摘除 + 父评论 replyCount-1；评论 → 列表摘除 +
    /// total-1 + 关掉其展开态/回复目标。
    pub(super) fn confirm_delete(&mut self, cx: &mut Context<Self>) {
        let Some((id, parent)) = self.comment_delete_target.take() else {
            return;
        };
        if self.comment_deleting || !self.logged_in() {
            return;
        }
        self.comment_deleting = true;
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn({
            let id = id.clone();
            async move {
                let env = state.api_env();
                delete_comment(&id, 4, &env).await
            }
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.comment_deleting = false;
                match result {
                    Ok(()) => {
                        if let Some(parent_id) = parent {
                            // 回复：回复缓存摘除 + 父评论 replyCount-1
                            //（与发送成功的 +1 对称，Tauri confirmDelete 同款）
                            let mut items = view
                                .replies
                                .get(&parent_id)
                                .map(|v| (**v).clone())
                                .unwrap_or_default();
                            items.retain(|r| r.reply_id != id);
                            view.replies.insert(parent_id.clone(), Arc::new(items));
                            let mut parents = (*view.comments).clone();
                            if let Some(hit) =
                                parents.iter_mut().find(|c| c.comment_id == parent_id)
                            {
                                hit.reply_count = (hit.reply_count - 1).max(0);
                            }
                            view.comments = Arc::new(parents);
                        } else {
                            let mut items = (*view.comments).clone();
                            items.retain(|c| c.comment_id != id);
                            view.comments = Arc::new(items);
                            view.comments_total = (view.comments_total - 1).max(0);
                            view.ep_comment_count = (view.ep_comment_count - 1).max(0);
                            // 被删评论的展开回复态与回复目标一并清掉
                            view.replies_open.remove(&id);
                            view.replies.remove(&id);
                            if view.reply_to.as_ref().is_some_and(|(rc, _, _)| rc == &id) {
                                view.reply_to = None;
                            }
                        }
                        log::info!("[Home] 评论/回复已删除（本地摘除）");
                    }
                    Err(e) => log::warn!("[Home] 评论删除失败: {e}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 开关评论面板：打开时拉当前集评论首页。
    pub(super) fn toggle_comments_panel(&mut self, cx: &mut Context<Self>) {
        self.comments_panel_open = !self.comments_panel_open;
        self.wake_controls(cx);
        if self.comments_panel_open {
            self.load_comments_first_page(cx);
            // @提及候选随面板打开确保在位（按剧缓存一次）
            self.load_mention_users(cx);
        }
        cx.notify();
    }

    /// 关评论面板（Esc 分层用：面板开着时 Esc 先关它、不退连播——
    /// Tauri comment-panel 自带 Esc 监听同序）。
    pub(super) fn close_comments_panel(&mut self, cx: &mut Context<Self>) {
        if self.comments_panel_open {
            self.comments_panel_open = false;
            self.wake_controls(cx);
            cx.notify();
        }
    }

    /// 回复点赞（乐观 ±1 + 红心翻转，失败回滚；与评论点赞同一 do_action）。
    pub(super) fn toggle_reply_digg(&mut self, reply_id: String, cx: &mut Context<Self>) {
        if !self.logged_in() {
            self.show_toast(crate::i18n::t("player.interact.loginRequired"), cx);
            return;
        }
        let Some(reply) = self
            .replies
            .values()
            .flat_map(|v| v.iter())
            .find(|r| r.reply_id == reply_id)
            .cloned()
        else {
            return;
        };
        let current = self
            .comment_digg_override
            .get(&reply_id)
            .copied()
            .unwrap_or(reply.user_digg);
        let next = !current;
        let delta = if next { 1 } else { -1 };
        self.comment_digg_override.insert(reply_id.clone(), next);
        for list in self.replies.values_mut() {
            if let Some(items) = Arc::get_mut(list)
                && let Some(hit) = items.iter_mut().find(|r| r.reply_id == reply_id)
            {
                hit.digg_count += delta;
            }
        }
        let state = Arc::clone(&self.state);
        let rid = reply_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            digg_comment(&rid, next, &env).await.map(|_| next)
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, _| {
                if let Err(e) = result {
                    log::warn!("[Home] 回复点赞失败: {e}");
                    view.comment_digg_override.remove(&reply_id);
                }
            });
        })
        .detach();
        cx.notify();
    }
}
