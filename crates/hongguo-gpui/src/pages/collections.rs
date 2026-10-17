//! 收藏页（Tauri `/collections` = `collection-page.tsx` 的 GPUI 对齐）。
//!
//! 书架列表（bookshelf/video/list）：卡片网格（封面 3:4 + 标题 + 共 N 集），
//! 悬浮出「继续播放」与「取消收藏」；左上类型筛选（全部/真人剧/漫剧，按
//! 条目 content_type）+ 搜索框 + 管理模式（多选后一请求批量取消收藏）。
//! 数据与操作走 `hongguo_api::interact`（fetch_bookshelf / collect_series /
//! collect_series_batch），与 Tauri 的 bookshelf 系列 command 同一段。

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
use hongguo_api::interact::BookshelfEntry;
use hongguo_service::app_state::AppStateInner;

use crate::bridge;
use crate::pages::list_common::{
    BatchBar, decode_cover, fetch_series_visual, pick_dot, search_box,
};

/// 收藏页发往壳的事件（壳订阅后切目的地/起播）。
#[derive(Debug, Clone)]
pub(crate) enum CollectionEvent {
    Play {
        series_id: String,
        vid_index: u32,
        start_secs: f64,
    },
}

/// 内容类型筛选（Tauri 按 content_type：1=真人 1004=漫剧）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeTab {
    All,
    Real,
    Comic,
}

impl TypeTab {
    const ALL: [TypeTab; 3] = [TypeTab::All, TypeTab::Real, TypeTab::Comic];

    fn label(self) -> &'static str {
        match self {
            TypeTab::All => "全部",
            TypeTab::Real => "真人剧",
            TypeTab::Comic => "漫剧",
        }
    }

    fn matches(self, entry: &BookshelfEntry) -> bool {
        match self {
            TypeTab::All => true,
            TypeTab::Real => entry.content_type == 1,
            TypeTab::Comic => entry.content_type == 1004,
        }
    }
}

enum Phase {
    Loading,
    Failed(String),
    /// 未登录：预约/书架接口匿名只给空表，这里给明确提示 + 重试。
    LoginRequired,
    Loaded,
}

pub(crate) struct CollectionView {
    state: Arc<AppStateInner>,
    phase: Phase,
    entries: Vec<BookshelfEntry>,
    /// resolve 到位的标题/集数（书架条目本身只有 series_id）。
    titles: HashMap<String, String>,
    episode_counts: HashMap<String, u32>,
    covers: HashMap<String, Arc<RenderImage>>,
    covers_pending: HashSet<String>,
    hovered: Option<String>,
    search: Entity<InputState>,
    type_tab: TypeTab,
    managing: bool,
    selected: BTreeSet<String>,
    deleting: bool,
    loading: bool,
}

impl CollectionView {
    pub(crate) fn new(
        state: Arc<AppStateInner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索收藏的剧"));
        cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        Self {
            state,
            phase: Phase::Loading,
            entries: Vec::new(),
            titles: HashMap::new(),
            episode_counts: HashMap::new(),
            covers: HashMap::new(),
            covers_pending: HashSet::new(),
            hovered: None,
            search,
            type_tab: TypeTab::All,
            managing: false,
            selected: BTreeSet::new(),
            deleting: false,
            loading: false,
        }
    }

    /// 拉书架列表。未登录直接给空列表（Tauri useBookshelf enabled=!!account
    /// 的同款形态：匿名不发请求）；登录后失败按「未登录」/「加载失败」分态。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        let account = self.state.settings().account.clone();
        let Some(account) = account else {
            // 未登录：空列表空态（与 Tauri 一致，不发请求）
            self.phase = Phase::Loaded;
            self.entries.clear();
            self.loading = false;
            cx.notify();
            return;
        };
        self.loading = true;
        if self.entries.is_empty() {
            self.phase = Phase::Loading;
        }
        cx.notify();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::interact::fetch_bookshelf(&account.user_id, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            this.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok(entries) => {
                        log::info!("[Collections] GPUI 壳 -> {} 条", entries.len());
                        view.load_visuals(&entries, cx);
                        view.entries = entries;
                        let ids: HashSet<&str> =
                            view.entries.iter().map(|e| e.series_id.as_str()).collect();
                        view.selected.retain(|id| ids.contains(id.as_str()));
                        view.phase = Phase::Loaded;
                    }
                    Err(e) => {
                        log::warn!("[Collections] GPUI 壳失败: {e}");
                        if view.entries.is_empty() {
                            view.phase = if format!("{e}").contains("未登录") {
                                Phase::LoginRequired
                            } else {
                                Phase::Failed(format!("{e}"))
                            };
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 逐条解析标题/集数/封面（书架条目只有 series_id）。
    fn load_visuals(&mut self, entries: &[BookshelfEntry], cx: &mut Context<Self>) {
        for entry in entries {
            let key = entry.series_id.clone();
            if key.is_empty() || !self.covers_pending.insert(key.clone()) {
                continue;
            }
            let rx = fetch_series_visual(Arc::clone(&self.state), entry.series_id.clone());
            cx.spawn(async move |this, cx| {
                let visual = rx.await.expect("tokio 桥不会丢弃发送端");
                this.update(cx, |view, cx| {
                    view.covers_pending.remove(&visual.series_id);
                    if !visual.title.is_empty() {
                        view.titles.insert(visual.series_id.clone(), visual.title);
                        view.episode_counts
                            .insert(visual.series_id.clone(), visual.episode_count);
                    }
                    if let Some(bytes) = &visual.cover
                        && let Some(image) = decode_cover(bytes)
                    {
                        view.covers.insert(visual.series_id.clone(), image);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    /// 单条取消收藏（bookshelf update operate_type=1）。
    fn remove_one(&mut self, series_id: String, cx: &mut Context<Self>) {
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::interact::collect_series(&series_id, false, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| match result {
                Ok(()) => {
                    window.push_notification(Notification::success("已取消收藏"), cx);
                    view.load(cx);
                }
                Err(e) => {
                    log::warn!("[Collections] 取消收藏失败: {e}");
                    window.push_notification(Notification::error(format!("取消收藏失败：{e}")), cx);
                }
            });
        })
        .detach();
    }

    /// 批量取消收藏（一个请求，Tauri useSeriesCollectBatch 同款）。
    fn batch_remove(&mut self, cx: &mut Context<Self>) {
        if self.deleting {
            return;
        }
        let refs: Vec<(String, bool)> = self
            .entries
            .iter()
            .filter(|e| self.selected.contains(&e.series_id))
            .map(|e| (e.series_id.clone(), false))
            .collect();
        if refs.is_empty() {
            return;
        }
        self.deleting = true;
        cx.notify();
        let total = refs.len();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::interact::collect_series_batch(&refs, &env).await
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
                        log::warn!("[Collections] 批量取消收藏失败: {e}");
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

    fn confirm_batch_remove(&self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let count = self.selected.len();
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            let view = view.clone();
            dialog
                .title("批量取消收藏？")
                .description(format!("将删除选中的 {count} 项，删除后不可恢复。"))
                .ok_text("确定")
                .cancel_text("取消")
                .show_cancel(true)
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _window, cx| {
                    view.update(cx, |view, cx| view.batch_remove(cx));
                    true
                })
        });
    }

    /// 客户端过滤（Tauri `matchListQuery` + 类型 tab 同口径）。
    fn visible_entries(&self, cx: &App) -> Vec<BookshelfEntry> {
        let q = self.search.read(cx).value().trim().to_lowercase();
        self.entries
            .iter()
            .filter(|e| {
                if !self.type_tab.matches(e) {
                    return false;
                }
                if q.is_empty() {
                    return true;
                }
                let title = self.titles.get(&e.series_id).cloned().unwrap_or_default();
                title.to_lowercase().contains(&q) || e.series_id.to_lowercase().contains(&q)
            })
            .cloned()
            .collect()
    }

    fn open_series(&self, series_id: String, cx: &mut Context<Self>) {
        let (vid_index, start_secs) = hongguo_service::play_service::resume::resolve_entry(
            &self.state.store,
            &series_id,
            &[],
        )
        .ok()
        .unwrap_or((1, 0.0));
        cx.emit(CollectionEvent::Play {
            series_id,
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
            .children(TypeTab::ALL.map(|tab| {
                let active = self.type_tab == tab;
                let button = Button::new(format!("collections-tab-{}", tab.label()))
                    .label(tab.label())
                    .small();
                let button = if active {
                    // 选中胶囊走 primary（Tauri data-[state=on]:bg-primary 同款）
                    button.primary()
                } else {
                    button.ghost()
                };
                button.on_click(cx.listener(move |this, _, _, cx| {
                    this.type_tab = tab;
                    cx.notify();
                }))
            }))
            .child(div().flex_1())
            .child(search_box(&self.search, cx))
            .child(
                Button::new("collections-manage")
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
                    })),
            )
            .into_any_element()
    }

    fn render_card(
        &self,
        entry: &BookshelfEntry,
        managing: bool,
        picked: bool,
        hovered: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let series_id = entry.series_id.clone();
        let title = self
            .titles
            .get(&series_id)
            .cloned()
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| {
                let tail: String = series_id
                    .chars()
                    .rev()
                    .take(6)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                format!("剧集 {tail}")
            });
        let episode_line = match self.episode_counts.get(&series_id) {
            Some(n) if *n > 0 => format!("共 {n} 集"),
            _ => "正在获取剧集信息…".to_string(),
        };
        let cover = self.covers.get(&series_id).cloned();
        let play_open = hovered && !managing;
        div()
            .relative()
            .id(ElementId::from(format!("collections-card-{}", series_id)))
            .on_hover(cx.listener({
                let key = series_id.clone();
                move |this, hovered: &bool, _, cx| {
                    let now = hovered.then(|| key.clone());
                    if this.hovered != now {
                        this.hovered = now;
                        cx.notify();
                    }
                }
            }))
            .child(
                v_flex()
                    .id(ElementId::from(format!("collections-open-{}", series_id)))
                    .w_full()
                    .overflow_hidden()
                    .rounded(gpui_kit::px(12.0))
                    .border_1()
                    .when(picked, |s| s.border_color(theme.primary))
                    .when(!picked, |s| s.border_color(theme.border))
                    .when(!managing, |s| s.hover(|s| s.border_color(theme.foreground)))
                    .bg(if picked {
                        theme.secondary
                    } else {
                        theme.group_box
                    })
                    .cursor_pointer()
                    .on_click({
                        let series_id = series_id.clone();
                        cx.listener(move |this, _, _, cx| {
                            if this.managing {
                                this.toggle_selected(&series_id, cx);
                            } else {
                                this.open_series(series_id.clone(), cx);
                            }
                        })
                    })
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(gpui_kit::px(229.0))
                            .flex_shrink_0()
                            .overflow_hidden()
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
                                            gpui_kit::assets::IconName::Star,
                                        )
                                        .text_color(theme.muted_foreground),
                                    )
                                    .into_any_element(),
                            })
                            .children(play_open.then(|| {
                                h_flex()
                                    .absolute()
                                    .inset_0()
                                    .items_center()
                                    .justify_center()
                                    .bg(gpui_kit::rgba(0x00000059))
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_1p5()
                                            .rounded_full()
                                            .bg(theme.primary)
                                            .px_3()
                                            .py_1p5()
                                            .text_xs()
                                            .text_color(theme.primary_foreground)
                                            .child(
                                                gpui_kit::component::Icon::new(
                                                    gpui_kit::assets::IconName::Play,
                                                )
                                                .small(),
                                            )
                                            .child("继续播放"),
                                    )
                            })),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_1()
                            .p_2p5()
                            .child(
                                div()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(SharedString::from(title)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(episode_line)),
                            ),
                    ),
            )
            .children(managing.then(|| {
                let sid = series_id.clone();
                div().absolute().top_2().left_2().child(pick_dot(
                    ElementId::from(format!("collections-pick-{}", series_id)),
                    picked,
                    cx.listener(move |this, _, _, cx| this.toggle_selected(&sid, cx)),
                    cx,
                ))
            }))
            .children((hovered && !managing).then(|| {
                let sid = series_id.clone();
                Button::new(ElementId::from(format!("collections-remove-{}", series_id)))
                    .icon(gpui_kit::assets::IconName::StarOff)
                    .tooltip("取消收藏")
                    .ghost()
                    .small()
                    .absolute()
                    .top_2()
                    .right_2()
                    .bg(gpui_kit::rgba(0x00000099))
                    .text_color(gpui_kit::rgb(0xffffffff))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.remove_one(sid.clone(), cx);
                    }))
            }))
            .into_any_element()
    }

    /// 预览种子（screenshot feature）：灌假书架 + 合成封面，跳过网络。
    /// `collections-empty` = 空态；`collections-manage` = 管理模式。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(&mut self, scene: &str, cx: &mut Context<Self>) {
        let empty = scene == "collections-empty";
        self.phase = Phase::Loaded;
        self.loading = false;
        self.entries = if empty {
            Vec::new()
        } else {
            crate::preview::fixtures::bookshelf_entries()
        };
        for (i, entry) in self.entries.iter().enumerate() {
            let title =
                crate::preview::fixtures::SERIES[i % crate::preview::fixtures::SERIES.len()].1;
            self.titles.insert(entry.series_id.clone(), title.into());
            self.episode_counts
                .insert(entry.series_id.clone(), 24 + i as u32 * 6);
            self.covers
                .insert(entry.series_id.clone(), crate::preview::fixtures::cover(i));
        }
        if scene == "collections-manage" {
            self.managing = true;
            self.selected = self
                .entries
                .iter()
                .take(2)
                .map(|e| e.series_id.clone())
                .collect();
        }
        cx.notify();
    }
}

impl gpui_kit::EventEmitter<CollectionEvent> for CollectionView {}

impl gpui_kit::Render for CollectionView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body =
            match &self.phase {
                Phase::Loading => h_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .child(Spinner::new().small())
                    .into_any_element(),
                Phase::LoginRequired => {
                    let theme = cx.theme();
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .child(
                            gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Star)
                                .with_size(gpui_kit::px(32.0))
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child("登录后可查看收藏（书架与官方 App 同步）"),
                        )
                        .child(
                            Button::new("collections-retry-login")
                                .label("重试")
                                .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                        )
                        .into_any_element()
                }
                Phase::Failed(msg) => {
                    let detail = msg.clone();
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
                                .child("收藏列表加载失败"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(SharedString::from(detail)),
                        )
                        .child(
                            Button::new("collections-retry")
                                .label("重试")
                                .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                        )
                        .into_any_element()
                }
                Phase::Loaded if self.entries.is_empty() => {
                    let theme = cx.theme();
                    // Tauri Empty 组件是流内块（p-4 py-16，顶部对齐），不是
                    // 垂直居中——空态位置也要同构
                    v_flex()
                        .items_center()
                        .py_16()
                        .gap_2()
                        .child(
                            gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Star)
                                .with_size(gpui_kit::px(32.0))
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child("还没有收藏"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("在推荐页点 ☆ 收藏喜欢的剧，就会出现在这里"),
                        )
                        .into_any_element()
                }
                Phase::Loaded => {
                    let managing = self.managing;
                    let hovered = self.hovered.clone();
                    let visible = self.visible_entries(cx);
                    div()
                        .id("collections-grid")
                        .flex_1()
                        .overflow_y_scroll()
                        .p_4()
                        .child(h_flex().flex_wrap().items_start().gap_3().children(
                            visible.iter().map(|entry| {
                                let picked = self.selected.contains(&entry.series_id);
                                let is_hovered =
                                    hovered.as_deref() == Some(entry.series_id.as_str());
                                div().w(gpui_kit::px(172.0)).child(
                                    self.render_card(entry, managing, picked, is_hovered, cx),
                                )
                            }),
                        ))
                        .into_any_element()
                }
            };
        let toolbar = (matches!(self.phase, Phase::Loaded) && !self.entries.is_empty())
            .then(|| self.render_toolbar(cx));
        let batch_bar = self.managing.then(|| {
            let total = self.visible_entries(cx).len();
            BatchBar::new(self.selected.len(), total, self.deleting, "取消收藏")
                .on_toggle_all(cx.listener(|this, _, _, cx| {
                    let visible = this.visible_entries(cx);
                    let total = visible.len();
                    if total > 0 && this.selected.len() >= total {
                        this.selected.clear();
                    } else {
                        for entry in visible {
                            this.selected.insert(entry.series_id);
                        }
                    }
                    cx.notify();
                }))
                .on_delete(cx.listener(|this, _, window, cx| {
                    this.confirm_batch_remove(window, cx);
                }))
                .into_any_element()
        });
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .id("collections-page")
            .children(toolbar.map(|t| div().w_full().px_4().pt_4().child(t)))
            .child(body)
            .children(batch_bar)
    }
}
