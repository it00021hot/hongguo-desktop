//! 找剧页（Tauri `/browse` = `browse-page.tsx` 的 GPUI 对齐）。
//!
//! 顶部搜索框（含联想）+ 官方筛选面板（八行维度 + 客户端合成「状态」行，
//! 可整块折叠、已选计数徽标）+ 结果卡片网格（封面 3:4 + 已下载角标 +
//! 全 N 集角标 + 热度行 + 题材 chip）。浏览与搜索共用结果区：提交关键词
//! 原地切搜索结果，退出回筛选流；浏览流按 session 游标翻页。数据走
//! `hongguo_api::{discover, search}`，与 Tauri 的 browse/search command 同一段。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ElementId, Entity, ImageSource, ObjectFit, RenderImage, SharedString, Window, div, img,
};
use hongguo_api::discover::{BrowseFilters, FeedItem, SelectorRow};
use hongguo_api::search::{SearchResult, SuggestItem};
use hongguo_service::app_state::AppStateInner;

use crate::bridge;
use crate::home::fmt::format_play_count;

/// 找剧页发往壳的事件（壳订阅后切目的地/起播）。
#[derive(Debug, Clone)]
pub(crate) enum BrowseEvent {
    Play {
        series_id: String,
        vid_index: u32,
        start_secs: f64,
    },
}

/// 结果卡的归一化视图（浏览 `FeedItem` 与搜索 `SearchResult` 的字段在此拉平）。
struct CardView<'a> {
    series_id: &'a str,
    title: &'a str,
    cover_url: &'a str,
    episode_count: u32,
    heat: &'a str,
    tags: &'a [String],
}

/// 面板行类型 → 行头标签（Tauri FILTER_LABEL_KEYS 同款；未知类型回落
/// 服务端行名去掉「全部」前缀）。
fn row_label(row_type: &str, row_name: &str) -> String {
    match row_type {
        "genre" => "体裁".into(),
        "category_dim_theme" => "主题".into(),
        "category_dim_role" => "设定".into(),
        "category_dim_epoch" => "背景".into(),
        "sort" => "推荐".into(),
        "gender" => "受众".into(),
        "online_time" => "时间".into(),
        "duration" => "长度".into(),
        other => {
            let stripped = row_name.trim_start_matches("全部");
            if stripped.is_empty() {
                other.to_string()
            } else {
                stripped.to_string()
            }
        }
    }
}

/// 行类型 → `BrowseFilters` 的当前值。
fn filter_get(filters: &BrowseFilters, row_type: &str) -> String {
    match row_type {
        "genre" => filters.genre.clone(),
        "category_dim_theme" => filters.theme.clone(),
        "category_dim_role" => filters.role.clone(),
        "category_dim_epoch" => filters.epoch.clone(),
        "sort" => filters.sort.clone(),
        "gender" => filters.gender.clone(),
        "online_time" => filters.online_time.clone(),
        "duration" => filters.duration.clone(),
        _ => String::new(),
    }
}

/// 行类型 → `BrowseFilters` 的选中值写入。
fn filter_set(filters: &mut BrowseFilters, row_type: &str, value: String) {
    match row_type {
        "genre" => filters.genre = value,
        "category_dim_theme" => filters.theme = value,
        "category_dim_role" => filters.role = value,
        "category_dim_epoch" => filters.epoch = value,
        "sort" => filters.sort = value,
        "gender" => filters.gender = value,
        "online_time" => filters.online_time = value,
        "duration" => filters.duration = value,
        _ => {}
    }
}

pub(crate) struct BrowseView {
    state: Arc<AppStateInner>,
    keyword: Entity<InputState>,
    /// 已提交搜索词（空串 = 浏览模式）。
    submitted: String,
    filters: BrowseFilters,
    panel_rows: Option<Vec<SelectorRow>>,
    panel_failed: bool,
    filters_collapsed: bool,
    // —— 浏览流 ——
    feed_items: Vec<FeedItem>,
    feed_next_offset: i64,
    feed_session_id: String,
    feed_has_more: bool,
    feed_loading: bool,
    feed_failed: bool,
    // —— 搜索 ——
    search_items: Vec<SearchResult>,
    search_next_offset: i64,
    search_id: String,
    search_has_more: bool,
    search_loading: bool,
    search_failed: bool,
    // —— 联想（简化形态：输入框下方行式列表，点击即选）——
    suggest_items: Vec<SuggestItem>,
    suggest_seq: u64,
    // —— 封面与下载角标 ——
    covers: HashMap<String, Arc<RenderImage>>,
    covers_pending: HashSet<String>,
    downloaded: HashMap<String, u32>,
}

impl BrowseView {
    pub(crate) fn new(
        state: Arc<AppStateInner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let keyword = cx.new(|cx| InputState::new(window, cx).placeholder("输入剧名 / 关键词"));
        // 输入变化 → 更新联想（300ms 防抖在延迟任务里做）
        cx.subscribe(&keyword, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.refresh_suggest(cx);
                cx.notify();
            }
        })
        .detach();
        Self {
            state,
            keyword,
            submitted: String::new(),
            filters: BrowseFilters::default(),
            panel_rows: None,
            panel_failed: false,
            // 折叠态持久化（Tauri zustand browseFiltersCollapsed 同款）
            filters_collapsed: crate::ui_prefs::load().browse_filters_collapsed,
            feed_items: Vec::new(),
            feed_next_offset: 0,
            feed_session_id: String::new(),
            feed_has_more: false,
            feed_loading: false,
            feed_failed: false,
            search_items: Vec::new(),
            search_next_offset: 0,
            search_id: String::new(),
            search_has_more: false,
            search_loading: false,
            search_failed: false,
            suggest_items: Vec::new(),
            suggest_seq: 0,
            covers: HashMap::new(),
            covers_pending: HashSet::new(),
            downloaded: HashMap::new(),
        }
    }

    /// 进入页面：拉筛选面板 + 首屏浏览流 + 下载角标快照。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        self.refresh_downloaded(cx);
        if self.panel_rows.is_none() && !self.panel_failed {
            let state = Arc::clone(&self.state);
            let rx = bridge::spawn(async move {
                let env = state.api_env();
                hongguo_api::discover::fetch_browse_panel(&env).await
            });
            cx.spawn(async move |this, cx| {
                let result = rx.await.expect("tokio 桥不会丢弃发送端");
                this.update(cx, |view, cx| {
                    match result {
                        Ok(rows) => view.panel_rows = Some(rows),
                        Err(e) => {
                            log::warn!("[Browse] 筛选面板失败: {e}");
                            view.panel_failed = true;
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        if self.feed_items.is_empty() && !self.feed_loading {
            self.load_browse_page(cx);
        }
    }

    /// 下载角标：按剧聚合已完成任务数（本地快照，直读即刷）。
    fn refresh_downloaded(&mut self, cx: &mut Context<Self>) {
        let mut map: HashMap<String, u32> = HashMap::new();
        for task in self.state.queue().all() {
            if task.status == hongguo_common::model::TaskStatus::Completed {
                *map.entry(task.series_id).or_insert(0) += 1;
            }
        }
        self.downloaded = map;
        let _ = cx;
    }

    /// 拉一页浏览流（空 offset = 重开/换筛选；带游标 = 翻页）。
    fn load_browse_page(&mut self, cx: &mut Context<Self>) {
        if self.feed_loading {
            return;
        }
        self.feed_loading = true;
        self.feed_failed = false;
        cx.notify();
        let state = Arc::clone(&self.state);
        let filters = self.filters.clone();
        let offset = self.feed_next_offset;
        let session_id = self.feed_session_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::discover::fetch_browse(&filters, offset, &session_id, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            this.update(cx, |view, cx| {
                view.feed_loading = false;
                match result {
                    Ok(page) => {
                        log::info!("[Browse] GPUI 壳 -> {} 条", page.items.len());
                        if offset == 0 {
                            view.feed_items = page.items.clone();
                        } else {
                            view.feed_items.extend(page.items.clone());
                        }
                        view.feed_next_offset = page.next_offset;
                        view.feed_session_id = page.session_id.clone();
                        view.feed_has_more = page.has_more;
                        view.load_covers(
                            &page
                                .items
                                .iter()
                                .map(|i| i.cover.clone())
                                .collect::<Vec<_>>(),
                            cx,
                        );
                    }
                    Err(e) => {
                        log::warn!("[Browse] 浏览流失败: {e}");
                        view.feed_failed = true;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 重新按当前筛选拉首页（切筛选/退出搜索回浏览）。
    fn reload_browse(&mut self, cx: &mut Context<Self>) {
        self.feed_next_offset = 0;
        self.feed_session_id.clear();
        self.feed_has_more = false;
        self.load_browse_page(cx);
    }

    /// 提交搜索：App 综合 tab 搜索；首页精选不足 18 条时自动续拉一页。
    fn submit_search(&mut self, cx: &mut Context<Self>) {
        let value = self.keyword.read(cx).value().trim().to_string();
        if value.is_empty() {
            return;
        }
        self.submitted = value;
        self.suggest_items.clear();
        self.search_items.clear();
        self.search_next_offset = 0;
        self.search_id.clear();
        self.search_has_more = false;
        self.load_search_page(true, cx);
    }

    fn load_search_page(&mut self, first_page: bool, cx: &mut Context<Self>) {
        if self.search_loading {
            return;
        }
        self.search_loading = true;
        self.search_failed = false;
        cx.notify();
        let state = Arc::clone(&self.state);
        let query = self.submitted.clone();
        let offset = if first_page {
            0
        } else {
            self.search_next_offset
        };
        let search_id = if first_page {
            String::new()
        } else {
            self.search_id.clone()
        };
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::search::search_series(&query, offset, &search_id, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.search_loading = false;
                match result {
                    Ok(page) => {
                        if offset == 0 {
                            view.search_items = page.items.clone();
                        } else {
                            view.search_items.extend(page.items.clone());
                        }
                        view.search_next_offset = page.next_offset;
                        view.search_id = page.search_id.clone();
                        view.search_has_more = page.has_more;
                        view.load_covers(
                            &page
                                .items
                                .iter()
                                .map(|i| i.cover.clone())
                                .collect::<Vec<_>>(),
                            cx,
                        );
                        // 首页精选不足 18 条且还有下一页：自动续拉（Tauri 同款）
                        if first_page && page.has_more && view.search_items.len() < 18 {
                            view.load_search_page(false, cx);
                        }
                    }
                    Err(e) => {
                        log::warn!("[Browse] 搜索失败: {e}");
                        view.search_failed = true;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn exit_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.submitted.clear();
        self.keyword
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.suggest_items.clear();
        cx.notify();
    }

    /// 刷新联想（≥2 字才发；防抖 300ms，序号防乱序回填）。
    fn refresh_suggest(&mut self, cx: &mut Context<Self>) {
        let kw = self.keyword.read(cx).value().trim().to_string();
        self.suggest_seq += 1;
        let seq = self.suggest_seq;
        if kw.chars().count() < 2 || !self.submitted.is_empty() {
            self.suggest_items.clear();
            return;
        }
        let state = Arc::clone(&self.state);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(300))
                .await;
            let ok = this
                .update(cx, |view, _| view.suggest_seq == seq)
                .unwrap_or(false);
            if !ok {
                return; // 期间又敲了字/视图销毁
            }
            let rx = bridge::spawn(async move {
                let env = state.api_env();
                hongguo_api::search::search_suggest(&kw, &env).await
            });
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                if view.suggest_seq != seq {
                    return; // 过期结果丢弃
                }
                view.suggest_items = result.unwrap_or_default();
                cx.notify();
            });
        })
        .detach();
    }

    fn pick_suggest(&mut self, item: SuggestItem, window: &mut Window, cx: &mut Context<Self>) {
        self.suggest_items.clear();
        if !item.series_id.is_empty() {
            self.play_series(item.series_id.clone(), cx);
            return;
        }
        self.keyword.update(cx, |input, cx| {
            input.set_value(item.word.clone(), window, cx)
        });
        self.submitted = item.word;
        self.load_search_page(true, cx);
        cx.notify();
    }

    /// 逐条取封面（HEIC 走同一段转码阶梯；键 = 远端 URL）。
    fn load_covers(&mut self, urls: &[String], cx: &mut Context<Self>) {
        for url in urls {
            let url = url.clone();
            if url.is_empty() || !self.covers_pending.insert(url.clone()) {
                continue;
            }
            let rx = crate::cover_img::fetch(url.clone());
            cx.spawn(async move |this, cx| {
                let bytes = rx.await.expect("tokio 桥不会丢弃发送端");
                this.update(cx, |view, cx| {
                    view.covers_pending.remove(&url);
                    if let Ok(bytes) = bytes
                        && let Ok(decoded) = image::load_from_memory(&bytes)
                    {
                        view.covers.insert(
                            url.clone(),
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
        cx.emit(BrowseEvent::Play {
            series_id,
            vid_index,
            start_secs,
        });
    }

    fn active_filter_count(&self) -> usize {
        [
            &self.filters.genre,
            &self.filters.theme,
            &self.filters.role,
            &self.filters.epoch,
            &self.filters.sort,
            &self.filters.gender,
            &self.filters.online_time,
            &self.filters.duration,
            &self.filters.creation_status,
        ]
        .iter()
        .filter(|v| !v.is_empty())
        .count()
    }

    // —— 渲染 ——

    fn render_search_row(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let searching = !self.submitted.is_empty();
        let total = if searching {
            self.search_items.len()
        } else {
            self.feed_items.len()
        };
        v_flex()
            .w_full()
            .gap_1()
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w(gpui_kit::px(256.0))
                            .child(Input::new(&self.keyword)),
                    )
                    .child(
                        Button::new("browse-submit")
                            .icon(gpui_kit::assets::IconName::Search)
                            .label("搜索")
                            .disabled(self.keyword.read(cx).value().trim().is_empty())
                            .on_click(cx.listener(|this, _, _, cx| this.submit_search(cx))),
                    )
                    .children(searching.then(|| {
                        Button::new("browse-exit-search")
                            .icon(gpui_kit::assets::IconName::X)
                            .label("退出搜索")
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.exit_search(window, cx);
                            }))
                    }))
                    .child(div().flex_1())
                    .children((!searching && total > 0).then(|| {
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(format!("共 {total} 部")))
                    })),
            )
            // 联想（简化形态：行式列表，点击即选）
            .children((!self.suggest_items.is_empty()).then(|| {
                v_flex()
                    .w_full()
                    .max_w(gpui_kit::px(560.0))
                    .rounded(gpui_kit::px(8.0))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.group_box)
                    .p_1p5()
                    .gap_0p5()
                    .children(self.suggest_items.iter().take(8).map(|item| {
                        let item = item.clone();
                        let click_item = item.clone();
                        h_flex()
                            .id(ElementId::from(format!("browse-suggest-{}", item.word)))
                            .items_center()
                            .gap_2()
                            .rounded(gpui_kit::px(6.0))
                            .px_2()
                            .py_1()
                            .text_sm()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.secondary))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.pick_suggest(click_item.clone(), window, cx);
                            }))
                            .child(
                                gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Search)
                                    .small()
                                    .text_color(theme.muted_foreground),
                            )
                            .child(SharedString::from(item.word.clone()))
                            .children((!item.abstract_text.is_empty()).then(|| {
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .truncate()
                                    .child(SharedString::from(item.abstract_text.clone()))
                            }))
                    }))
            }))
            .into_any_element()
    }

    fn render_filter_panel(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let theme = cx.theme();
        let rows = self.panel_rows.as_ref()?;
        let mut elements: Vec<gpui_kit::AnyElement> = Vec::new();
        for row in rows {
            let current = filter_get(&self.filters, &row.row_type);
            let mut pills: Vec<gpui_kit::AnyElement> = vec![pill(
                cx,
                ElementId::from(format!("browse-all-{}", row.row_type)),
                "全部".to_string(),
                current.is_empty(),
                {
                    let row_type = row.row_type.clone();
                    cx.listener(move |this, _, _, cx| {
                        if filter_get(&this.filters, &row_type).is_empty() {
                            return;
                        }
                        filter_set(&mut this.filters, &row_type, String::new());
                        this.reload_browse(cx);
                    })
                },
            )];
            for item in &row.items {
                let selected = current == item.id;
                let row_type = row.row_type.clone();
                let id = item.id.clone();
                pills.push(pill(
                    cx,
                    ElementId::from(format!("browse-{}-{}", row.row_type, item.id)),
                    item.name.clone(),
                    selected,
                    cx.listener(move |this, _, _, cx| {
                        if filter_get(&this.filters, &row_type) == id {
                            return;
                        }
                        filter_set(&mut this.filters, &row_type, id.clone());
                        this.reload_browse(cx);
                    }),
                ));
            }
            elements.push(
                h_flex()
                    .items_start()
                    .gap_3()
                    .text_sm()
                    .child(
                        div()
                            .w(gpui_kit::px(40.0))
                            .flex_shrink_0()
                            .pt_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(row_label(&row.row_type, &row.row_name))),
                    )
                    .child(h_flex().flex_wrap().gap_1().py_0p5().children(pills))
                    .into_any_element(),
            );
        }
        // 客户端合成「状态」行（服务端 rows 不含；值走请求体顶层）
        {
            let current = self.filters.creation_status.clone();
            let mut pills: Vec<gpui_kit::AnyElement> = vec![pill(
                cx,
                "browse-all-creation".into(),
                "全部".to_string(),
                current.is_empty(),
                cx.listener(|this, _, _, cx| {
                    if this.filters.creation_status.is_empty() {
                        return;
                    }
                    this.filters.creation_status.clear();
                    this.reload_browse(cx);
                }),
            )];
            for (id, label) in [
                ("creation_status_0", "已完结"),
                ("creation_status_1", "连载中"),
            ] {
                let selected = current == id;
                let id_owned = id.to_string();
                pills.push(pill(
                    cx,
                    ElementId::from(format!("browse-creation-{id}")),
                    label.to_string(),
                    selected,
                    cx.listener(move |this, _, _, cx| {
                        if this.filters.creation_status == id_owned {
                            return;
                        }
                        this.filters.creation_status = id_owned.clone();
                        this.reload_browse(cx);
                    }),
                ));
            }
            elements.push(
                h_flex()
                    .items_start()
                    .gap_3()
                    .text_sm()
                    .child(
                        div()
                            .w(gpui_kit::px(40.0))
                            .flex_shrink_0()
                            .pt_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("状态"),
                    )
                    .child(h_flex().flex_wrap().gap_1().py_0p5().children(pills))
                    .into_any_element(),
            );
        }
        Some(
            v_flex()
                .w_full()
                .gap_1p5()
                .children(elements)
                .into_any_element(),
        )
    }

    /// 一张结果卡（浏览/搜索共用；字段差异在调用方已归一）。
    fn render_card(&self, card: CardView<'_>, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let CardView {
            series_id,
            title,
            cover_url,
            episode_count,
            heat,
            tags,
        } = card;
        let theme = cx.theme();
        let downloaded = self.downloaded.get(series_id).copied().unwrap_or(0);
        let cover = self.covers.get(cover_url).cloned();
        let tags: Vec<String> = tags
            .iter()
            .filter(|t| !t.is_empty())
            .take(3)
            .cloned()
            .collect();
        v_flex()
            .id(ElementId::from(format!("browse-card-{series_id}")))
            .w(gpui_kit::px(196.0))
            .overflow_hidden()
            .rounded(gpui_kit::px(12.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.group_box)
            .cursor_pointer()
            .hover(|s| s.border_color(theme.foreground))
            .on_click({
                let series_id = series_id.to_string();
                cx.listener(move |this, _, _, cx| {
                    this.play_series(series_id.clone(), cx);
                })
            })
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(gpui_kit::px(261.0))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .bg(theme.secondary)
                    .child(match cover {
                        Some(bitmap) => img(ImageSource::Render(bitmap))
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element(),
                        None => div().size_full().into_any_element(),
                    })
                    .children((downloaded > 0).then(|| {
                        div()
                            .absolute()
                            .top_2()
                            .right_2()
                            .child(Tag::success().child(SharedString::from(downloaded.to_string())))
                    }))
                    .children((episode_count > 0).then(|| {
                        div()
                            .absolute()
                            .bottom_2()
                            .left_2()
                            .rounded(gpui_kit::px(4.0))
                            .bg(gpui_kit::rgba(0x000000b3))
                            .px_1p5()
                            .py_0p5()
                            .text_xs()
                            .text_color(gpui_kit::rgb(0xffffffff))
                            .child(SharedString::from(format!("全 {episode_count} 集")))
                    })),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .p_3()
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(SharedString::from(title.to_string())),
                    )
                    .children((!heat.is_empty()).then(|| {
                        h_flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(
                                gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Flame)
                                    .small(),
                            )
                            .child(SharedString::from(heat.to_string()))
                    }))
                    .children((!tags.is_empty()).then(|| {
                        h_flex().flex_nowrap().gap_1().overflow_hidden().children(
                            tags.into_iter().map(|tag| {
                                div()
                                    .flex_shrink_0()
                                    .rounded(gpui_kit::px(4.0))
                                    .bg(theme.secondary)
                                    .px_1p5()
                                    .py_0p5()
                                    .text_size(gpui_kit::px(10.0))
                                    .child(SharedString::from(tag))
                            }),
                        )
                    })),
            )
            .into_any_element()
    }

    /// 预览种子（screenshot feature）：灌假面板/假结果/合成封面，跳过网络。
    /// `browse-search` = 搜索模式；`browse-empty` = 空态。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(&mut self, scene: &str, cx: &mut Context<Self>) {
        self.panel_rows = Some(crate::preview::fixtures::browse_panel_rows());
        let items = crate::preview::fixtures::browse_feed_items();
        if scene == "browse-search" {
            self.submitted = "命运".into();
            self.search_items = crate::preview::fixtures::search_results();
        }
        if scene != "browse-empty" {
            self.feed_items = items;
        }
        self.downloaded = HashMap::from([("7100001".to_string(), 3u32)]);
        for (i, item) in self.feed_items.iter().enumerate() {
            self.covers
                .insert(item.cover.clone(), crate::preview::fixtures::cover(i));
        }
        for (i, item) in self.search_items.iter().enumerate() {
            self.covers
                .insert(item.cover.clone(), crate::preview::fixtures::cover(6 + i));
        }
        cx.notify();
    }
}

impl gpui_kit::EventEmitter<BrowseEvent> for BrowseView {}

impl gpui_kit::Render for BrowseView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let searching = !self.submitted.is_empty();
        let search_row = self.render_search_row(cx);
        // 筛选区先建（内部有 cx 闭包，不能与后面取的 theme 借用交叠）
        let filter_section = if searching {
            None
        } else {
            let active_filters = self.active_filter_count();
            let filters_collapsed = self.filters_collapsed;
            let theme = cx.theme();
            Some(
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1p5()
                            .cursor_pointer()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .id("browse-filter-toggle")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.filters_collapsed = !this.filters_collapsed;
                                let mut prefs = crate::ui_prefs::load();
                                prefs.browse_filters_collapsed = this.filters_collapsed;
                                crate::ui_prefs::save(prefs);
                                cx.notify();
                            }))
                            .child(
                                gpui_kit::component::Icon::new(
                                    gpui_kit::assets::IconName::SlidersHorizontal,
                                )
                                .small(),
                            )
                            .child("筛选")
                            .children((active_filters > 0).then(|| {
                                Tag::secondary()
                                    .child(SharedString::from(active_filters.to_string()))
                            }))
                            .child(
                                gpui_kit::component::Icon::new(if filters_collapsed {
                                    gpui_kit::assets::IconName::ChevronDown
                                } else {
                                    gpui_kit::assets::IconName::ChevronUp
                                })
                                .small(),
                            ),
                    )
                    .children(
                        (!filters_collapsed)
                            .then(|| self.render_filter_panel(cx))
                            .flatten(),
                    )
                    .into_any_element(),
            )
        };
        let (loading, failed, empty, count) = if searching {
            (
                self.search_loading && self.search_items.is_empty(),
                self.search_failed,
                self.search_items.is_empty(),
                self.search_items.len(),
            )
        } else {
            (
                self.feed_loading && self.feed_items.is_empty(),
                self.feed_failed,
                self.feed_items.is_empty(),
                self.feed_items.len(),
            )
        };
        let grid: Vec<gpui_kit::AnyElement> = if searching {
            self.search_items
                .iter()
                .map(|item| {
                    let tags = if item.tags.is_empty() {
                        item.sub_title
                            .split('·')
                            .next()
                            .map(|s| vec![s.to_string()])
                            .unwrap_or_default()
                    } else {
                        item.tags.clone()
                    };
                    let heat = if item.heat_text.is_empty() && item.play_cnt > 0 {
                        format!("{}播放", format_play_count(item.play_cnt))
                    } else {
                        item.heat_text.clone()
                    };
                    self.render_card(
                        CardView {
                            series_id: &item.series_id,
                            title: &item.title,
                            cover_url: &item.cover,
                            episode_count: item.episode_cnt,
                            heat: &heat,
                            tags: &tags,
                        },
                        cx,
                    )
                })
                .collect()
        } else {
            self.feed_items
                .iter()
                .map(|item| {
                    self.render_card(
                        CardView {
                            series_id: &item.series_id,
                            title: &item.title,
                            cover_url: &item.cover,
                            episode_count: item.episode_cnt,
                            heat: &item.heat_text,
                            tags: &item.tags,
                        },
                        cx,
                    )
                })
                .collect()
        };
        let (searching_state, feed_state) = (searching, !searching);
        let _ = feed_state;
        let has_more = if searching_state {
            self.search_has_more
        } else {
            self.feed_has_more
        };
        let loading_more = if searching_state {
            self.search_loading
        } else {
            self.feed_loading
        };
        // 加载更多按钮先建（on_click 闭包要 &mut cx，不能与 theme 借用交叠）
        let more_button = has_more.then(|| {
            h_flex().w_full().justify_center().py_2().child(
                Button::new("browse-load-more")
                    .label(if loading_more {
                        "正在加载更多…"
                    } else {
                        "加载更多"
                    })
                    .ghost()
                    .small()
                    .disabled(loading_more)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.submitted.is_empty() {
                            this.load_browse_page(cx);
                        } else {
                            this.load_search_page(false, cx);
                        }
                    })),
            )
        });
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .id("browse-page")
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .p_6()
                    .gap_4()
                    .child(search_row)
                    .children(filter_section)
                    .children(failed.then(|| {
                        div()
                            .text_sm()
                            .text_color(theme.danger)
                            .child(if searching {
                                "搜索失败"
                            } else {
                                "加载失败"
                            })
                    }))
                    .child(if loading {
                        h_flex()
                            .w_full()
                            .justify_center()
                            .py_8()
                            .child(Spinner::new().small())
                            .into_any_element()
                    } else if empty {
                        v_flex()
                            .w_full()
                            .items_center()
                            .py_16()
                            .child(div().text_sm().text_color(theme.muted_foreground).child(
                                if searching {
                                    "换个关键词试试"
                                } else {
                                    "这个分类下暂时没有内容"
                                },
                            ))
                            .into_any_element()
                    } else {
                        h_flex()
                            .w_full()
                            .flex_wrap()
                            .items_start()
                            .gap_4()
                            .children(grid)
                            .into_any_element()
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
            )
    }
}

/// 行内筛选胶囊（Tauri ToggleGroupItem 形态：圆角全、选中主色描边）。
fn pill(
    cx: &App,
    id: ElementId,
    label: String,
    selected: bool,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui_kit::AnyElement {
    let theme = cx.theme();
    div()
        .id(id)
        .flex()
        .items_center()
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
        .px_3()
        .py_0p5()
        .text_xs()
        .cursor_pointer()
        .hover(|s| s.bg(theme.secondary))
        .on_click(on_click)
        .child(label)
        .into_any_element()
}
