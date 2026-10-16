//! 排行榜页（Tauri `/rank` = `rank-page.tsx` 的 GPUI 对齐）。
//!
//! 内容 tab（全部/真人剧/漫剧…由接口下发）× 左侧子榜竖排 × 右上筛选面板
//! （子榜自带 panel 行，单选；内联展开形态，Tauri 是弹出层）+ 榜单行卡
//! （名次 | 封面 | 标题/季徽/元信息/简介/热度行 | 播放+详情 / 预约）。
//! 数据走 `hongguo_api::rank::fetch_rank_ex`（selected/sub/panel/offset/
//! session 全参），与 Tauri 的 rank_list command 同一段。

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, ImageSource, ObjectFit, RenderImage, SharedString, Window, div, img};
use hongguo_api::rank::{RankItem, RankTab};
use hongguo_service::app_state::AppStateInner;

use crate::bridge;
use crate::detail::DetailPrefill;

/// 排行榜发往壳的事件。
#[derive(Debug, Clone)]
pub(crate) enum RankEvent {
    Play {
        series_id: String,
        vid_index: u32,
        start_secs: f64,
    },
    OpenDetail {
        series_id: String,
        prefill: DetailPrefill,
    },
}

/// 榜单 tab 归一（Tauri normalize-tabs.ts 同口径）：两级形态过滤掉无内容
/// 的 celebrity tab；一级（旧身份）形态把榜单折成子榜、平铺选项包成面板行。
fn normalize_tabs(tabs: &[RankTab]) -> Vec<RankTab> {
    if tabs.is_empty() {
        return Vec::new();
    }
    if tabs.iter().any(|t| t.id == "all") {
        return tabs
            .iter()
            .filter(|t| t.id != "ranklist_celebrity" && !t.subs.is_empty())
            .cloned()
            .collect();
    }
    vec![RankTab {
        id: "all".into(),
        name: "全部".into(),
        subs: tabs
            .iter()
            .map(|tab| hongguo_api::rank::RankSubList {
                id: tab.id.clone(),
                name: tab.name.clone(),
                description: String::new(),
                panel: if tab.subs.is_empty() {
                    Vec::new()
                } else {
                    vec![hongguo_api::rank::RankPanelRow {
                        name: "筛选".into(),
                        items: tab
                            .subs
                            .iter()
                            .map(|s| hongguo_api::rank::RankPanelItem {
                                id: s.id.clone(),
                                name: s.name.clone(),
                            })
                            .collect(),
                    }]
                },
                // 一级形态（老版本身份）没有月榜 schema
                background: Vec::new(),
            })
            .collect(),
    }]
}

pub(crate) struct RankView {
    state: Arc<AppStateInner>,
    /// 内容 tab id（响应下发前默认 all）。
    selected: String,
    /// 子榜 id。
    sub: String,
    /// 筛选面板选中项（空 = 总榜）。
    panel: String,
    raw_tabs: Vec<RankTab>,
    items: Vec<RankItem>,
    next_offset: i64,
    session_id: String,
    /// 响应下发的版本戳，翻页回传（2026-10-17 官方实流协议）。
    rank_version: String,
    has_more: bool,
    loading: bool,
    failed: bool,
    /// 筛选面板展开（内联形态）。
    filter_open: bool,
    /// 逐行预约乐观态（条目自带 reserved 起态）。
    reserved_local: HashMap<String, bool>,
    reserving: std::collections::HashSet<String>,
    covers: HashMap<String, Arc<RenderImage>>,
    covers_pending: std::collections::HashSet<String>,
}

impl RankView {
    pub(crate) fn new(state: Arc<AppStateInner>) -> Self {
        Self {
            state,
            selected: "all".into(),
            sub: "ranklist_hot_sc".into(),
            panel: String::new(),
            raw_tabs: Vec::new(),
            items: Vec::new(),
            next_offset: 0,
            session_id: String::new(),
            rank_version: String::new(),
            has_more: false,
            loading: false,
            failed: false,
            filter_open: false,
            reserved_local: HashMap::new(),
            reserving: std::collections::HashSet::new(),
            covers: HashMap::new(),
            covers_pending: std::collections::HashSet::new(),
        }
    }

    /// 拉榜单：首屏或换 tab/子榜/筛选（`more` = 翻页）。
    fn load_page(&mut self, more: bool, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.failed = false;
        cx.notify();
        let state = Arc::clone(&self.state);
        let selected = self.selected.clone();
        let sub = self.sub.clone();
        let panel = (!self.panel.is_empty()).then(|| self.panel.clone());
        let offset = if more { self.next_offset } else { 0 };
        let session = if more {
            self.session_id.clone()
        } else {
            String::new()
        };
        let rank_version = self.rank_version.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::rank::fetch_rank_ex(
                hongguo_api::rank::RankReq {
                    selected: &selected,
                    sub: &sub,
                    panel: panel.as_deref(),
                    background: None,
                    offset,
                    session_id: &session,
                    rank_version: &rank_version,
                },
                &env,
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            this.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok(page) => {
                        log::info!("[Rank] GPUI 壳 -> {} 条", page.items.len());
                        if more {
                            view.items.extend(page.items.clone());
                        } else {
                            view.items = page.items.clone();
                        }
                        // recommend_info.rank 是页内位次（每页 1~10、可重复，
                        // 2026-10-17 解包实锤），连续名次按拼接序号重算；
                        // rank=0 是无名次榜（预约榜）保持 0
                        for (idx, it) in view.items.iter_mut().enumerate() {
                            if it.rank > 0 {
                                it.rank = (idx + 1) as u32;
                            }
                        }
                        view.next_offset = page.next_offset;
                        view.session_id = page.session_id.clone();
                        // 版本戳全局记最新（官方客户端语义：跨 tab/子榜回传）
                        if !page.rank_version.is_empty() {
                            view.rank_version = page.rank_version.clone();
                        }
                        view.has_more = page.has_more;
                        if !page.tabs.is_empty() {
                            view.raw_tabs = page.tabs.clone();
                        }
                        view.load_covers(&page.items, cx);
                        // 子榜失效（换 tab/形态切换）：落到第一个子榜重拉一次
                        let tabs = normalize_tabs(&view.raw_tabs);
                        let needs_fallback = tabs
                            .iter()
                            .find(|t| t.id == view.selected)
                            .map(|t| !t.subs.iter().any(|s| s.id == view.sub))
                            .unwrap_or(false);
                        if needs_fallback
                            && !more
                            && let Some(first) = tabs
                                .iter()
                                .find(|t| t.id == view.selected)
                                .and_then(|t| t.subs.first())
                                .map(|s| s.id.clone())
                        {
                            view.sub = first;
                            view.panel.clear();
                            view.load_page(false, cx);
                            return;
                        }
                    }
                    Err(e) => {
                        log::warn!("[Rank] GPUI 壳失败: {e}");
                        view.failed = true;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 进入页面：有旧数据保留（刷新），否则首拉。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        if self.items.is_empty() && !self.loading {
            self.load_page(false, cx);
        }
    }

    fn switch_to(&mut self, selected: String, sub: String, cx: &mut Context<Self>) {
        self.selected = selected;
        self.sub = sub;
        self.panel.clear();
        self.items.clear();
        self.next_offset = 0;
        self.session_id.clear();
        self.has_more = false;
        self.load_page(false, cx);
        cx.notify();
    }

    fn pick_panel(&mut self, panel: String, cx: &mut Context<Self>) {
        self.panel = panel;
        self.items.clear();
        self.next_offset = 0;
        self.session_id.clear();
        self.has_more = false;
        self.filter_open = false;
        self.load_page(false, cx);
        cx.notify();
    }

    fn load_covers(&mut self, items: &[RankItem], cx: &mut Context<Self>) {
        for item in items {
            let key = item.cover.clone();
            if key.is_empty() || !self.covers_pending.insert(key.clone()) {
                continue;
            }
            let rx = crate::cover_img::fetch(key.clone());
            cx.spawn(async move |this, cx| {
                let bytes = rx.await.expect("tokio 桥不会丢弃发送端");
                this.update(cx, |view, cx| {
                    view.covers_pending.remove(&key);
                    if let Ok(bytes) = bytes
                        && let Ok(decoded) = image::load_from_memory(&bytes)
                    {
                        view.covers.insert(
                            key.clone(),
                            Arc::new(crate::image_upload::render_image(decoded.to_rgba8())),
                        );
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    fn play_series(&self, series_id: String, cx: &mut Context<Self>) {
        let (vid_index, start_secs) = hongguo_service::play_service::resume::resolve_entry(
            &self.state.store,
            &series_id,
            &[],
        )
        .ok()
        .unwrap_or((1, 0.0));
        cx.emit(RankEvent::Play {
            series_id,
            vid_index,
            start_secs,
        });
    }

    fn open_detail(&self, item: &RankItem, cx: &mut Context<Self>) {
        cx.emit(RankEvent::OpenDetail {
            series_id: item.series_id.clone(),
            prefill: DetailPrefill {
                title: item.title.clone(),
                cover: item.cover.clone(),
                tags: item.tags.clone(),
                desc: item.description.clone(),
                reserved: self.reserved_of(item),
            },
        });
    }

    fn reserved_of(&self, item: &RankItem) -> bool {
        self.reserved_local
            .get(&item.series_id)
            .copied()
            .unwrap_or(item.reserved)
    }

    /// 行内预约切换（乐观翻转，与 Tauri RankRow 同款）。
    fn toggle_reserve(&mut self, item: &RankItem, cx: &mut Context<Self>) {
        let sid = item.series_id.clone();
        if !self.reserving.insert(sid.clone()) {
            return;
        }
        let next = !self.reserved_of(item);
        cx.notify();
        let state = Arc::clone(&self.state);
        let sid_for_call = sid.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::reservation::reserve_series(&sid_for_call, next, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.reserving.remove(&sid);
                match result {
                    Ok(()) => {
                        view.reserved_local.insert(sid.clone(), next);
                        window.push_notification(
                            Notification::success(if next {
                                "预约成功"
                            } else {
                                "已取消预约"
                            }),
                            cx,
                        );
                    }
                    Err(e) => {
                        log::warn!("[Rank] 预约失败: {e}");
                        window.push_notification(Notification::error(format!("操作失败：{e}")), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    // —— 渲染 ——

    fn render_tabs_row(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        // 两级形态才显示内容 tab 行（旧一级形态只有一个合成 tab）
        if !self.raw_tabs.iter().any(|t| t.id == "all") {
            return None;
        }
        let tabs = normalize_tabs(&self.raw_tabs);
        Some(
            h_flex()
                .w_full()
                .flex_wrap()
                .items_center()
                .gap_2()
                .children(tabs.into_iter().map(|tab| {
                    let active = self.selected == tab.id;
                    let button = Button::new(format!("rank-tab-{}", tab.id))
                        .label(tab.name.clone())
                        .small();
                    let button = if active {
                        button.secondary()
                    } else {
                        button.ghost()
                    };
                    let first_sub = tab.subs.first().map(|s| s.id.clone()).unwrap_or_default();
                    let id = tab.id.clone();
                    button.on_click(cx.listener(move |this, _, _, cx| {
                        if this.selected == id {
                            return;
                        }
                        this.switch_to(id.clone(), first_sub.clone(), cx);
                    }))
                }))
                .into_any_element(),
        )
    }

    fn current_tab(&self) -> Option<RankTab> {
        normalize_tabs(&self.raw_tabs)
            .into_iter()
            .find(|t| t.id == self.selected)
    }

    fn render_sub_nav(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let subs = self.current_tab().map(|t| t.subs).unwrap_or_default();
        v_flex()
            .id("rank-sub-nav")
            .w(gpui_kit::px(144.0))
            .flex_shrink_0()
            .gap_1()
            .overflow_y_scroll()
            .children(subs.into_iter().map(|sub| {
                let active = self.sub == sub.id;
                let id = sub.id.clone();
                div()
                    .id(ElementId::from(format!("rank-sub-{}", sub.id)))
                    .w_full()
                    .rounded(gpui_kit::px(6.0))
                    .px_3()
                    .py_2()
                    .text_sm()
                    .cursor_pointer()
                    .text_color(if active {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .when(active, |s| s.bg(theme.secondary))
                    .hover(|s| s.bg(theme.secondary_hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.sub == id {
                            return;
                        }
                        let selected = this.selected.clone();
                        this.switch_to(selected, id.clone(), cx);
                    }))
                    .child(SharedString::from(sub.name))
            }))
            .into_any_element()
    }

    fn render_filter_panel(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let theme = cx.theme();
        let sub = self
            .current_tab()?
            .subs
            .into_iter()
            .find(|s| s.id == self.sub)?;
        if sub.panel.is_empty() {
            return None;
        }
        let value = self.panel.clone();
        let rows: Vec<gpui_kit::AnyElement> = sub
            .panel
            .into_iter()
            .map(|row| {
                let items: Vec<gpui_kit::AnyElement> = row
                    .items
                    .into_iter()
                    .map(|it| {
                        let selected = it.id == value;
                        let id = it.id.clone();
                        div()
                            .id(ElementId::from(format!("rank-panel-{}", it.id)))
                            .rounded_full()
                            .border_1()
                            .when(selected, |s| {
                                s.border_color(theme.primary)
                                    .bg(theme.secondary)
                                    .text_color(theme.primary)
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                            })
                            .when(!selected, |s| {
                                s.border_color(theme.border)
                                    .text_color(theme.muted_foreground)
                            })
                            .px_2p5()
                            .py_1()
                            .text_xs()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.secondary))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.pick_panel(id.clone(), cx);
                            }))
                            .child(SharedString::from(it.name))
                            .into_any_element()
                    })
                    .collect();
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(row.name)),
                    )
                    .child(h_flex().flex_wrap().gap_1p5().children(items))
                    .into_any_element()
            })
            .collect();
        Some(
            v_flex()
                .w_full()
                .gap_3()
                .rounded(gpui_kit::px(8.0))
                .border_1()
                .border_color(theme.border)
                .bg(theme.group_box)
                .p_3()
                .children(rows)
                .children((!value.is_empty()).then(|| {
                    h_flex().w_full().justify_end().child(
                        Button::new("rank-panel-reset")
                            .label("重置筛选")
                            .ghost()
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pick_panel(String::new(), cx);
                            })),
                    )
                }))
                .into_any_element(),
        )
    }

    fn render_row(&self, item: &RankItem, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let cover = self.covers.get(&item.cover).cloned();
        let reserved = self.reserved_of(item);
        let rank_color: gpui_kit::Hsla = if item.rank <= 3 {
            gpui_kit::rgba(0xfacc15ff).into() // 星语言（与详情页 star_color 同源）
        } else {
            theme.muted_foreground
        };
        let mut meta: Vec<String> = Vec::new();
        if !item.sub_title.is_empty() {
            meta.push(item.sub_title.clone());
        }
        if item.score > 0.0 {
            meta.push(format!("{:.1}", item.score));
        }
        meta.extend(item.tags.clone());
        let secondary: Vec<String> = item
            .secondary_infos
            .iter()
            .filter(|s| !s.is_empty() && **s != item.rec_text)
            .cloned()
            .collect();
        h_flex()
            .id(ElementId::from(format!("rank-row-{}", item.series_id)))
            .items_center()
            .gap_4()
            .rounded(gpui_kit::px(12.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.group_box)
            .p_3()
            .cursor_pointer()
            .hover(|s| s.border_color(theme.foreground))
            .on_click({
                let item = item.clone();
                cx.listener(move |this, _, _, cx| this.open_detail(&item, cx))
            })
            .children((item.rank > 0).then(|| {
                div()
                    .w_8()
                    .flex_shrink_0()
                    .text_center()
                    .text_2xl()
                    .font_weight(gpui_kit::FontWeight::BLACK)
                    .text_color(rank_color)
                    .child(SharedString::from(item.rank.to_string()))
            }))
            .child(
                div()
                    .w(gpui_kit::px(80.0))
                    .h(gpui_kit::px(107.0))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .rounded(gpui_kit::px(8.0))
                    .bg(theme.secondary)
                    .child(match cover {
                        Some(bitmap) => img(ImageSource::Render(bitmap))
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element(),
                        None => div().size_full().into_any_element(),
                    }),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .gap_1()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1p5()
                            .child(
                                div()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(SharedString::from(item.title.clone())),
                            )
                            .children((!item.season.is_empty()).then(|| {
                                div()
                                    .flex_shrink_0()
                                    .rounded(gpui_kit::px(4.0))
                                    .border_1()
                                    .border_color(theme.border)
                                    .px_1()
                                    .text_size(gpui_kit::px(10.0))
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(item.season.clone()))
                            })),
                    )
                    .children((!meta.is_empty()).then(|| {
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(meta.join(" · ")))
                    }))
                    .children((!item.description.is_empty()).then(|| {
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(item.description.clone()))
                    }))
                    .children(
                        (!item.rec_text.is_empty() || !secondary.is_empty()).then(|| {
                            h_flex()
                                .items_center()
                                .gap_2()
                                .text_xs()
                                .children((!item.rec_text.is_empty()).then(|| {
                                    h_flex()
                                        .flex_shrink_0()
                                        .items_center()
                                        .gap_0p5()
                                        // 推荐语（热度）走业务令牌（Tauri 榜单行 text-warm）
                                        .text_color(crate::tokens::warm())
                                        .child(
                                            gpui_kit::component::Icon::new(
                                                gpui_kit::assets::IconName::Flame,
                                            )
                                            .small(),
                                        )
                                        .child(SharedString::from(item.rec_text.clone()))
                                }))
                                .children((!secondary.is_empty()).then(|| {
                                    div()
                                        .truncate()
                                        .text_color(theme.muted_foreground)
                                        .child(SharedString::from(secondary.join("  ")))
                                }))
                        }),
                    ),
            )
            .child(
                v_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_1p5()
                    .children(if item.upcoming {
                        let item_clone = item.clone();
                        let pending = self.reserving.contains(&item.series_id);
                        vec![
                            Button::new(ElementId::from(format!(
                                "rank-reserve-{}",
                                item.series_id
                            )))
                            .icon(if reserved {
                                gpui_kit::assets::IconName::Check
                            } else {
                                gpui_kit::assets::IconName::Bell
                            })
                            .label(if reserved { "已预约" } else { "预约" })
                            .outline()
                            .small()
                            .disabled(pending)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.toggle_reserve(&item_clone, cx);
                            }))
                            .into_any_element(),
                        ]
                    } else {
                        let sid = item.series_id.clone();
                        let item_clone = item.clone();
                        vec![
                            Button::new(ElementId::from(format!("rank-play-{}", item.series_id)))
                                .icon(gpui_kit::assets::IconName::Play)
                                .label("播放")
                                .outline()
                                .small()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.play_series(sid.clone(), cx);
                                }))
                                .into_any_element(),
                            Button::new(ElementId::from(format!("rank-detail-{}", item.series_id)))
                                .label("详情")
                                .ghost()
                                .small()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.open_detail(&item_clone, cx);
                                }))
                                .into_any_element(),
                        ]
                    }),
            )
            .into_any_element()
    }

    /// 预览种子（screenshot feature）：灌假 tabs/子榜/榜单与合成封面。
    /// `rank-empty` = 空态分支；`rank-filter` = 筛选面板展开。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(&mut self, scene: &str, cx: &mut Context<Self>) {
        let empty = scene == "rank-empty";
        self.raw_tabs = crate::preview::fixtures::rank_tabs();
        self.selected = "all".into();
        self.sub = "ranklist_hot_sc".into();
        self.filter_open = scene == "rank-filter";
        self.loading = false;
        self.failed = false;
        self.items = if empty {
            Vec::new()
        } else {
            let mut items = crate::preview::fixtures::rank_items();
            if let Some(last) = items.last_mut() {
                last.upcoming = true;
                last.reserved = true;
            }
            items
        };
        for (i, item) in self.items.iter().enumerate() {
            self.covers
                .insert(item.cover.clone(), crate::preview::fixtures::cover(i));
        }
        cx.notify();
    }
}

impl gpui_kit::EventEmitter<RankEvent> for RankView {}

impl gpui_kit::Render for RankView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs_row = self.render_tabs_row(cx);
        let sub_nav = self.render_sub_nav(cx);
        let filter_panel = self
            .filter_open
            .then(|| self.render_filter_panel(cx))
            .flatten();
        let filter_button = self
            .current_tab()
            .and_then(|t| t.subs.into_iter().find(|s| s.id == self.sub));
        let has_panel = filter_button
            .as_ref()
            .map(|s| !s.panel.is_empty())
            .unwrap_or(false);
        let sub_name = filter_button
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        let sub_desc = filter_button
            .as_ref()
            .map(|s| s.description.clone())
            .unwrap_or_default();
        let panel_label = filter_button
            .as_ref()
            .and_then(|s| {
                s.panel
                    .iter()
                    .flat_map(|r| r.items.iter())
                    .find(|it| it.id == self.panel && !it.id.is_empty())
                    .map(|it| it.name.clone())
            })
            .unwrap_or_else(|| "总榜".to_string());
        let filter_open = self.filter_open;
        let filter_toggle = has_panel.then(|| {
            let theme = cx.theme();
            Button::new("rank-filter-toggle")
                .icon(gpui_kit::assets::IconName::SlidersHorizontal)
                .label(panel_label)
                .outline()
                .small()
                .text_color(if filter_open {
                    theme.primary
                } else {
                    theme.foreground
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.filter_open = !this.filter_open;
                    cx.notify();
                }))
        });
        let (loading, failed, empty, has_more, count) = (
            self.loading && self.items.is_empty(),
            self.failed && self.items.is_empty(),
            self.items.is_empty(),
            self.has_more,
            self.items.len(),
        );
        let more_button = has_more.then(|| {
            let loading_more = self.loading;
            h_flex().w_full().justify_center().py_2().child(
                Button::new("rank-more")
                    .label(if loading_more {
                        "正在加载更多…"
                    } else {
                        "加载更多"
                    })
                    .ghost()
                    .small()
                    .disabled(loading_more)
                    .on_click(cx.listener(|this, _, _, cx| this.load_page(true, cx))),
            )
        });
        let rows: Vec<gpui_kit::AnyElement> = self
            .items
            .iter()
            .map(|item| self.render_row(item, cx))
            .collect();
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .id("rank-page")
            .p_4()
            .gap_3()
            .children(tabs_row)
            .child(
                h_flex()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    // 禁 cross 居中：右侧列高于行高时会被整体居中、头部裁出顶
                    // （gpui 默认 items_center 的经典坑）
                    .items_stretch()
                    .gap_4()
                    .child(sub_nav)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .child(
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .min_w_0()
                                            .items_center()
                                            .gap_2()
                                            .text_sm()
                                            .text_color(theme.muted_foreground)
                                            .child(SharedString::from(sub_name))
                                            .children((!sub_desc.is_empty()).then(|| {
                                                div()
                                                    .text_xs()
                                                    .truncate()
                                                    .child(SharedString::from(sub_desc))
                                            })),
                                    )
                                    .children(filter_toggle),
                            )
                            .children(filter_panel)
                            .child(
                                div()
                                    .id("rank-list")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .child(
                                        v_flex()
                                            .w_full()
                                            .gap_2()
                                            .children(if loading {
                                                vec![
                                                    h_flex()
                                                        .w_full()
                                                        .justify_center()
                                                        .py_8()
                                                        .child(Spinner::new().small())
                                                        .into_any_element(),
                                                ]
                                            } else if failed {
                                                vec![
                                                    v_flex()
                                                        .w_full()
                                                        .items_center()
                                                        .py_16()
                                                        .gap_3()
                                                        .child(
                                                            div()
                                                                .text_sm()
                                                                .text_color(theme.muted_foreground)
                                                                .child("排行榜加载失败"),
                                                        )
                                                        .child(
                                                            Button::new("rank-retry")
                                                                .label("重试")
                                                                .on_click(cx.listener(
                                                                    |this, _, _, cx| {
                                                                        this.load_page(false, cx)
                                                                    },
                                                                )),
                                                        )
                                                        .into_any_element(),
                                                ]
                                            } else if empty {
                                                vec![
                                                    v_flex()
                                                        .w_full()
                                                        .items_center()
                                                        .py_16()
                                                        .child(
                                                            div()
                                                                .text_sm()
                                                                .text_color(theme.muted_foreground)
                                                                .child("暂无内容"),
                                                        )
                                                        .into_any_element(),
                                                ]
                                            } else {
                                                rows
                                            })
                                            .children(more_button)
                                            .children((!has_more && count > 0).then(|| {
                                                h_flex().w_full().justify_center().py_2().child(
                                                    div()
                                                        .text_sm()
                                                        .text_color(theme.muted_foreground)
                                                        .child("没有更多了"),
                                                )
                                            })),
                                    ),
                            ),
                    ),
            )
    }
}
