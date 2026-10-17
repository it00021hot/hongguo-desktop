//! 观看历史页（Tauri `/history` = `history-page.tsx` 的 GPUI 对齐）。
//!
//! **双模式**（Tauri issue #10 同款）：登录 = 云端观看记录
//! （read_history/list，book_type=2 短剧过滤，跨设备）；未登录 = 本地
//! 观看记录（播放时已落库，删除只动本机，页面顶部给「未登录」提示行）。
//! 行式列表（封面 + 标题 + 进度徽标 + 相对时间 + 继续播放），顶部
//! 「全部/未看完/已看完」筛选与标题搜索，管理模式下多选批量删除
//! （云端一条请求；本地逐剧删）。云端数据与操作走 `hongguo_api::history`，
//! 本地走 `hongguo_store`，与 Tauri 的 history command 同一段。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

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
use hongguo_api::history::WatchHistoryItem;
use hongguo_common::model::LocalHistoryItem;
use hongguo_service::app_state::AppStateInner;

use crate::bridge;
use crate::home::fmt::format_duration;
use crate::pages::list_common::{BatchBar, decode_cover, pick_dot, search_box};

/// 历史页发往壳的事件（壳订阅后切目的地/起播）。
#[derive(Debug, Clone)]
pub(crate) enum HistoryEvent {
    Play {
        series_id: String,
        vid_index: u32,
        start_secs: f64,
    },
}

/// 筛选 tab（Tauri 全部/未看完/已看完）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryTab {
    All,
    Unfinished,
    Finished,
}

impl HistoryTab {
    const ALL: [HistoryTab; 3] = [
        HistoryTab::All,
        HistoryTab::Unfinished,
        HistoryTab::Finished,
    ];

    fn label(self) -> String {
        match self {
            HistoryTab::All => crate::i18n::t("history.tabAll"),
            HistoryTab::Unfinished => crate::i18n::t("history.tabUnfinished"),
            HistoryTab::Finished => crate::i18n::t("history.tabFinished"),
        }
    }
}

/// 参考端同款「已看完」判定：看到最后一集即算，与片内进度无关。
fn is_finished(item: &WatchHistoryItem) -> bool {
    item.episode_cnt > 0 && item.vid_index.max(1) >= item.episode_cnt
}

/// 本地条目 → 渲染行（字段与云端同形；u32 集号升为 i64 统一口径）。
fn row_from_local(item: &LocalHistoryItem) -> WatchHistoryItem {
    WatchHistoryItem {
        series_id: item.series_id.clone(),
        title: item.title.clone(),
        cover: item.cover.clone(),
        vid_index: i64::from(item.vid_index),
        vid: item.vid.clone(),
        position_ms: item.position_ms,
        duration_ms: item.duration_ms,
        episode_cnt: i64::from(item.episode_cnt),
        updated_at_ms: item.updated_at_ms,
    }
}

enum Phase {
    Loading,
    Failed(String),
    Loaded,
}

pub(crate) struct HistoryView {
    state: Arc<AppStateInner>,
    phase: Phase,
    items: Vec<WatchHistoryItem>,
    covers: HashMap<String, Arc<RenderImage>>,
    covers_pending: HashSet<String>,
    search: Entity<InputState>,
    tab: HistoryTab,
    managing: bool,
    selected: BTreeSet<String>,
    deleting: bool,
    loading: bool,
    /// 渐进渲染：首批 20 行，「加载更多」按钮每批 +40（Tauri 用滚动哨兵
    /// 自动续载；GPUI 先给显式入口，语义相同、列表长度不设上限）。
    visible_count: usize,
    /// 预览种子用的登录态开关（真实壳恒 false，以 settings 的 account 为准）。
    preview_logged_in: bool,
}

impl HistoryView {
    /// 语言切换：刷新搜索框 placeholder，保留筛选条件与输入。
    pub(crate) fn refresh_locale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |input, cx| {
            input.set_placeholder(crate::i18n::t("history.searchPlaceholder"), window, cx);
        });
        cx.notify();
    }

    pub(crate) fn new(
        state: Arc<AppStateInner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::i18n::t("history.searchPlaceholder"))
        });
        cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        Self {
            state,
            phase: Phase::Loading,
            items: Vec::new(),
            covers: HashMap::new(),
            covers_pending: HashSet::new(),
            search,
            tab: HistoryTab::All,
            managing: false,
            selected: BTreeSet::new(),
            deleting: false,
            loading: false,
            visible_count: 20,
            preview_logged_in: false,
        }
    }

    /// 登录态（settings 里有账号即已登录，与首页同口径）。
    fn logged_in(&self) -> bool {
        self.state.settings().account.is_some() || self.preview_logged_in
    }

    /// 拉历史（双模式，Tauri 同款）：登录 = 云端首屏一页；未登录 = 本地
    /// 观看记录（播放时已落库，全量读）。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        if self.items.is_empty() {
            self.phase = Phase::Loading;
        }
        cx.notify();
        if !self.logged_in() {
            // 本地模式（issue #10）
            let state = Arc::clone(&self.state);
            let rx = bridge::spawn(async move {
                tokio::task::spawn_blocking(move || state.store.local_history_list())
                    .await
                    .expect("阻塞池 join")
            });
            cx.spawn(async move |this, cx| {
                let result = rx.await.expect("tokio 桥不会丢弃发送端");
                this.update(cx, |view, cx| {
                    view.loading = false;
                    match result {
                        Ok(items) => {
                            log::info!("[History] GPUI 壳本地模式 -> {} 条", items.len());
                            view.items = items.iter().map(row_from_local).collect();
                            view.selected.clear();
                            view.visible_count = 20;
                            view.phase = Phase::Loaded;
                            view.load_covers(&view.items.clone(), cx);
                        }
                        Err(e) => {
                            log::warn!("[History] GPUI 壳本地读取失败: {e}");
                            if view.items.is_empty() {
                                view.phase = Phase::Failed(crate::i18n::app_error_text(&e));
                            }
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
            return;
        }
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::history::fetch_watch_history(0, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            this.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok(page) => {
                        log::info!("[History] GPUI 壳 -> {} 条", page.items.len());
                        view.load_covers(&page.items, cx);
                        view.items = page.items;
                        view.selected.clear();
                        view.visible_count = 20;
                        view.phase = Phase::Loaded;
                    }
                    Err(e) => {
                        log::warn!("[History] GPUI 壳失败: {e}");
                        if view.items.is_empty() {
                            view.phase = Phase::Failed(crate::i18n::app_error_text(&e));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 逐条取封面字节（条目自带上线封面 URL；HEIC 走同一段转码阶梯）。
    fn load_covers(&mut self, items: &[WatchHistoryItem], cx: &mut Context<Self>) {
        for item in items {
            let key = format!("{}:{}", item.series_id, item.updated_at_ms);
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

    /// 批量删除历史：云端一条请求（useWatchHistoryDelete 同款）；本地逐剧
    /// 删（useLocalHistoryDelete 同款）。
    fn batch_delete(&mut self, cx: &mut Context<Self>) {
        if self.deleting {
            return;
        }
        let refs: Vec<(String, String, i64)> = self
            .items
            .iter()
            .filter(|i| self.selected.contains(&i.series_id))
            .map(|i| (i.series_id.clone(), i.vid.clone(), i.vid_index))
            .collect();
        if refs.is_empty() {
            return;
        }
        self.deleting = true;
        cx.notify();
        let total = refs.len();
        let local_mode = !self.logged_in();
        let state_local = Arc::clone(&self.state);
        let state_cloud = Arc::clone(&self.state);
        let ids: Vec<String> = refs.iter().map(|(sid, _, _)| sid.clone()).collect();
        let rx = bridge::spawn(async move {
            if local_mode {
                // 本地模式：逐剧删（只动本机，Tauri localDelete 同款）
                tokio::task::spawn_blocking(move || state_local.store.delete_local_history(&ids))
                    .await
                    .expect("阻塞池 join")
            } else {
                let env = state_cloud.api_env();
                hongguo_api::history::delete_watch_history(&refs, &env).await
            }
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.deleting = false;
                match result {
                    Ok(()) => {
                        window.push_notification(
                            Notification::success(crate::i18n::tf(
                                "batch.done",
                                &[("count", &total.to_string())],
                            )),
                            cx,
                        );
                        view.managing = false;
                        view.selected.clear();
                    }
                    Err(e) => {
                        log::warn!("[History] 批量删除失败: {e}");
                        window.push_notification(
                            Notification::error(crate::i18n::tf(
                                "batch.partialFail",
                                &[("error", &crate::i18n::app_error_text(&e))],
                            )),
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
                .title(crate::i18n::t("history.deleteConfirmTitle"))
                .description(crate::i18n::tf(
                    "batch.confirmBody",
                    &[("count", &count.to_string())],
                ))
                .ok_text(crate::i18n::t("common.confirm"))
                .cancel_text(crate::i18n::t("common.cancel"))
                .show_cancel(true)
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _window, cx| {
                    view.update(cx, |view, cx| view.batch_delete(cx));
                    true
                })
        });
    }

    /// 过滤（tab + 标题/seriesId 包含，Tauri 同口径）。
    fn visible_items(&self, cx: &App) -> Vec<WatchHistoryItem> {
        let q = self.search.read(cx).value().trim().to_lowercase();
        self.items
            .iter()
            .filter(|item| {
                match self.tab {
                    HistoryTab::Finished if !is_finished(item) => return false,
                    HistoryTab::Unfinished if is_finished(item) => return false,
                    _ => {}
                }
                if q.is_empty() {
                    return true;
                }
                item.title.to_lowercase().contains(&q) || item.series_id.to_lowercase().contains(&q)
            })
            .cloned()
            .collect()
    }

    fn open_item(&self, item: &WatchHistoryItem, cx: &mut Context<Self>) {
        // 云端 vid_index 1 起（历史数据可能混 0，钳到第 1 集）；进度直接
        // 取自本行（页数据源就是云端历史，等价 Tauri 播放器的云端兜底）。
        let vid_index = item.vid_index.max(1) as u32;
        let start_secs = (item.position_ms as f64 / 1000.0).max(0.0);
        cx.emit(HistoryEvent::Play {
            series_id: item.series_id.clone(),
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
            .children(HistoryTab::ALL.map(|tab| {
                let active = self.tab == tab;
                let button = Button::new(format!("history-tab-{}", tab.label()))
                    .label(tab.label())
                    .small();
                let button = if active {
                    // Tauri ToggleGroupItem：选中 bg-primary text-primary-foreground
                    //（深色主题白底胶囊）；secondary 是深灰胶囊，对比度与真机不同
                    button.primary()
                } else {
                    button.ghost()
                };
                button.on_click(cx.listener(move |this, _, _, cx| {
                    this.tab = tab;
                    this.visible_count = 20;
                    cx.notify();
                }))
            }))
            .child(div().flex_1())
            .child(search_box(&self.search, cx))
            .child(
                Button::new("history-manage")
                    .label(if self.managing {
                        crate::i18n::t("batch.exitManage")
                    } else {
                        crate::i18n::t("batch.manage")
                    })
                    .outline()
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.managing = !this.managing;
                        if !this.managing {
                            this.selected.clear();
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn render_row(
        &self,
        item: &WatchHistoryItem,
        managing: bool,
        picked: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let cover = self
            .covers
            .get(&format!("{}:{}", item.series_id, item.updated_at_ms))
            .cloned();
        let index = item.vid_index.max(1);
        let badge = if item.duration_ms > 0 {
            let total = if item.episode_cnt > 0 {
                item.episode_cnt.to_string()
            } else {
                "–".to_string()
            };
            crate::i18n::tf(
                "history.watching",
                &[
                    ("index", &index.to_string()),
                    ("total", total.as_str()),
                    (
                        "time",
                        format_duration(item.position_ms as f64 / 1000.0).as_str(),
                    ),
                ],
            )
        } else {
            crate::i18n::tf("player.epShort", &[("index", &index.to_string())])
        };
        h_flex()
            .id(ElementId::from(format!(
                "history-row-{}:{}",
                item.series_id, item.updated_at_ms
            )))
            .items_center()
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
            .cursor_pointer()
            .when(!managing, |s| s.hover(|s| s.border_color(theme.foreground)))
            .on_click({
                let item = item.clone();
                cx.listener(move |this, _, _, cx| {
                    if this.managing {
                        this.toggle_selected(&item.series_id, cx);
                    } else {
                        this.open_item(&item, cx);
                    }
                })
            })
            .children(managing.then(|| {
                let sid = item.series_id.clone();
                pick_dot(
                    ElementId::from(format!("history-pick-{}", item.series_id)),
                    picked,
                    cx.listener(move |this, _, _, cx| this.toggle_selected(&sid, cx)),
                    cx,
                )
            }))
            .child(
                div()
                    .w(gpui_kit::px(72.0))
                    .h(gpui_kit::px(96.0))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .rounded(gpui_kit::px(8.0))
                    .bg(theme.secondary)
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
                                gpui_kit::component::Icon::new(
                                    gpui_kit::assets::IconName::ClockArrowDown,
                                )
                                .text_color(theme.muted_foreground),
                            )
                            .into_any_element(),
                    }),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .gap_1p5()
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(SharedString::from(if item.title.is_empty() {
                                crate::i18n::t("history.unknownTitle")
                            } else {
                                item.title.clone()
                            })),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .child(
                                div()
                                    .rounded(gpui_kit::px(4.0))
                                    .bg(theme.secondary)
                                    .px_1p5()
                                    .py_0p5()
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .child(SharedString::from(badge)),
                            )
                            .child(
                                div()
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(days_ago(item.updated_at_ms))),
                            ),
                    ),
            )
            .children((!managing).then(|| {
                let item = item.clone();
                Button::new(ElementId::from(format!(
                    "history-continue-{}:{}",
                    item.series_id, item.updated_at_ms
                )))
                .icon(gpui_kit::assets::IconName::Play)
                .label(crate::i18n::t("history.continue"))
                .outline()
                .small()
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.open_item(&item, cx);
                }))
            }))
            .into_any_element()
    }

    /// 预览种子（screenshot feature）：灌假历史行，跳过网络。
    /// `history-empty` = 空态；`history-manage` = 管理模式（前两条已选）；
    /// `history-local` = 未登录本地模式（顶部提示行）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(&mut self, scene: &str, cx: &mut Context<Self>) {
        let empty = scene == "history-empty";
        // 本地模式场景之外都按登录态出图（本地场景留给提示行形态）
        self.preview_logged_in = scene != "history-local";
        self.phase = Phase::Loaded;
        self.loading = false;
        self.items = if empty {
            Vec::new()
        } else {
            crate::preview::fixtures::watch_history_items()
        };
        for (i, item) in self.items.iter().enumerate() {
            self.covers.insert(
                format!("{}:{}", item.series_id, item.updated_at_ms),
                crate::preview::fixtures::cover(i),
            );
        }
        if scene == "history-manage" {
            self.managing = true;
            self.selected = self
                .items
                .iter()
                .take(2)
                .map(|i| i.series_id.clone())
                .collect();
        }
        cx.notify();
    }
}

impl gpui_kit::EventEmitter<HistoryEvent> for HistoryView {}

impl gpui_kit::Render for HistoryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.phase {
            Phase::Loading => h_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(Spinner::new().small())
                .into_any_element(),
            Phase::Failed(msg) => {
                let detail = msg.clone();
                let theme = cx.theme();
                // Tauri ErrorState（Empty py-16）：流内顶部对齐
                v_flex()
                    .items_center()
                    .py_16()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("history.loadFailed")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(detail)),
                    )
                    .child(
                        Button::new("history-retry")
                            .label(crate::i18n::t("common.retry"))
                            .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                    )
                    .into_any_element()
            }
            Phase::Loaded if self.items.is_empty() => {
                let theme = cx.theme();
                v_flex()
                    .items_center()
                    // Tauri Empty：流内 py-16 顶部对齐
                    .py_16()
                    .gap_2()
                    .child(
                        gpui_kit::component::Icon::new(gpui_kit::assets::IconName::ClockArrowDown)
                            .with_size(gpui_kit::px(32.0))
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("history.empty")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("history.emptyHint")),
                    )
                    .into_any_element()
            }
            Phase::Loaded => {
                let visible = self.visible_items(cx);
                if visible.is_empty() {
                    // 列表非空但被 tab/搜索滤空：给一行轻提示（区分场景）
                    let theme = cx.theme();
                    let query_empty = self.search.read(cx).value().trim().is_empty();
                    let hint = if !query_empty {
                        crate::i18n::t("common.searchNoMatch")
                    } else if self.tab == HistoryTab::Finished {
                        crate::i18n::t("history.noMatchFinished")
                    } else {
                        crate::i18n::t("history.noMatchUnfinished")
                    };
                    v_flex()
                        .items_center()
                        .py_16()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(hint),
                        )
                        .into_any_element()
                } else {
                    let managing = self.managing;
                    let shown = visible.len();
                    let visible_count = self.visible_count.min(shown);
                    div()
                        .id("history-list")
                        .flex_1()
                        .overflow_y_scroll()
                        .p_4()
                        .child(
                            v_flex()
                                .w_full()
                                .gap_2()
                                .children(visible.iter().take(visible_count).map(|item| {
                                    let picked = self.selected.contains(&item.series_id);
                                    self.render_row(item, managing, picked, cx)
                                }))
                                .children((visible_count < shown).then(|| {
                                    h_flex().w_full().justify_center().p_2().child(
                                        Button::new("history-more")
                                            .label(crate::i18n::tf(
                                                "history.loadMore",
                                                &[
                                                    ("shown", &visible_count.to_string()),
                                                    ("total", &shown.to_string()),
                                                ],
                                            ))
                                            .ghost()
                                            .small()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.visible_count += 40;
                                                cx.notify();
                                            })),
                                    )
                                })),
                        )
                        .into_any_element()
                }
            }
        };
        let toolbar = (matches!(self.phase, Phase::Loaded) && !self.items.is_empty())
            .then(|| self.render_toolbar(cx));
        // 未登录提示行（issue #10）：本地模式可记可管、数据不上云
        let local_hint = (!self.logged_in() && !self.items.is_empty()).then(|| {
            let theme = cx.theme();
            div()
                .w_full()
                .px_4()
                .pt_3()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(crate::i18n::t("history.localModeHint"))
        });
        let batch_bar = self.managing.then(|| {
            let total = self.visible_items(cx).len();
            BatchBar::new(
                self.selected.len(),
                total,
                self.deleting,
                crate::i18n::t("batch.deleteSelected"),
            )
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
            .id("history-page")
            .children(local_hint)
            .children(toolbar.map(|t| div().w_full().px_4().pt_4().child(t)))
            .child(body)
            .children(batch_bar)
    }
}

/// unix 毫秒 → 相对时间（今天/昨天/N 天前，Tauri history formatTimeAgo 同款）。
fn days_ago(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let diff_days = (now - ms) / 86_400_000;
    match diff_days {
        d if d <= 0 => crate::i18n::t("history.today"),
        1 => crate::i18n::t("history.yesterday"),
        d => crate::i18n::tf("history.daysAgo", &[("count", &d.to_string())]),
    }
}
