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

mod cards;
mod feed;
mod filter;
// 预览种子整个模块只在 screenshot feature 下存在（同 detail/preview_seed.rs）。
#[cfg(feature = "screenshot")]
mod preview;
mod render;
mod search;
mod suggest;

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
    /// 滚动哨兵：滚到底自动续页（浏览流 / 搜索结果共用；按钮留作兜底）。
    scroll: crate::pages::scroll_load::ScrollLoad,
    // —— 联想（简化形态：输入框下方行式列表，点击即选）——
    suggest_items: Vec<SuggestItem>,
    suggest_seq: u64,
    /// 联想键盘导航（Tauri browse-page onKeyDown 同款）：高亮下标 + 开合态
    /// （Esc 收起；下次输入重开）。
    suggest_index: usize,
    suggest_open: bool,
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
        let keyword = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::i18n::t("search.placeholder"))
        });
        // 输入变化 → 更新联想（300ms 防抖在延迟任务里做）；聚焦重开、失焦
        // 收起（Tauri browse-page onFocus/onBlur → setSuggestOpen 同款）
        cx.subscribe(&keyword, |this, _, event: &InputEvent, cx| {
            match event {
                InputEvent::Change => {
                    // 一次新输入 = 重开联想并回到首条高亮
                    this.suggest_open = true;
                    this.suggest_index = 0;
                    this.refresh_suggest(cx);
                    cx.notify();
                }
                InputEvent::Focus => {
                    this.suggest_open = true;
                    cx.notify();
                }
                InputEvent::Blur => {
                    this.suggest_open = false;
                    cx.notify();
                }
                _ => {}
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
            scroll: crate::pages::scroll_load::ScrollLoad::new(),
            suggest_items: Vec::new(),
            suggest_seq: 0,
            suggest_index: 0,
            suggest_open: false,
            covers: HashMap::new(),
            covers_pending: HashSet::new(),
            downloaded: HashMap::new(),
        }
    }
}

impl gpui_kit::EventEmitter<BrowseEvent> for BrowseView {}
