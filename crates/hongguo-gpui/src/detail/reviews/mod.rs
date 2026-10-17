//! 剧评 tab：发评框（评分 + 表情 + 文本，Tauri ReviewComposer 同款）+
//! 评分摘要 + 分页列表（Tauri ReviewList 同款）。数据走
//! fetch_series_comments_page（group_type=1 剧级一条线，与播放器单集评论
//! 分属不同 group_id）；发送走 send_series_review（十分制 = 星 ×2）。

use std::sync::Arc;

use gpui_kit::Window;
use gpui_kit::component::WindowExt;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::notification::Notification;
use gpui_kit::prelude::*;
use hongguo_api::danmaku::fetch_series_comments_page;

use super::DetailView;
use crate::bridge;

mod view;

impl DetailView {
    /// 剧评首页（评分摘要取自首页 extra；换剧重置由 load 统一做）。
    pub(super) fn load_reviews_first_page(&mut self, cx: &mut Context<Self>) {
        if self.reviews_loading {
            return;
        }
        self.reviews_loading = true;
        let sid = self.series_id.clone();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            fetch_series_comments_page(&sid, "", &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.reviews_loading = false;
                match result {
                    Ok(page) => {
                        view.reviews_total = page.page.total;
                        view.reviews_score = page.score.clone();
                        view.reviews_score_cnt = page.score_cnt;
                        view.reviews_tag_stats = page.tag_stats.clone();
                        view.reviews_has_more = page.page.has_more;
                        view.reviews_cursor = page.page.next_cursor.clone();
                        view.reviews = Arc::new(page.page.items.clone());
                        for url in page.page.items.iter().map(|c| c.avatar.clone()) {
                            view.load_image(url, cx);
                        }
                        // 评分摘要变了要回头刷新头部统计行
                        cx.notify();
                    }
                    Err(e) => log::warn!("[Detail] 剧评拉取失败: {e}"),
                }
            });
        })
        .detach();
    }

    /// 剧评翻页（相邻页重叠条目按 comment_id 去重，首页评论同款）。
    pub(super) fn load_reviews_next(&mut self, cx: &mut Context<Self>) {
        if self.reviews_loading || !self.reviews_has_more {
            return;
        }
        self.reviews_loading = true;
        let sid = self.series_id.clone();
        let cursor = self.reviews_cursor.clone();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            fetch_series_comments_page(&sid, &cursor, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.reviews_loading = false;
                if let Ok(page) = result {
                    view.reviews_has_more = page.page.has_more;
                    view.reviews_cursor = page.page.next_cursor;
                    let mut items = (*view.reviews).clone();
                    for c in page.page.items {
                        if !items.iter().any(|x| x.comment_id == c.comment_id) {
                            view.load_image(c.avatar.clone(), cx);
                            items.push(c);
                        }
                    }
                    view.reviews = Arc::new(items);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 发表剧评（Tauri ReviewComposer 同款）：评分必选（十分制 = 星 ×2）、
    /// 正文非空；成功后回执 + 清空输入框 + 重拉首页让新剧评置顶。
    pub(super) fn submit_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = &self.review_input else {
            return;
        };
        let text = input.read(cx).value().trim().to_string();
        if text.is_empty() || self.review_stars == 0 || self.review_sending {
            return;
        }
        if !self.logged_in() {
            // 登录墙（Tauri 由后端拒绝 + 全局 toast；这里就地给回执）
            window.push_notification(Notification::error("登录后才能发表剧评"), cx);
            return;
        }
        let score = i64::from(self.review_stars) * 2;
        self.review_sending = true;
        cx.notify();
        let state = Arc::clone(&self.state);
        let sid = self.series_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::danmaku::send_series_review(&sid, &text, score, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.review_sending = false;
                match result {
                    Ok(_) => {
                        window.push_notification(
                            Notification::success(format!("剧评已发布（{score} 分）")),
                            cx,
                        );
                        // 评分保留、正文清空（Tauri setText('') 同款）
                        view.review_clear_input = true;
                        // 重拉首页置顶新剧评（Tauri invalidate 重取同款）
                        view.load_reviews_first_page(cx);
                    }
                    Err(e) => {
                        log::warn!("[Detail] 剧评发布失败: {e}");
                        window.push_notification(Notification::error(format!("发布失败：{e}")), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 当前登录用户 id（评论/剧评「删除」的 isMine 判据，空 = 未登录）。
    pub(super) fn my_user_id(&self) -> String {
        self.state
            .settings()
            .account
            .map(|a| a.user_id)
            .unwrap_or_default()
    }

    /// 剧评点赞（Tauri useReviewDigg 同款：本地乐观覆盖 + 计数 ±1，失败
    /// 回滚；端点是剧评形态 digg_review（object_type=2），与评论点赞不同）。
    pub(super) fn toggle_review_digg(&mut self, review_id: String, cx: &mut Context<Self>) {
        if !self.logged_in() {
            log::info!("[Detail] 剧评点赞需要登录，已忽略");
            return;
        }
        let Some(item) = self
            .reviews
            .iter()
            .find(|r| r.comment_id == review_id)
            .cloned()
        else {
            return;
        };
        let current = self
            .review_digg_override
            .get(&review_id)
            .copied()
            .unwrap_or(item.user_digg);
        let next = !current;
        let delta = if next { 1 } else { -1 };
        self.review_digg_override.insert(review_id.clone(), next);
        let mut items = (*self.reviews).clone();
        if let Some(hit) = items.iter_mut().find(|r| r.comment_id == review_id) {
            hit.digg_count += delta;
        }
        self.reviews = Arc::new(items);
        let state = Arc::clone(&self.state);
        let rid = review_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::interact::digg_review(&rid, next, &env)
                .await
                .map(|_| next)
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, _| {
                if let Err(e) = result {
                    log::warn!("[Detail] 剧评点赞失败: {e}");
                    view.review_digg_override.remove(&review_id);
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// 剧评回复点赞（评论形态端点——Tauri 实测口径：与评论点赞同端点，
    /// object_id=reply_id，不是剧评形态）。
    pub(super) fn toggle_review_reply_digg(
        &mut self,
        parent_review_id: String,
        reply_id: String,
        cx: &mut Context<Self>,
    ) {
        if !self.logged_in() {
            log::info!("[Detail] 回复点赞需要登录，已忽略");
            return;
        }
        let Some(reply) = self
            .review_replies
            .get(&parent_review_id)
            .and_then(|v| v.iter().find(|r| r.reply_id == reply_id))
            .cloned()
        else {
            return;
        };
        let current = self
            .review_digg_override
            .get(&reply_id)
            .copied()
            .unwrap_or(reply.user_digg);
        let next = !current;
        let delta = if next { 1 } else { -1 };
        self.review_digg_override.insert(reply_id.clone(), next);
        if let Some(list) = self.review_replies.get_mut(&parent_review_id)
            && let Some(items) = Arc::get_mut(list)
            && let Some(hit) = items.iter_mut().find(|r| r.reply_id == reply_id)
        {
            hit.digg_count += delta;
        }
        let state = Arc::clone(&self.state);
        let rid = reply_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::interact::digg_comment(&rid, next, &env)
                .await
                .map(|_| next)
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, _| {
                if let Err(e) = result {
                    log::warn!("[Detail] 剧评回复点赞失败: {e}");
                    view.review_digg_override.remove(&reply_id);
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// 展开/收起剧评回复（展开才拉首页，Tauri ReviewReplySection 同款）。
    pub(super) fn toggle_review_replies(
        &mut self,
        review_id: String,
        reply_count: i64,
        cx: &mut Context<Self>,
    ) {
        if self.review_replies_open.remove(&review_id) {
            cx.notify();
            return;
        }
        self.review_replies_open.insert(review_id.clone());
        cx.notify();
        if reply_count <= 0 || self.review_replies.contains_key(&review_id) {
            return;
        }
        self.fetch_review_replies(review_id, String::new(), cx);
    }

    /// 拉一页剧评回复（首页或翻页；游标原样续传，has_more=false 即到底）。
    pub(super) fn fetch_review_replies(
        &mut self,
        review_id: String,
        cursor: String,
        cx: &mut Context<Self>,
    ) {
        if self.review_replies_loading.contains(&review_id) {
            return;
        }
        self.review_replies_loading.insert(review_id.clone());
        cx.notify();
        let state = Arc::clone(&self.state);
        let sid = self.series_id.clone();
        let rid = review_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::danmaku::fetch_review_replies(&sid, &rid, &cursor, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.review_replies_loading.remove(&review_id);
                match result {
                    Ok(page) => {
                        for r in &page.items {
                            view.load_image(r.avatar.clone(), cx);
                        }
                        let mut list = view
                            .review_replies
                            .get(&review_id)
                            .map(|v| (**v).clone())
                            .unwrap_or_default();
                        for r in page.items {
                            if !list.iter().any(|x| x.reply_id == r.reply_id) {
                                list.push(r);
                            }
                        }
                        view.review_replies
                            .insert(review_id.clone(), Arc::new(list));
                        if page.has_more {
                            view.review_replies_cursor
                                .insert(review_id.clone(), page.next_cursor);
                        } else {
                            view.review_replies_cursor.remove(&review_id);
                        }
                    }
                    Err(e) => log::warn!("[Detail] 剧评回复拉取失败: {e}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 设/取消回复目标（行内输入框显隐；点「回复」再点一次 = 收起）。
    pub(super) fn set_review_reply_target(
        &mut self,
        review_id: String,
        to_reply: Option<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        if self
            .review_reply_target
            .as_ref()
            .is_some_and(|(rid, _)| rid == &review_id)
        {
            self.review_reply_target = None;
        } else {
            self.review_reply_target = Some((review_id, to_reply));
            self.review_reply_clear = self.review_reply_input.is_some();
        }
        cx.notify();
    }

    /// 发表剧评回复（剧评形态 reply/add commit_source=13；回复「回复」带
    /// reply_to_reply_id；登录墙与成败都走通知；成功后本地追加 + 父剧评
    /// replyCount+1 + 强制展开回复区——Tauri submitReply 同款）。
    pub(super) fn submit_review_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((review_id, to_reply)) = self.review_reply_target.clone() else {
            return;
        };
        let Some(input) = &self.review_reply_input else {
            return;
        };
        let text = input.read(cx).value().trim().to_string();
        if text.is_empty() || self.review_reply_sending {
            return;
        }
        if !self.logged_in() {
            window.push_notification(Notification::error("登录后才能回复"), cx);
            return;
        }
        self.review_reply_sending = true;
        cx.notify();
        let state = Arc::clone(&self.state);
        let sid = self.series_id.clone();
        let reply_to_reply_id = to_reply.as_ref().map(|(id, _)| id.clone());
        let rid = review_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::interact::send_review_reply(
                &sid,
                &rid,
                reply_to_reply_id.as_deref(),
                &text,
                &env,
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.review_reply_sending = false;
                match result {
                    Ok(reply) => {
                        let avatar = reply.avatar.clone();
                        let mut list = view
                            .review_replies
                            .get(&review_id)
                            .map(|v| (**v).clone())
                            .unwrap_or_default();
                        if !list.iter().any(|x| x.reply_id == reply.reply_id) {
                            list.push(reply);
                        }
                        view.review_replies
                            .insert(review_id.clone(), Arc::new(list));
                        // 父剧评 replyCount+1 且强制展开（新回复立刻可见）
                        let mut items = (*view.reviews).clone();
                        if let Some(hit) = items.iter_mut().find(|r| r.comment_id == review_id) {
                            hit.reply_count += 1;
                        }
                        view.reviews = Arc::new(items);
                        view.review_replies_open.insert(review_id.clone());
                        view.review_reply_target = None;
                        view.review_reply_clear = true;
                        view.load_image(avatar, cx);
                    }
                    Err(e) => {
                        log::warn!("[Detail] 剧评回复失败: {e}");
                        window.push_notification(Notification::error(format!("回复失败：{e}")), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 删除确认（剧评本体 service_id=2 / 回复 service_id=4；Tauri
    /// ConfirmDialog 同款语义，用本仓 AlertDialog）。
    pub(super) fn confirm_delete_review_item(
        &self,
        id: String,
        service_id: i64,
        parent: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let is_reply = parent.is_some();
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            let view = view.clone();
            let id = id.clone();
            let parent = parent.clone();
            dialog
                .title(if is_reply {
                    "删除这条回复？"
                } else {
                    "删除这条剧评？"
                })
                .description("删除后不可恢复。")
                .ok_text("删除")
                .cancel_text("取消")
                .show_cancel(true)
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _window, cx| {
                    view.update(cx, |view, cx| {
                        view.delete_review_item(id.clone(), service_id, parent.clone(), cx)
                    });
                    true
                })
        });
    }

    /// 执行删除：成功后本地摘除（不重取整列表，Tauri confirmDelete 同款）——
    /// 回复 → 回复列表摘除 + 父剧评 replyCount-1；剧评 → 列表摘除 +
    /// total-1 + 关掉展开/回复目标。
    fn delete_review_item(
        &mut self,
        id: String,
        service_id: i64,
        parent: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.review_deleting || !self.logged_in() {
            return;
        }
        self.review_deleting = true;
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn({
            let id = id.clone();
            async move {
                let env = state.api_env();
                hongguo_api::interact::delete_comment(&id, service_id, &env).await
            }
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.review_deleting = false;
                match result {
                    Ok(()) => {
                        if let Some(parent_id) = parent {
                            // 回复：回复缓存摘除 + 父剧评 replyCount-1
                            let mut list = view
                                .review_replies
                                .get(&parent_id)
                                .map(|v| (**v).clone())
                                .unwrap_or_default();
                            list.retain(|r| r.reply_id != id);
                            view.review_replies
                                .insert(parent_id.clone(), Arc::new(list));
                            let mut items = (*view.reviews).clone();
                            if let Some(hit) = items.iter_mut().find(|r| r.comment_id == parent_id)
                            {
                                hit.reply_count = (hit.reply_count - 1).max(0);
                            }
                            view.reviews = Arc::new(items);
                        } else {
                            let mut items = (*view.reviews).clone();
                            items.retain(|r| r.comment_id != id);
                            view.reviews = Arc::new(items);
                            view.reviews_total = (view.reviews_total - 1).max(0);
                            view.review_replies_open.remove(&id);
                            view.review_replies.remove(&id);
                            view.review_replies_cursor.remove(&id);
                            if view
                                .review_reply_target
                                .as_ref()
                                .is_some_and(|(rid, _)| rid == &id)
                            {
                                view.review_reply_target = None;
                            }
                        }
                    }
                    Err(e) => log::warn!("[Detail] 删除失败: {e}"),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
