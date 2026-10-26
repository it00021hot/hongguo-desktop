//! 收藏页（Tauri `/collections` = `collection-page.tsx` 的 GPUI 对齐）。
//!
//! 书架列表（bookshelf/video/list）：卡片网格（封面 3:4 + 标题 + 共 N 集），
//! 悬浮出「继续播放」与「取消收藏」；左上类型筛选（全部/真人剧/漫剧，按
//! 条目 content_type）+ 搜索框 + 管理模式（多选后一请求批量取消收藏）。
//! 数据与操作走 `hongguo_api::interact`（fetch_bookshelf / collect_series /
//! collect_series_batch），与 Tauri 的 bookshelf 系列 command 同一段。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::button::{ButtonVariant, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{App, ElementId, Entity, SharedString, Window, div};
use hongguo_api::interact::BookshelfEntry;
use hongguo_service::app_state::AppStateInner;

use crate::bridge;
use crate::pages::list_common::{BatchBar, fetch_series_visual, pick_dot, search_box};
use crate::ui_button::button as Button;

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

    fn label(self) -> String {
        match self {
            TypeTab::All => crate::i18n::t("collections.filterAll"),
            TypeTab::Real => crate::i18n::t("collections.filterReal"),
            TypeTab::Comic => crate::i18n::t("collections.filterComic"),
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
    /// 封面 URL（series_id → URL）：像素在进程级共享缓存里。
    covers: HashMap<String, String>,
    /// 在途的剧档案解析（series_id 去重）。
    resolving: HashSet<String>,
    hovered: Option<String>,
    search: Entity<InputState>,
    type_tab: TypeTab,
    managing: bool,
    selected: BTreeSet<String>,
    deleting: bool,
    loading: bool,
    /// 网格滚动句柄（常驻视图里）：离开再回来位置还在——对齐 Tauri
    /// `useScrollRestore('hongguo.collections.scroll')` 的会话级记忆。
    scroll: gpui_kit::ScrollHandle,
}

impl CollectionView {
    /// 语言切换：刷新搜索框 placeholder，保留筛选条件与输入。
    pub(crate) fn refresh_locale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |input, cx| {
            input.set_placeholder(crate::i18n::t("collections.searchPlaceholder"), window, cx);
        });
        cx.notify();
    }

    pub(crate) fn new(
        state: Arc<AppStateInner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::i18n::t("collections.searchPlaceholder"))
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
            entries: Vec::new(),
            titles: HashMap::new(),
            episode_counts: HashMap::new(),
            covers: HashMap::new(),
            resolving: HashSet::new(),
            hovered: None,
            search,
            type_tab: TypeTab::All,
            managing: false,
            selected: BTreeSet::new(),
            deleting: false,
            loading: false,
            scroll: gpui_kit::ScrollHandle::new(),
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
                            view.phase = if e.i18n_key() == "error.auth" {
                                Phase::LoginRequired
                            } else {
                                Phase::Failed(crate::i18n::app_error_text(&e))
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

    /// 逐条解析标题/集数/封面地址（书架条目只有 series_id）。
    fn load_visuals(&mut self, entries: &[BookshelfEntry], cx: &mut Context<Self>) {
        for entry in entries {
            let key = entry.series_id.clone();
            if key.is_empty() || !self.resolving.insert(key.clone()) {
                continue;
            }
            let rx = fetch_series_visual(Arc::clone(&self.state), entry.series_id.clone());
            cx.spawn(async move |this, cx| {
                let visual = rx.await.expect("tokio 桥不会丢弃发送端");
                this.update(cx, |view, cx| {
                    view.resolving.remove(&visual.series_id);
                    if !visual.title.is_empty() {
                        view.titles.insert(visual.series_id.clone(), visual.title);
                        view.episode_counts
                            .insert(visual.series_id.clone(), visual.episode_count);
                    }
                    if !visual.cover_url.is_empty() {
                        view.covers
                            .insert(visual.series_id.clone(), visual.cover_url);
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
                    window.push_notification(
                        Notification::success(crate::i18n::t("collections.removed")),
                        cx,
                    );
                    view.load(cx);
                }
                Err(e) => {
                    log::warn!("[Collections] 取消收藏失败: {e}");
                    let message = crate::i18n::app_error_text(&e);
                    window.push_notification(
                        Notification::error(crate::i18n::tf(
                            "common.failedWith",
                            &[("message", message.as_str())],
                        )),
                        cx,
                    );
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
                        log::warn!("[Collections] 批量取消收藏失败: {e}");
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

    fn confirm_batch_remove(&self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let count = self.selected.len();
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            let view = view.clone();
            dialog
                .title(crate::i18n::t("collections.batchRemoveTitle"))
                .description(crate::i18n::tf(
                    "batch.confirmBody",
                    &[("count", &count.to_string())],
                ))
                .ok_text(crate::i18n::t("common.confirm"))
                .cancel_text(crate::i18n::t("common.cancel"))
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
                crate::i18n::tf("collections.unresolved", &[("id", tail.as_str())])
            });
        let episode_line = match self.episode_counts.get(&series_id) {
            Some(n) if *n > 0 => {
                crate::i18n::tf("collections.episodeCount", &[("n", &n.to_string())])
            }
            _ => crate::i18n::t("collections.resolvingHint"),
        };
        let cover = match self.covers.get(&series_id) {
            Some(url) => crate::cover_img::card(url, crate::cover_img::CoverShape::Card),
            None => div().size_full().into_any_element(),
        };
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
                // 整卡可点 → 用 base::Button 承担 role/focus/Enter-Space（Tauri 侧
                // 是 `article role="button" tabIndex={0}`，同一语义）。库强制
                // flex row + 居中，这里要复原原 v_flex 的竖排：flex_col +
                // items_stretch（子件撑满卡宽，标题才会按卡宽折行）+ justify_start
                // （内容从顶上排）。
                BaseButton::new(ElementId::from(format!("collections-open-{}", series_id)))
                    .accessibility_label(title.clone())
                    .w_full()
                    .overflow_hidden()
                    .rounded(crate::tokens::radius::CARD)
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
                    .flex_col()
                    .items_stretch()
                    .justify_start()
                    // base Button 强制 line_height(relative(1.))，文字行盒比默认矮一
                    // 截；钉回默认行高才与原 div 像素一致。
                    .line_height(gpui_kit::phi())
                    // 键盘焦点没有默认焦点环：按 Tauri/shadcn 同款补 border-ring + 3px 外圈。
                    .focus_visible(|s| {
                        s.border_color(theme.ring)
                            .shadow(crate::tokens::focus_ring(theme))
                    })
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
                            // 封面顶两角跟着卡片圆角（Tauri：卡片 overflow-hidden
                            // + rounded-xl 把封面顶角裁圆，底角接文字区是方的）
                            .rounded_t(crate::tokens::radius::CARD)
                            .bg(theme.secondary)
                            .child(cover)
                            .children(play_open.then(|| {
                                h_flex()
                                    .absolute()
                                    .inset_0()
                                    .items_center()
                                    .justify_center()
                                    // 常驻播放遮罩：Tauri 是 hover 才上 bg-black/30，
                                    // 这边有续播点就一直压着，档位略深到 35%。
                                    .bg(crate::tokens::scrim(35))
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
                                            .child(crate::i18n::t("history.continue")),
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
                                    .child(SharedString::from(title.clone())),
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
                    title.clone(),
                    cx.listener(move |this, _, _, cx| this.toggle_selected(&sid, cx)),
                    cx,
                ))
            }))
            .children((hovered && !managing).then(|| {
                let sid = series_id.clone();
                Button::new(ElementId::from(format!("collections-remove-{}", series_id)))
                    .icon(gpui_kit::assets::IconName::StarOff)
                    .tooltip(crate::i18n::t("collections.remove"))
                    .ghost()
                    .small()
                    .absolute()
                    .top_2()
                    .right_2()
                    .bg(crate::tokens::scrim(60))
                    .text_color(crate::tokens::on_media(100))
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
            let url = format!("preview://collection/{i}");
            crate::preview::fixtures::put_preview_image(
                url.clone(),
                crate::preview::fixtures::cover(i),
            );
            self.covers.insert(entry.series_id.clone(), url);
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
        // 封面预热：排上转码（gpui 资产机制去重），成品图落盘后 gpui 刷新窗口。
        let cover_urls: Vec<String> = self.covers.values().cloned().collect();
        crate::cover_img::prewarm_covers(&cover_urls, cx);
        let body =
            match &self.phase {
                // 首拉加载：封面网格骨架铺在网格容器里（与真实网格同
                // padding / 同卡宽，数据到位不跳版）
                Phase::Loading => div()
                    .id("collections-skeleton")
                    .flex_1()
                    .overflow_y_scrollbar()
                    .p_4()
                    .child(crate::pages::skeleton::cover_grid(cx, 10, 172.0, 229.0))
                    .into_any_element(),
                Phase::LoginRequired => crate::pages::states::error_state(
                    cx,
                    Some(gpui_kit::assets::IconName::Star),
                    crate::i18n::t("collections.loginRequired"),
                    None,
                    Some((
                        "collections-retry-login",
                        crate::pages::states::retry_on(cx.entity(), |this, cx| this.load(cx)),
                    )),
                ),
                Phase::Failed(msg) => {
                    let detail = msg.clone();
                    crate::pages::states::error_state(
                        cx,
                        None,
                        crate::i18n::t("collections.loadFailed"),
                        Some(detail),
                        Some((
                            "collections-retry",
                            crate::pages::states::retry_on(cx.entity(), |this, cx| this.load(cx)),
                        )),
                    )
                }
                Phase::Loaded if self.entries.is_empty() => crate::pages::states::empty_state(
                    cx,
                    Some(gpui_kit::assets::IconName::Star),
                    crate::i18n::t("collections.empty"),
                    Some(crate::i18n::t("collections.emptyHint")),
                ),
                Phase::Loaded => {
                    let managing = self.managing;
                    let hovered = self.hovered.clone();
                    let visible = self.visible_entries(cx);
                    div()
                        .id("collections-grid")
                        .flex_1()
                        .track_scroll(&self.scroll)
                        .overflow_y_scrollbar()
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
            BatchBar::new(
                self.selected.len(),
                total,
                self.deleting,
                crate::i18n::t("collections.remove"),
            )
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
