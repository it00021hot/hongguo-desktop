//! 剧评 tab：发评框（评分 + 表情 + 文本，Tauri ReviewComposer 同款）+
//! 评分摘要 + 分页列表（Tauri ReviewList 同款）。数据走
//! fetch_series_comments_page（group_type=1 剧级一条线，与播放器单集评论
//! 分属不同 group_id）；发送走 send_series_review（十分制 = 星 ×2）。

use std::sync::Arc;

use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, ImageSource, ObjectFit, SharedString, Window, div, img};
use hongguo_api::danmaku::{ReplyItem, fetch_series_comments_page};

use super::DetailView;
use super::header::{hot_color, star_color};
use crate::bridge;

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
    fn my_user_id(&self) -> String {
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
    fn set_review_reply_target(
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
    fn confirm_delete_review_item(
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

    /// 剧评回复行（Tauri ReplyThread 剧评皮肤同款：头像 + 昵称 +
    /// 「回复 @x：」正文 + 时间/♥/回复/删除（自己的））。
    fn render_review_reply_row(
        &self,
        parent: &str,
        r: &ReplyItem,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let rid = r.reply_id.clone();
        let avatar = self.images.get(&r.avatar).cloned();
        let digged = self
            .review_digg_override
            .get(&rid)
            .copied()
            .unwrap_or(r.user_digg);
        let digg_count = r.digg_count;
        let mine = !self.my_user_id().is_empty() && r.user_id == self.my_user_id();
        let parent_id = parent.to_string();
        h_flex()
            .items_start()
            .gap_2()
            .child(
                div()
                    .size_6()
                    .flex_shrink_0()
                    .overflow_hidden()
                    .rounded_full()
                    .bg(theme.secondary)
                    .children(avatar.map(|bitmap| {
                        img(ImageSource::Render(bitmap))
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                    })),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().text_xs().text_color(theme.muted_foreground).child(
                        SharedString::from(if r.user_name.is_empty() {
                            "匿名用户".to_string()
                        } else {
                            r.user_name.clone()
                        }),
                    ))
                    .child(div().text_sm().child(SharedString::from(format!(
                        "回复 @{}：{}",
                        if r.reply_to_name.is_empty() {
                            "原评论".to_string()
                        } else {
                            r.reply_to_name.clone()
                        },
                        r.text
                    ))))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_3()
                            .text_xs()
                            .child(div().text_color(theme.muted_foreground).child(
                                SharedString::from(crate::home::fmt::relative_time(r.create_time)),
                            ))
                            .child(
                                h_flex()
                                    .id(ElementId::from(format!("reply-digg-{rid}")))
                                    .items_center()
                                    .gap_1()
                                    .cursor_pointer()
                                    .text_color(if digged {
                                        hot_color().into()
                                    } else {
                                        theme.muted_foreground
                                    })
                                    .hover(|s| s.text_color(theme.foreground))
                                    .on_click({
                                        let (p, rid) = (parent_id.clone(), rid.clone());
                                        cx.listener(move |this, _, _, cx| {
                                            this.toggle_review_reply_digg(
                                                p.clone(),
                                                rid.clone(),
                                                cx,
                                            );
                                        })
                                    })
                                    .child(Icon::new(gpui_kit::assets::IconName::Heart).size_3())
                                    .children((digg_count > 0).then(|| {
                                        div().child(SharedString::from(digg_count.to_string()))
                                    })),
                            )
                            .child({
                                let (p, rid, name) =
                                    (parent_id.clone(), rid.clone(), r.user_name.clone());
                                div()
                                    .id(ElementId::from(format!("reply-to-{rid}")))
                                    .cursor_pointer()
                                    .text_color(theme.muted_foreground)
                                    .hover(|s| s.text_color(theme.foreground))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.set_review_reply_target(
                                            p.clone(),
                                            Some((rid.clone(), name.clone())),
                                            cx,
                                        );
                                    }))
                                    .child("回复")
                            })
                            .children(mine.then(|| {
                                let (p, rid) = (parent_id.clone(), rid.clone());
                                div()
                                    .id(ElementId::from(format!("reply-del-{rid}")))
                                    .cursor_pointer()
                                    .text_color(theme.muted_foreground)
                                    .hover(|s| s.text_color(theme.danger))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.confirm_delete_review_item(
                                            rid.clone(),
                                            4,
                                            Some(p.clone()),
                                            window,
                                            cx,
                                        );
                                    }))
                                    .child("删除")
                            })),
                    ),
            )
            .into_any_element()
    }

    /// 发评框（Tauri ReviewComposer 同款：我的评分五星 + 表情 + 文本 +
    /// 「发表剧评」；未选评分或正文为空禁发；Enter 提交见 mod.rs 的订阅）。
    fn render_review_composer(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let shown = if self.review_hover_stars > 0 {
            self.review_hover_stars
        } else {
            self.review_stars
        };
        let has_text = self
            .review_input
            .as_ref()
            .is_some_and(|i| !i.read(cx).value().trim().is_empty());
        let can_send = has_text && self.review_stars >= 1 && !self.review_sending;
        let mut stars = h_flex().items_center().gap_2().child(
            div()
                .text_sm()
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .child("我的评分"),
        );
        for s in 1..=5u8 {
            stars = stars.child(
                div()
                    .id(ElementId::from(format!("review-star-{s}")))
                    .p_0p5()
                    .cursor_pointer()
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        let next = if *hovered {
                            s
                        } else if this.review_hover_stars == s {
                            0
                        } else {
                            this.review_hover_stars
                        };
                        if this.review_hover_stars != next {
                            this.review_hover_stars = next;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.review_stars = s;
                        this.review_hover_stars = 0;
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::Star)
                            .size_5()
                            .text_color(if s <= shown {
                                star_color().into()
                            } else {
                                theme.muted_foreground
                            }),
                    ),
            );
        }
        stars = stars.children((self.review_stars > 0).then(|| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child("已选评分，发布后生效")
        }));
        let mut card = v_flex()
            .rounded(gpui_kit::px(8.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.group_box)
            .p_3()
            .gap_2()
            .child(stars)
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("review-emoji-btn")
                            .px_1()
                            .cursor_pointer()
                            .text_color(if self.review_emoji_open {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            })
                            .hover(|s| s.text_color(theme.foreground))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.review_emoji_open = !this.review_emoji_open;
                                cx.notify();
                            }))
                            .child(Icon::new(gpui_kit::assets::IconName::FaceGrinning).size_4()),
                    )
                    .children(
                        self.review_input.as_ref().map(|input| {
                            Input::new(input).flex_1().h(gpui_kit::px(40.0)).text_sm()
                        }),
                    )
                    .child(
                        Button::new("review-send")
                            .label(if self.review_sending {
                                "发布中…"
                            } else {
                                "发表剧评"
                            })
                            .small()
                            .disabled(!can_send)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.submit_review(window, cx);
                            })),
                    ),
            );
        if self.review_emoji_open {
            // 表情网格（8 列，与评论面板同一套资产；点格子光标处插入
            // `[名字]`，连选不关）
            card = card.child(
                div()
                    .id("review-emoji-grid")
                    .max_h(gpui_kit::px(132.0))
                    .overflow_y_scroll()
                    .flex()
                    .flex_wrap()
                    .gap_0p5()
                    .children(crate::danmaku_emoji::EMOJI_FILES.iter().map(|(name, _)| {
                        let code = name.to_string();
                        let img = crate::danmaku_emoji::emoji_by_name(name);
                        let input = self.review_input.clone();
                        div()
                            .id(ElementId::from(format!("review-emoji-{code}")))
                            .size_7()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(gpui_kit::px(6.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.secondary_hover))
                            .on_click(move |_, window, cx| {
                                if let Some(input) = &input {
                                    input.update(cx, |state, cx| {
                                        state.insert(code.clone(), window, cx);
                                    });
                                }
                            })
                            .children(img.map(|img| {
                                gpui_kit::img(gpui_kit::ImageSource::Render(img))
                                    .size_5()
                                    .object_fit(gpui_kit::ObjectFit::Contain)
                            }))
                    })),
            );
        }
        card.into_any_element()
    }

    pub(super) fn render_reviews(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        v_flex()
            .mt_4()
            .gap_4()
            // 发评框（Tauri ReviewComposer 同位：列表在它下面）
            .child(self.render_review_composer(cx))
            // 评分摘要（剧均评分 + 评分人数；空 = 暂无评分整行不渲染）
            .children((!self.reviews_score.is_empty()).then(|| {
                h_flex()
                    .items_baseline()
                    .gap_2()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(gpui_kit::FontWeight::BOLD)
                            .text_color(gpui_kit::rgba(0xfacc15ff))
                            .child(format!(
                                "{:.1}",
                                self.reviews_score.parse::<f64>().unwrap_or(0.0)
                            )),
                    )
                    .child(div().text_sm().text_color(theme.muted_foreground).child(
                        if self.reviews_score_cnt > 0 {
                            format!("{}人评分", DetailView::fmt_count(self.reviews_score_cnt))
                        } else {
                            "剧均评分".to_string()
                        },
                    ))
            }))
            // 剧评列表（Tauri ReviewList 同款：头像 + 昵称 + 评分 + 时间 +
            // 正文 + 点赞/回复/删除 + 回复线程（懒加载）与行内回复框；
            // 评分行只有剧评有——单集评论 score 恒空）
            .children(
                self.reviews
                    .iter()
                    .map(|c| {
                        let avatar = self.images.get(&c.avatar).cloned();
                        let rid = c.comment_id.clone();
                        let is_mine =
                            !self.my_user_id().is_empty() && c.user_id == self.my_user_id();
                        let digged = self
                            .review_digg_override
                            .get(&rid)
                            .copied()
                            .unwrap_or(c.user_digg);
                        let digg_count = c.digg_count;
                        let reply_count = c.reply_count;
                        let replies_open = self.review_replies_open.contains(&rid);
                        let replies = self
                            .review_replies
                            .get(&rid)
                            .map(|v| (**v).clone())
                            .unwrap_or_default();
                        let loading = self.review_replies_loading.contains(&rid);
                        let has_more = self.review_replies_cursor.contains_key(&rid);
                        let reply_target_open = self
                            .review_reply_target
                            .as_ref()
                            .is_some_and(|(r, _)| r == &rid);
                        let reply_to_reply = self
                            .review_reply_target
                            .as_ref()
                            .filter(|(r, _)| r == &rid)
                            .and_then(|(_, t)| t.clone());
                        let reply_input = self.review_reply_input.clone();
                        v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .items_start()
                            .gap_3()
                            .child(
                                div()
                                    .size_9()
                                    .flex_shrink_0()
                                    .overflow_hidden()
                                    .rounded_full()
                                    .bg(theme.secondary)
                                    .children(avatar.map(|bitmap| {
                                        img(ImageSource::Render(bitmap))
                                            .size_full()
                                            .object_fit(ObjectFit::Cover)
                                    })),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                                            .child(SharedString::from(c.user_name.clone())),
                                    )
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .children((!c.score.is_empty()).then(|| {
                                                div()
                                                    .px_1p5()
                                                    .rounded(gpui_kit::px(4.0))
                                                    .text_xs()
                                                    .bg(gpui_kit::rgba(0xfacc1526))
                                                    .text_color(gpui_kit::rgba(0xfacc15ff))
                                                    .child(SharedString::from(format!(
                                                        "{}分",
                                                        c.score
                                                    )))
                                            }))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(theme.muted_foreground)
                                                    .child(SharedString::from(
                                                        crate::home::fmt::relative_time(
                                                            c.create_time,
                                                        ),
                                                    )),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .line_height(gpui_kit::relative(1.6))
                                            .child(render_review_text(&c.text)),
                                    )
                                    // 动作行（Tauri ReviewList 同款：♥点赞 /
                                    // 回复 / 删除（自己的）/ 展开回复）
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_4()
                                            .text_xs()
                                            .child(
                                                h_flex()
                                                    .id(ElementId::from(format!(
                                                        "review-digg-{rid}"
                                                    )))
                                                    .items_center()
                                                    .gap_1()
                                                    .cursor_pointer()
                                                    .text_color(if digged {
                                                        hot_color().into()
                                                    } else {
                                                        theme.muted_foreground
                                                    })
                                                    .hover(|s| s.text_color(theme.foreground))
                                                    .on_click({
                                                        let rid = rid.clone();
                                                        cx.listener(move |this, _, _, cx| {
                                                            this.toggle_review_digg(
                                                                rid.clone(),
                                                                cx,
                                                            );
                                                        })
                                                    })
                                                    .child(
                                                        Icon::new(
                                                            gpui_kit::assets::IconName::Heart,
                                                        )
                                                        .size_3p5(),
                                                    )
                                                    .children((digg_count > 0).then(|| {
                                                        div().child(SharedString::from(
                                                            digg_count.to_string(),
                                                        ))
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .id(ElementId::from(format!(
                                                        "review-reply-{rid}"
                                                    )))
                                                    .cursor_pointer()
                                                    .text_color(theme.muted_foreground)
                                                    .hover(|s| s.text_color(theme.foreground))
                                                    .on_click({
                                                        let rid = rid.clone();
                                                        cx.listener(move |this, _, _, cx| {
                                                            this.set_review_reply_target(
                                                                rid.clone(),
                                                                None,
                                                                cx,
                                                            );
                                                        })
                                                    })
                                                    .child("回复"),
                                            )
                                            .children(is_mine.then(|| {
                                                let rid = rid.clone();
                                                div()
                                                    .id(ElementId::from(format!(
                                                        "review-del-{rid}"
                                                    )))
                                                    .cursor_pointer()
                                                    .text_color(theme.muted_foreground)
                                                    .hover(|s| s.text_color(theme.danger))
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.confirm_delete_review_item(
                                                                rid.clone(),
                                                                2,
                                                                None,
                                                                window,
                                                                cx,
                                                            );
                                                        },
                                                    ))
                                                    .child("删除")
                                            }))
                                            .children((reply_count > 0).then(|| {
                                                let rid = rid.clone();
                                                div()
                                                    .id(ElementId::from(format!(
                                                        "review-replies-toggle-{rid}"
                                                    )))
                                                    .cursor_pointer()
                                                    .text_color(theme.muted_foreground)
                                                    .hover(|s| s.text_color(theme.foreground))
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.toggle_review_replies(
                                                                rid.clone(),
                                                                reply_count,
                                                                cx,
                                                            );
                                                        },
                                                    ))
                                                    .child(if replies_open {
                                                        "收起回复".to_string()
                                                    } else {
                                                        format!("展开 {reply_count} 条回复")
                                                    })
                                            })),
                                    ),
                            ),
                    )
                    // 回复线程（懒加载；行 = 头像/昵称/正文/♥/回复/删除）
                    .children(replies_open.then(|| {
                        let mut col = v_flex().pl_12().gap_2();
                        if loading && replies.is_empty() {
                            col = col.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child("加载中…"),
                            );
                        }
                        for r in &replies {
                            col = col.child(self.render_review_reply_row(&rid, r, cx));
                        }
                        if has_more {
                            col = col.child(
                                div()
                                    .id(ElementId::from(format!("review-more-{rid}")))
                                    .text_xs()
                                    .cursor_pointer()
                                    .text_color(theme.muted_foreground)
                                    .hover(|s| s.text_color(theme.foreground))
                                    .child(if loading {
                                        "加载中…"
                                    } else {
                                        "展开更多回复"
                                    })
                                    .on_click({
                                        let rid = rid.clone();
                                        let cursor = self
                                            .review_replies_cursor
                                            .get(&rid)
                                            .cloned()
                                            .unwrap_or_default();
                                        cx.listener(move |this, _, _, cx| {
                                            this.fetch_review_replies(
                                                rid.clone(),
                                                cursor.clone(),
                                                cx,
                                            );
                                        })
                                    }),
                            );
                        }
                        col
                    }))
                    // 行内回复框（Tauri replyTarget 同位：mt-2 小输入 + 回复）
                    .children((reply_target_open && reply_input.is_some()).then(|| {
                        let input = reply_input.clone().unwrap();
                        h_flex()
                            .pl_12()
                            .mt_1()
                            .items_center()
                            .gap_2()
                            .child(
                                Input::new(&input)
                                    .flex_1()
                                    .h(gpui_kit::px(32.0))
                                    .text_xs(),
                            )
                            .child(
                                Button::new(ElementId::from(format!(
                                    "review-reply-send-{rid}"
                                )))
                                .label(if self.review_reply_sending {
                                    "发送中…"
                                } else {
                                    "回复"
                                })
                                .small()
                                .disabled(
                                    input.read(cx).value().trim().is_empty()
                                        || self.review_reply_sending,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.submit_review_reply(window, cx);
                                })),
                            )
                            .child(
                                Button::new(ElementId::from(format!(
                                    "review-reply-cancel-{rid}"
                                )))
                                .label("取消")
                                .ghost()
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.review_reply_target = None;
                                    cx.notify();
                                })),
                            )
                            .children(reply_to_reply.as_ref().map(|(_, name)| {
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(format!("回复 @{name}")))
                            }))
                    }))
                    .into_any_element()
                    })
                    .collect::<Vec<_>>(),
            )
            .children(self.reviews.is_empty().then(|| {
                div()
                    .py_6()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(if self.reviews_loading {
                        "加载中…"
                    } else {
                        "暂无剧评"
                    })
            }))
            // 加载更多（Tauri 是滚动懒加载；GPUI 详情页滚动容器裁判复杂，
            // 按钮等价且省一档哨兵）
            .children(
                (self.reviews_has_more && !self.reviews.is_empty()).then(|| {
                    Button::new("detail-reviews-more")
                        .label(if self.reviews_loading {
                            "加载中…"
                        } else {
                            "加载更多剧评"
                        })
                        .secondary()
                        .small()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.load_reviews_next(cx);
                        }))
                }),
            )
            .into_any_element()
    }
}

/// 剧评正文内联渲染（`[名字]` 表情分段，与弹幕/评论共用一套 parse；
/// 剧评无 @提及形态）。
fn render_review_text(text: &str) -> gpui_kit::AnyElement {
    let mut row = div().flex().flex_wrap().items_center();
    for seg in crate::danmaku_emoji::parse_emoji_segments(text) {
        row = row.child(match seg {
            crate::danmaku_emoji::EmojiSegment::Text(t) => {
                div().child(SharedString::from(t)).into_any_element()
            }
            crate::danmaku_emoji::EmojiSegment::Emoji(img) => {
                gpui_kit::img(gpui_kit::ImageSource::Render(img))
                    .size(gpui_kit::px(18.0))
                    .object_fit(gpui_kit::ObjectFit::Contain)
                    .into_any_element()
            }
        });
    }
    row.into_any_element()
}
