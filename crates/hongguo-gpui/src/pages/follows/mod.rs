//! 追更页（Tauri `/follows` = `follows-page.tsx` 的 GPUI 对齐）。
//!
//! 「追更的剧 / 更新提醒」双 tab：
//! - 追更的剧：服务端跟随列表游标翻页（`relation/get_user_action`
//!   action_channel=9），行卡 = 封面 + 标题 + 状态行「已追平 / N 集未看 ·
//!   连载中/已完结」+ 集数进度 + 播放 / 取消追更；管理模式批量取消
//!   （逐条 do_action 35，服务端无批量端点，Tauri 同口径）。
//! - 更新提醒：本地通知（追更更新 / 预约上线），点条目跳播放并标已读；
//!   「检查更新」手动触发一轮（20 分钟定时器已在 service 跑，按钮是立即
//!   回音入口）；「弹出提醒」开关落 `ui_prefs`（Tauri zustand 持久化同义）。
//!
//! 追更必须登录（服务端跟随列表），未登录给登录入口（事件交壳开登录面板）。
//!
//! 数据与操作走 `hongguo_service::follow_service`——与 Tauri 的 follow_cmd
//! 同一段服务面（登录墙 / 基线维护 / 列表翻页都在 service，壳只做 UI）。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariant};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{RenderImage, Window, div};
use hongguo_api::interact::FollowUpdateItem;
use hongguo_common::model::FollowNotice;
use hongguo_service::app_state::AppStateInner;

use crate::bridge;
use crate::detail::DetailPrefill;
use crate::pages::list_common::{BatchBar, decode_cover};

mod view;

/// 追更页发往壳的事件（壳订阅后切目的地/起播/开登录面板/刷角标）。
#[derive(Debug, Clone)]
pub(crate) enum FollowsEvent {
    Play {
        series_id: String,
        vid_index: u32,
        start_secs: f64,
    },
    OpenDetail {
        series_id: String,
        prefill: DetailPrefill,
    },
    /// 未登录态点「登录」：切回首页并开登录面板（Tauri 弹 LoginDialog 同位）。
    Login,
    /// 通知未读状态变化：壳据此刷新侧栏未读角标。
    NoticesChanged,
}

/// 页面双 tab（Tauri 会话级保留 tab；GPUI 视图实体常驻，天然保留）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Follows,
    Notices,
}

pub(crate) struct FollowsView {
    state: Arc<AppStateInner>,
    tab: Tab,
    // —— 追更的剧 ——
    items: Vec<FollowUpdateItem>,
    cursor: String,
    has_more: bool,
    loading: bool,
    loading_more: bool,
    failed: Option<String>,
    covers: HashMap<String, Arc<RenderImage>>,
    covers_pending: HashSet<String>,
    managing: bool,
    selected: BTreeSet<String>,
    /// 单条取消追更进行中（按钮转圈/禁用）。
    cancelling: HashSet<String>,
    /// 批量取消进行中（吸底条转圈）。
    batch_cancelling: bool,
    // —— 更新提醒 ——
    notices: Vec<FollowNotice>,
    /// 弹出开关（`ui_prefs` 持久化；Tauri aibotPopupOn 同款）。
    popup_on: bool,
    checking: bool,
    clearing: bool,
    /// 「点过条目即全部视为已读」的本地标记（Tauri readAll 同款近似：
    /// 服务端只落单条已读，列表视觉一次翻面）。
    read_all: bool,
    /// 预览种子用的登录态开关（真实壳恒 false，以 settings 的 account 为准）。
    preview_logged_in: bool,
}

impl FollowsView {
    pub(crate) fn new(
        state: Arc<AppStateInner>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Self {
        let popup_on = crate::ui_prefs::load().aibot_popup_on;
        Self {
            state,
            tab: Tab::Follows,
            items: Vec::new(),
            cursor: String::new(),
            has_more: false,
            loading: false,
            loading_more: false,
            failed: None,
            covers: HashMap::new(),
            covers_pending: HashSet::new(),
            managing: false,
            selected: BTreeSet::new(),
            cancelling: HashSet::new(),
            batch_cancelling: false,
            notices: Vec::new(),
            popup_on,
            checking: false,
            clearing: false,
            read_all: false,
            preview_logged_in: false,
        }
    }

    fn logged_in(&self) -> bool {
        self.state.settings().account.is_some() || self.preview_logged_in
    }

    /// 进入页面：拉列表首页 + 刷新本地通知（Tauri staleTime 0 的重挂载语义）。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        if !self.logged_in() {
            self.items.clear();
            self.notices.clear();
            self.selected.clear();
            self.managing = false;
            self.failed = None;
            self.loading = false;
            self.has_more = false;
            self.cursor.clear();
            cx.notify();
            return;
        }
        self.refresh_notices(cx);
        if self.loading || self.loading_more {
            return;
        }
        self.failed = None;
        if self.items.is_empty() {
            self.loading = true;
        }
        cx.notify();
        let state = Arc::clone(&self.state);
        let rx =
            bridge::spawn(async move { hongguo_service::follow_service::list(&state, "").await });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            this.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok(page) => {
                        log::info!("[Follows] GPUI 壳追更列表 -> {} 条", page.items.len());
                        view.items = page.items;
                        view.cursor = page.cursor;
                        view.has_more = page.has_more;
                        view.load_covers(cx);
                    }
                    Err(e) => {
                        log::warn!("[Follows] GPUI 壳列表失败: {e}");
                        if view.items.is_empty() {
                            view.failed = Some(format!("{e}"));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 翻页（游标由服务端下发原样续传，首页为空串）。
    pub(super) fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.loading_more || self.loading || !self.has_more || self.cursor.is_empty() {
            return;
        }
        self.loading_more = true;
        cx.notify();
        let state = Arc::clone(&self.state);
        let cursor = self.cursor.clone();
        let rx =
            bridge::spawn(
                async move { hongguo_service::follow_service::list(&state, &cursor).await },
            );
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.loading_more = false;
                match result {
                    Ok(page) => {
                        view.items.extend(page.items);
                        view.cursor = page.cursor;
                        view.has_more = page.has_more;
                        view.load_covers(cx);
                    }
                    Err(e) => {
                        log::warn!("[Follows] GPUI 壳翻页失败: {e}");
                        window.push_notification(Notification::error(format!("加载失败：{e}")), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 逐条取封面（条目自带 CDN 签名 URL，HEIC 走同一封面阶梯）。
    fn load_covers(&mut self, cx: &mut Context<Self>) {
        for item in &self.items {
            let key = item.series_id.clone();
            if item.cover.is_empty() || !self.covers_pending.insert(key.clone()) {
                continue;
            }
            let rx = crate::cover_img::fetch(item.cover.clone());
            cx.spawn(async move |this, cx| {
                let bytes = rx.await.expect("tokio 桥不会丢弃发送端");
                this.update(cx, |view, cx| {
                    view.covers_pending.remove(&key);
                    if let Ok(bytes) = bytes
                        && let Some(image) = decode_cover(&bytes)
                    {
                        view.covers.insert(key, image);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    /// 单条取消追更（行卡上的「取消追更」；Tauri FollowButton 同位）。
    pub(super) fn cancel_follow(&mut self, series_id: String, cx: &mut Context<Self>) {
        if !self.cancelling.insert(series_id.clone()) {
            return;
        }
        cx.notify();
        let state = Arc::clone(&self.state);
        let sid = series_id.clone();
        let rx = bridge::spawn(async move {
            hongguo_service::follow_service::toggle(&state, &sid, false).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.cancelling.remove(&series_id);
                match result {
                    Ok(()) => {
                        window.push_notification(Notification::success("已取消追更"), cx);
                        view.load(cx);
                    }
                    Err(e) => {
                        log::warn!("[Follows] 取消追更失败: {e}");
                        window.push_notification(Notification::error(format!("操作失败：{e}")), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 批量取消确认（Tauri BatchBar 的确认框同位：危险动作 + 明确对象数）。
    fn confirm_batch_cancel(&self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let count = self.selected.len();
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            let view = view.clone();
            dialog
                .title("批量取消追更？")
                .description(format!("将删除选中的 {count} 项，删除后不可恢复。"))
                .ok_text("确定")
                .cancel_text("取消")
                .show_cancel(true)
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _window, cx| {
                    view.update(cx, |view, cx| view.batch_cancel(cx));
                    true
                })
        });
    }

    /// 批量取消（服务端无批量端点：逐条 do_action 35，成功一项记一项，
    /// 全部成功才退出管理模式——Tauri cancelPicked 同口径）。
    fn batch_cancel(&mut self, cx: &mut Context<Self>) {
        if self.batch_cancelling {
            return;
        }
        let ids: Vec<String> = self
            .items
            .iter()
            .filter(|i| self.selected.contains(&i.series_id))
            .map(|i| i.series_id.clone())
            .collect();
        if ids.is_empty() {
            return;
        }
        self.batch_cancelling = true;
        cx.notify();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let mut done = 0usize;
            let mut failed = 0usize;
            let mut last_err = String::new();
            for sid in ids {
                match hongguo_service::follow_service::toggle(&state, &sid, false).await {
                    Ok(()) => done += 1,
                    Err(e) => {
                        failed += 1;
                        last_err = format!("{e}");
                    }
                }
            }
            (done, failed, last_err)
        });
        cx.spawn(async move |this, cx| {
            let (done, failed, last_err) = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.batch_cancelling = false;
                if failed == 0 {
                    window.push_notification(
                        Notification::success(format!("已取消 {done} 部剧的追更")),
                        cx,
                    );
                    view.managing = false;
                    view.selected.clear();
                } else {
                    window.push_notification(
                        Notification::error(format!("部分取消失败：{last_err}")),
                        cx,
                    );
                }
                view.load(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// 手动检查更新（未登录时按钮不出现；失败给回音，不静默）。
    pub(super) fn check_updates(&mut self, cx: &mut Context<Self>) {
        if self.checking || !self.logged_in() {
            return;
        }
        self.checking = true;
        cx.notify();
        let state = Arc::clone(&self.state);
        let rx =
            bridge::spawn(
                async move { hongguo_service::follow_service::check_updates(&state).await },
            );
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.checking = false;
                match result {
                    Ok(0) => {
                        window.push_notification(Notification::success("暂时没有新的更新"), cx)
                    }
                    Ok(n) => window
                        .push_notification(Notification::success(format!("有 {n} 条新提醒")), cx),
                    Err(e) => {
                        log::warn!("[Follows] 检查更新失败: {e}");
                        window.push_notification(Notification::error(format!("检查失败：{e}")), cx);
                    }
                }
                view.refresh_notices(cx);
                cx.emit(FollowsEvent::NoticesChanged);
                cx.notify();
            });
        })
        .detach();
    }

    /// 清空提醒（Tauri 无二次确认，直接清 + 回执）。
    pub(super) fn clear_notices(&mut self, cx: &mut Context<Self>) {
        if self.clearing || !self.logged_in() {
            return;
        }
        self.clearing = true;
        cx.notify();
        let state = Arc::clone(&self.state);
        let rx =
            bridge::spawn(async move { hongguo_service::follow_service::clear_notices(&state) });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.clearing = false;
                match result {
                    Ok(()) => {
                        view.read_all = false;
                        window.push_notification(Notification::success("已清空"), cx);
                        view.refresh_notices(cx);
                        cx.emit(FollowsEvent::NoticesChanged);
                    }
                    Err(e) => {
                        log::warn!("[Follows] 清空提醒失败: {e}");
                        window.push_notification(Notification::error(format!("清空失败：{e}")), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 点一条提醒：标已读 + 跳播放（Tauri open 同序：先读后播）。
    pub(super) fn open_notice(&mut self, notice: &FollowNotice, cx: &mut Context<Self>) {
        if !notice.read {
            if let Err(e) =
                hongguo_service::follow_service::mark_notices_read(&self.state, &[notice.id])
            {
                log::warn!("[Follows] 标已读失败: {e}");
            }
            self.read_all = true;
            self.refresh_notices(cx);
            cx.emit(FollowsEvent::NoticesChanged);
        }
        self.play_series(&notice.series_id, cx);
    }

    /// 刷新本地通知列表（同步读 store：follow_notice 是小表，进页面/变更时读）。
    fn refresh_notices(&mut self, cx: &mut Context<Self>) {
        match hongguo_service::follow_service::notices(&self.state) {
            Ok(list) => self.notices = list,
            Err(e) => log::warn!("[Follows] 通知读取失败: {e}"),
        }
        cx.notify();
    }

    /// 服务事件（新提醒到达）：壳转发进来刷新列表与角标。
    pub(crate) fn on_notices_arrived(&mut self, cx: &mut Context<Self>) {
        if self.logged_in() {
            self.refresh_notices(cx);
        }
    }

    /// 弹出开关翻转：更新内存 + 落盘（`ui_prefs` 读-改-写）。
    pub(super) fn set_popup_on(&mut self, on: bool, cx: &mut Context<Self>) {
        self.popup_on = on;
        let mut prefs = crate::ui_prefs::load();
        prefs.aibot_popup_on = on;
        crate::ui_prefs::save(prefs);
        cx.notify();
    }

    pub(super) fn unread_count(&self) -> usize {
        if self.read_all {
            return 0;
        }
        self.notices.iter().filter(|n| !n.read).count()
    }

    pub(super) fn open_detail(&self, item: &FollowUpdateItem, cx: &mut Context<Self>) {
        cx.emit(FollowsEvent::OpenDetail {
            series_id: item.series_id.clone(),
            prefill: DetailPrefill {
                title: item.title.clone(),
                cover: item.cover.clone(),
                tags: Vec::new(),
                desc: String::new(),
                reserved: false,
            },
        });
    }

    pub(super) fn play_series(&self, series_id: &str, cx: &mut Context<Self>) {
        let (vid_index, start_secs) =
            hongguo_service::play_service::resume::resolve_entry(&self.state.store, series_id, &[])
                .ok()
                .unwrap_or((1, 0.0));
        cx.emit(FollowsEvent::Play {
            series_id: series_id.to_string(),
            vid_index,
            start_secs,
        });
    }

    pub(super) fn toggle_selected(&mut self, series_id: &str, cx: &mut Context<Self>) {
        if !self.selected.remove(series_id) {
            self.selected.insert(series_id.to_string());
        }
        cx.notify();
    }
}

impl gpui_kit::EventEmitter<FollowsEvent> for FollowsView {}

impl gpui_kit::Render for FollowsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let logged_in = self.logged_in();
        let body = if !logged_in {
            let theme = cx.theme();
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_3()
                .child(
                    gpui_kit::component::Icon::new(gpui_kit::assets::IconName::LogIn)
                        .with_size(gpui_kit::px(32.0))
                        .text_color(theme.muted_foreground),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("登录后才能追更"),
                )
                .child(
                    Button::new("follows-login")
                        .icon(gpui_kit::assets::IconName::LogIn)
                        .label("登录")
                        .small()
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.emit(FollowsEvent::Login);
                        })),
                )
                .into_any_element()
        } else {
            match self.tab {
                Tab::Follows => self.render_follows_body(cx),
                Tab::Notices => self.render_notices_body(cx),
            }
        };
        let toolbar = logged_in.then(|| self.render_tabs(cx));
        let manage_row =
            (logged_in && self.tab == Tab::Follows).then(|| self.render_manage_row(cx));
        let batch_bar = (logged_in && self.tab == Tab::Follows && self.managing).then(|| {
            let total = self.items.len();
            BatchBar::new(
                self.selected.len(),
                total,
                self.batch_cancelling,
                "取消追更",
            )
            .on_toggle_all(cx.listener(|this, _, _, cx| {
                let total = this.items.len();
                if total > 0 && this.selected.len() >= total {
                    this.selected.clear();
                } else {
                    for item in &this.items {
                        this.selected.insert(item.series_id.clone());
                    }
                }
                cx.notify();
            }))
            .on_delete(cx.listener(|this, _, window, cx| {
                this.confirm_batch_cancel(window, cx);
            }))
            .into_any_element()
        });
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .id("follows-page")
            .children(toolbar.map(|t| div().w_full().px_4().pt_4().child(t)))
            .children(manage_row.map(|t| div().w_full().px_4().pt_3().child(t)))
            .child(body)
            .children(batch_bar)
    }
}

#[cfg(feature = "screenshot")]
impl FollowsView {
    /// 预览种子（screenshot feature）：灌假追更列表/通知，跳过网络。
    /// `follows` = 双条列表；`follows-empty` = 空态；`follows-manage` =
    /// 管理模式（前两条已选）；`follows-notices` = 更新提醒 tab（含未读）。
    pub(crate) fn debug_preview_seed(&mut self, scene: &str, cx: &mut Context<Self>) {
        self.preview_logged_in = true;
        self.items = if scene == "follows-empty" {
            Vec::new()
        } else {
            crate::preview::fixtures::follow_items()
        };
        for (i, item) in self.items.iter().enumerate() {
            self.covers
                .insert(item.series_id.clone(), crate::preview::fixtures::cover(i));
        }
        self.notices = crate::preview::fixtures::follow_notices();
        self.has_more = self.items.len() > 3;
        if scene == "follows-manage" {
            self.managing = true;
            self.selected = self
                .items
                .iter()
                .take(2)
                .map(|i| i.series_id.clone())
                .collect();
        }
        if scene == "follows-notices" {
            self.tab = Tab::Notices;
        }
        cx.notify();
    }
}
