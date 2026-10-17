//! 我的预约页（Tauri `/reservations` = `reservation-page.tsx` 的 GPUI 对齐）。
//!
//! 「已上线/待上线」两个 tab（带计数）+ 搜索 + 管理批量删除；行卡片 =
//! 封面（左下上线日期角标）+ 标题/评分/简介/标签/上线时间 + 动作列
//! （已上线 = 播放 + 详情；未上线 = 取消预约）。未登录给登录入口。
//! 数据与操作走 `hongguo_api::reservation`（fetch_reservations /
//! reserve_series / delete_reservations），与 Tauri 的预约 command 同一段。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::component::Disableable;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ElementId, Entity, ImageSource, ObjectFit, RenderImage, SharedString, Window, div, img,
};
use hongguo_api::calendar::CalendarItem;
use hongguo_service::app_state::AppStateInner;

use crate::bridge;
use crate::detail::DetailPrefill;
use crate::pages::list_common::{BatchBar, decode_cover, pick_dot, search_box};

/// 预约页发往壳的事件（壳订阅后切目的地/起播/开详情/开登录面板）。
#[derive(Debug, Clone)]
pub(crate) enum ReservationEvent {
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
}

/// 一个 tab 的数据面（两条查询各自独立加载/失败，Tauri 双 query 同构）。
#[derive(Default)]
struct TabData {
    items: Option<Vec<CalendarItem>>,
    total: i64,
    loading: bool,
    failed: Option<String>,
}

pub(crate) struct ReservationView {
    state: Arc<AppStateInner>,
    /// true = 已上线 tab。
    online_tab: bool,
    online: TabData,
    offline: TabData,
    covers: HashMap<String, Arc<RenderImage>>,
    covers_pending: HashSet<String>,
    search: Entity<InputState>,
    managing: bool,
    selected: BTreeSet<String>,
    deleting: bool,
    /// 单条取消预约进行中（按钮转圈/禁用）。
    cancelling: HashSet<String>,
    /// 预览种子用的登录态开关（真实壳恒 false，以 settings 的 account 为准）。
    preview_logged_in: bool,
}

impl ReservationView {
    pub(crate) fn new(
        state: Arc<AppStateInner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索预约的剧"));
        cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        Self {
            state,
            online_tab: true,
            online: TabData::default(),
            offline: TabData::default(),
            covers: HashMap::new(),
            covers_pending: HashSet::new(),
            search,
            managing: false,
            selected: BTreeSet::new(),
            deleting: false,
            cancelling: HashSet::new(),
            preview_logged_in: false,
        }
    }

    /// 拉两个 tab 的预约列表（角标计数两条都要，Tauri 双 query 同款）。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        let account = self.state.settings().account.clone();
        if account.is_none() {
            // 未登录：不拉（接口匿名只给空表），页面走登录入口
            self.online = TabData::default();
            self.offline = TabData::default();
            cx.notify();
            return;
        }
        if self.online.loading || self.offline.loading {
            return;
        }
        for online in [true, false] {
            let tab = if online {
                &mut self.online
            } else {
                &mut self.offline
            };
            if tab.items.is_none() {
                tab.loading = true;
            }
            tab.failed = None;
        }
        cx.notify();
        for online in [true, false] {
            let state = Arc::clone(&self.state);
            let rx = bridge::spawn(async move {
                let env = state.api_env();
                hongguo_api::reservation::fetch_reservations(online, &env).await
            });
            cx.spawn(async move |this, cx| {
                let result = rx.await.expect("tokio 桥不会丢弃发送端");
                this.update(cx, |view, cx| {
                    let tab = if online {
                        &mut view.online
                    } else {
                        &mut view.offline
                    };
                    tab.loading = false;
                    match result {
                        Ok(page) => {
                            log::info!(
                                "[Reservations] GPUI 壳 online={online} -> {} 条",
                                page.items.len()
                            );
                            tab.total = if online {
                                page.online_total
                            } else {
                                page.offline_total
                            }
                            .max(page.items.len() as i64);
                            if online {
                                view.online.items = Some(page.items.clone());
                            } else {
                                view.offline.items = Some(page.items.clone());
                            }
                            view.load_covers(&page.items, cx);
                        }
                        Err(e) => {
                            log::warn!("[Reservations] GPUI 壳失败 online={online}: {e}");
                            if tab.items.is_none() {
                                tab.failed = Some(format!("{e}"));
                            }
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    /// 逐条取封面（条目自带上线封面 URL）。
    fn load_covers(&mut self, items: &[CalendarItem], cx: &mut Context<Self>) {
        for item in items {
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

    /// 单条取消预约（未上线卡片的行尾动作）。
    fn cancel_reserve(&mut self, series_id: String, cx: &mut Context<Self>) {
        if !self.cancelling.insert(series_id.clone()) {
            return;
        }
        cx.notify();
        let state = Arc::clone(&self.state);
        let sid = series_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::reservation::reserve_series(&sid, false, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.cancelling.remove(&series_id);
                match result {
                    Ok(()) => {
                        window.push_notification(Notification::success("已取消预约"), cx);
                        view.load(cx);
                    }
                    Err(e) => {
                        log::warn!("[Reservations] 取消预约失败: {e}");
                        window.push_notification(Notification::error(format!("取消失败：{e}")), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 批量删除预约（一个请求，Tauri useReservationsDelete 同款）。
    fn batch_delete(&mut self, cx: &mut Context<Self>) {
        if self.deleting {
            return;
        }
        let ids: Vec<String> = self
            .current_items()
            .iter()
            .filter(|i| self.selected.contains(&i.series_id))
            .map(|i| i.series_id.clone())
            .collect();
        if ids.is_empty() {
            return;
        }
        self.deleting = true;
        cx.notify();
        let total = ids.len();
        let online = self.online_tab;
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::reservation::delete_reservations(&ids, false, &[], online, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.deleting = false;
                match result {
                    Ok(()) => {
                        window.push_notification(
                            Notification::success(format!("已删除 {total} 项")),
                            cx,
                        );
                        view.managing = false;
                        view.selected.clear();
                    }
                    Err(e) => {
                        log::warn!("[Reservations] 批量删除失败: {e}");
                        window.push_notification(
                            Notification::error(format!("部分删除失败：{e}")),
                            cx,
                        );
                    }
                }
                view.load(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn confirm_batch_delete(&self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let count = self.selected.len();
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            let view = view.clone();
            dialog
                .title("批量删除预约？")
                .description(format!("将删除选中的 {count} 项，删除后不可恢复。"))
                .ok_text("确定")
                .cancel_text("取消")
                .show_cancel(true)
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _window, cx| {
                    view.update(cx, |view, cx| view.batch_delete(cx));
                    true
                })
        });
    }

    fn logged_in(&self) -> bool {
        self.state.settings().account.is_some() || self.preview_logged_in
    }

    fn current(&self) -> &TabData {
        if self.online_tab {
            &self.online
        } else {
            &self.offline
        }
    }

    fn current_items(&self) -> Vec<CalendarItem> {
        self.current().items.clone().unwrap_or_default()
    }

    /// 过滤（标题/seriesId 包含，Tauri matchListQuery 同口径）。
    fn visible_items(&self, cx: &App) -> Vec<CalendarItem> {
        let q = self.search.read(cx).value().trim().to_lowercase();
        self.current_items()
            .into_iter()
            .filter(|item| {
                q.is_empty()
                    || item.title.to_lowercase().contains(&q)
                    || item.series_id.to_lowercase().contains(&q)
            })
            .collect()
    }

    fn open_detail(&self, item: &CalendarItem, cx: &mut Context<Self>) {
        cx.emit(ReservationEvent::OpenDetail {
            series_id: item.series_id.clone(),
            prefill: DetailPrefill {
                title: item.title.clone(),
                cover: item.cover.clone(),
                tags: item
                    .rec_tags
                    .iter()
                    .filter(|t| !t.is_empty())
                    .cloned()
                    .collect(),
                desc: item.description.clone(),
                reserved: item.has_subscribed,
            },
        });
    }

    fn play_series(&self, series_id: &str, cx: &mut Context<Self>) {
        let (vid_index, start_secs) =
            hongguo_service::play_service::resume::resolve_entry(&self.state.store, series_id, &[])
                .ok()
                .unwrap_or((1, 0.0));
        cx.emit(ReservationEvent::Play {
            series_id: series_id.to_string(),
            vid_index,
            start_secs,
        });
    }

    fn toggle_selected(&mut self, series_id: &str, cx: &mut Context<Self>) {
        if !self.selected.remove(series_id) {
            self.selected.insert(series_id.to_string());
        }
        cx.notify();
    }

    // —— 渲染 ——

    fn render_toolbar(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        h_flex()
            .w_full()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(self.render_tab_pair(cx))
            .child(div().flex_1())
            .child(search_box(&self.search, cx))
            .children(self.logged_in().then(|| {
                Button::new("reservations-manage")
                    .label(if self.managing {
                        "退出管理"
                    } else {
                        "管理"
                    })
                    .outline()
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.managing = !this.managing;
                        if !this.managing {
                            this.selected.clear();
                        }
                        cx.notify();
                    }))
            }))
            .into_any_element()
    }

    /// 两个 tab 按钮（数量取自各自 TabData；加载中显示省略号）。
    fn render_tab_pair(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        h_flex()
            .items_center()
            .gap_2()
            .children([true, false].map(|online| {
                let data = if online { &self.online } else { &self.offline };
                let label = if online { "已上线" } else { "待上线" };
                let count = if data.loading {
                    "…".to_string()
                } else {
                    data.total.to_string()
                };
                let active = self.online_tab == online;
                let button = Button::new(format!("reservations-tab-{online}"))
                    .label(format!("{label} {count}"))
                    .small();
                let button = if active {
                    button.primary()
                } else {
                    button.ghost()
                };
                button.on_click(cx.listener(move |this, _, _, cx| {
                    this.online_tab = online;
                    this.selected.clear();
                    cx.notify();
                }))
            }))
            .into_any_element()
    }

    fn render_row(
        &self,
        item: &CalendarItem,
        managing: bool,
        picked: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let cover = self.covers.get(&item.series_id).cloned();
        let publish_date = if item.publish_time > 0 {
            format_month_day(item.publish_time)
        } else {
            String::new()
        };
        let tags: Vec<String> = item
            .rec_tags
            .iter()
            .filter(|t| !t.is_empty())
            .take(3)
            .cloned()
            .collect();
        h_flex()
            .id(ElementId::from(format!(
                "reservation-row-{}",
                item.series_id
            )))
            .items_stretch()
            .gap_4()
            .rounded(gpui_kit::px(12.0))
            .border_1()
            .border_color(if picked { theme.primary } else { theme.border })
            .bg(if picked {
                theme.secondary
            } else {
                theme.group_box
            })
            .p_3()
            .children(managing.then(|| {
                let sid = item.series_id.clone();
                div().flex().items_center().child(pick_dot(
                    ElementId::from(format!("reservation-pick-{}", item.series_id)),
                    picked,
                    cx.listener(move |this, _, _, cx| this.toggle_selected(&sid, cx)),
                    cx,
                ))
            }))
            // 封面（左下上线日期角标）
            .child(
                div()
                    .id(ElementId::from(format!(
                        "reservation-cover-{}",
                        item.series_id
                    )))
                    .relative()
                    .w(gpui_kit::px(92.0))
                    .h(gpui_kit::px(123.0))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .rounded(gpui_kit::px(8.0))
                    .bg(theme.secondary)
                    .cursor_pointer()
                    .on_click({
                        let item = item.clone();
                        cx.listener(move |this, _, _, cx| {
                            if this.managing {
                                this.toggle_selected(&item.series_id, cx);
                            } else {
                                this.open_detail(&item, cx);
                            }
                        })
                    })
                    .child(match cover {
                        Some(bitmap) => img(ImageSource::Render(bitmap))
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element(),
                        None => h_flex()
                            .size_full()
                            .items_center()
                            .justify_center()
                            .child(
                                gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Tv)
                                    .text_color(theme.muted_foreground),
                            )
                            .into_any_element(),
                    })
                    .children((!publish_date.is_empty()).then(|| {
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .bg(gpui_kit::rgba(0x000000a6))
                            .px_1p5()
                            .py_0p5()
                            .text_size(gpui_kit::px(10.0))
                            .text_color(gpui_kit::rgb(0xffffffff))
                            .child(SharedString::from(format!("{publish_date} 上线")))
                    })),
            )
            // 标题/简介/标签
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .justify_center()
                    .gap_1()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .id(ElementId::from(format!(
                                        "reservation-title-{}",
                                        item.series_id
                                    )))
                                    .truncate()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .cursor_pointer()
                                    .on_click({
                                        let item = item.clone();
                                        cx.listener(move |this, _, _, cx| {
                                            if this.managing {
                                                this.toggle_selected(&item.series_id, cx);
                                            } else {
                                                this.open_detail(&item, cx);
                                            }
                                        })
                                    })
                                    .child(SharedString::from(item.title.clone())),
                            )
                            .children((item.score > 0.0).then(|| {
                                h_flex()
                                    .flex_shrink_0()
                                    .items_center()
                                    .gap_0p5()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(
                                        gpui_kit::component::Icon::new(
                                            gpui_kit::assets::IconName::Star,
                                        )
                                        .small()
                                        .text_color(crate::tokens::star(theme.mode)),
                                    )
                                    .child(SharedString::from(format!("{:.1}", item.score)))
                            })),
                    )
                    .children((!item.description.is_empty()).then(|| {
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(item.description.clone()))
                    }))
                    .child(
                        h_flex()
                            .flex_wrap()
                            .items_center()
                            .gap_1p5()
                            .mt_1()
                            .children((!item.category.is_empty()).then(|| {
                                div()
                                    .rounded(gpui_kit::px(4.0))
                                    .bg(theme.secondary)
                                    .px_1p5()
                                    .py_0p5()
                                    .text_size(gpui_kit::px(10.0))
                                    .child(SharedString::from(item.category.clone()))
                            }))
                            .children(tags.into_iter().map(|tag| {
                                div()
                                    .rounded(gpui_kit::px(4.0))
                                    .border_1()
                                    .border_color(theme.border)
                                    .px_1p5()
                                    .py_0p5()
                                    .text_size(gpui_kit::px(10.0))
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(tag))
                            }))
                            .children((item.publish_time > 0 && !item.is_online).then(|| {
                                div().text_xs().text_color(theme.muted_foreground).child(
                                    SharedString::from(format!(
                                        "{} 上线",
                                        format_month_day_time(item.publish_time)
                                    )),
                                )
                            })),
                    ),
            )
            // 动作列（管理态不渲染）
            .children((!managing).then(|| {
                let mut column = v_flex().flex_shrink_0().justify_center().gap_1p5();
                if item.is_online {
                    let sid = item.series_id.clone();
                    column = column
                        .child(
                            Button::new(ElementId::from(format!(
                                "reservation-play-{}",
                                item.series_id
                            )))
                            .icon(gpui_kit::assets::IconName::Play)
                            .label("播放")
                            .outline()
                            .small()
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.play_series(&sid, cx);
                                },
                            )),
                        )
                        .child({
                            let item = item.clone();
                            Button::new(ElementId::from(format!(
                                "reservation-detail-{}",
                                item.series_id
                            )))
                            .label("详情")
                            .ghost()
                            .small()
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.open_detail(&item, cx);
                                },
                            ))
                        });
                } else {
                    let sid = item.series_id.clone();
                    let cancelling = self.cancelling.contains(&item.series_id);
                    column = column.child(
                        Button::new(ElementId::from(format!(
                            "reservation-cancel-{}",
                            item.series_id
                        )))
                        .icon(gpui_kit::assets::IconName::BellRing)
                        .label("取消预约")
                        .outline()
                        .small()
                        .disabled(cancelling)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.cancel_reserve(sid.clone(), cx);
                        })),
                    );
                }
                column
            }))
            .into_any_element()
    }

    /// 预览种子（screenshot feature）：灌假预约列表，跳过网络。
    /// `reservation-offline` = 待上线 tab；`reservation-empty` = 空态；
    /// `reservation-manage` = 管理模式（前两条已选）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(&mut self, scene: &str, cx: &mut Context<Self>) {
        let empty = scene == "reservation-empty";
        let online = !scene.contains("offline");
        self.online_tab = online;
        let online_items = if empty || !online {
            Vec::new()
        } else {
            crate::preview::fixtures::reservation_items(true)
        };
        let offline_items = if empty || online {
            Vec::new()
        } else {
            crate::preview::fixtures::reservation_items(false)
        };
        for (i, item) in offline_items.iter().enumerate() {
            self.covers.insert(
                item.series_id.clone(),
                crate::preview::fixtures::cover(6 + i),
            );
        }
        for (i, item) in online_items.iter().enumerate() {
            self.covers
                .insert(item.series_id.clone(), crate::preview::fixtures::cover(i));
        }
        self.online = TabData {
            total: online_items.len() as i64,
            items: Some(online_items),
            loading: false,
            failed: None,
        };
        self.offline = TabData {
            total: offline_items.len() as i64,
            items: Some(offline_items),
            loading: false,
            failed: None,
        };
        if scene == "reservation-manage" {
            self.managing = true;
            self.selected = self.seed_selection().into_iter().collect();
        }
        self.preview_logged_in = true;
        cx.notify();
    }

    #[cfg(feature = "screenshot")]
    fn seed_selection(&self) -> Vec<String> {
        self.online
            .items
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .take(2)
            .map(|i| i.series_id.clone())
            .collect()
    }
}

impl gpui_kit::EventEmitter<ReservationEvent> for ReservationView {}

impl gpui_kit::Render for ReservationView {
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
                        .child("登录后才能查看预约列表"),
                )
                .child(
                    Button::new("reservations-login")
                        .icon(gpui_kit::assets::IconName::LogIn)
                        .label("登录")
                        .small()
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.emit(ReservationEvent::Login);
                        })),
                )
                .into_any_element()
        } else {
            let current = self.current();
            if current.loading && current.items.is_none() {
                h_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .child(Spinner::new().small())
                    .into_any_element()
            } else if let Some(err) = current.failed.clone() {
                let theme = cx.theme();
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("预约列表加载失败"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(err)),
                    )
                    .child(
                        Button::new("reservations-retry")
                            .label("重试")
                            .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                    )
                    .into_any_element()
            } else if self.current_items().is_empty() {
                let theme = cx.theme();
                v_flex()
                    .items_center()
                    // Tauri Empty：流内 py-16 顶部对齐
                    .py_16()
                    .gap_2()
                    .child(
                        gpui_kit::component::Icon::new(gpui_kit::assets::IconName::BellRing)
                            .with_size(gpui_kit::px(32.0))
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("还没有预约的短剧"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("在上新日历点「预约」，上线时这里就能看到"),
                    )
                    .into_any_element()
            } else {
                let managing = self.managing;
                let visible = self.visible_items(cx);
                div()
                    .id("reservations-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_4()
                    .child(
                        v_flex()
                            .w_full()
                            .gap_2()
                            .children(visible.iter().map(|item| {
                                let picked = self.selected.contains(&item.series_id);
                                self.render_row(item, managing, picked, cx)
                            })),
                    )
                    .into_any_element()
            }
        };
        let toolbar = logged_in.then(|| self.render_toolbar(cx));
        let batch_bar = (logged_in && self.managing).then(|| {
            let total = self.visible_items(cx).len();
            BatchBar::new(self.selected.len(), total, self.deleting, "取消预约")
                .on_toggle_all(cx.listener(|this, _, _, cx| {
                    let visible = this.visible_items(cx);
                    let total = visible.len();
                    if total > 0 && this.selected.len() >= total {
                        this.selected.clear();
                    } else {
                        for item in visible {
                            this.selected.insert(item.series_id);
                        }
                    }
                    cx.notify();
                }))
                .on_delete(cx.listener(|this, _, window, cx| {
                    this.confirm_batch_delete(window, cx);
                }))
                .into_any_element()
        });
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .id("reservations-page")
            .children(toolbar.map(|t| div().w_full().px_4().pt_4().child(t)))
            .child(body)
            .children(batch_bar)
    }
}

/// UTC 毫秒 → "9月27日"（角标用）。
fn format_month_day(ms: i64) -> String {
    use chrono::{Datelike, TimeZone};
    match chrono::Local.timestamp_millis_opt(ms).single() {
        Some(dt) => format!("{}月{}日", dt.month(), dt.day()),
        None => String::new(),
    }
}

/// UTC 毫秒 → "10-07 12:00"（待上线精确时间）。
fn format_month_day_time(ms: i64) -> String {
    use chrono::{Datelike, TimeZone, Timelike};
    match chrono::Local.timestamp_millis_opt(ms).single() {
        Some(dt) => format!(
            "{:02}-{:02} {:02}:{:02}",
            dt.month(),
            dt.day(),
            dt.hour(),
            dt.minute()
        ),
        None => String::new(),
    }
}
