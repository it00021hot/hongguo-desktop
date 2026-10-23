//! 观看历史页（Tauri `/history` = `history-page.tsx` 的 GPUI 对齐）。
//!
//! **双模式**（Tauri issue #10 同款）：登录 = 云端观看记录
//! （read_history/list，book_type=2 短剧过滤，跨设备）；未登录 = 本地
//! 观看记录（播放时已落库，删除只动本机，页面顶部给「未登录」提示行）。
//! 行式列表（封面 + 标题 + 进度徽标 + 相对时间 + 继续播放），顶部
//! 「全部/未看完/已看完」筛选与标题搜索，管理模式下多选批量删除
//! （云端一条请求；本地逐剧删）。云端数据与操作走 `hongguo_api::history`，
//! 本地走 `hongguo_store`，与 Tauri 的 history command 同一段。

use std::collections::BTreeSet;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{App, ElementId, Entity, SharedString, Window, div};
use hongguo_api::history::WatchHistoryItem;
use hongguo_common::model::LocalHistoryItem;
use hongguo_service::app_state::AppStateInner;

use crate::bridge;
use crate::home::fmt::format_duration;
use crate::pages::list_common::{BatchBar, pick_dot, search_box};

mod actions;
mod list;
mod render;
mod rows;

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

enum Phase {
    Loading,
    Failed(String),
    Loaded,
}

pub(crate) struct HistoryView {
    state: Arc<AppStateInner>,
    phase: Phase,
    items: Vec<WatchHistoryItem>,
    search: Entity<InputState>,
    tab: HistoryTab,
    managing: bool,
    selected: BTreeSet<String>,
    deleting: bool,
    loading: bool,
    /// 渐进渲染：首批 20 行，滚到底自动 +40（Tauri 同款哨兵；按钮留作兜底），
    /// 语义相同、列表长度不设上限。
    visible_count: usize,
    /// 滚动哨兵：滚到底自动续载。
    scroll: crate::pages::scroll_load::ScrollLoad,
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
            search,
            tab: HistoryTab::All,
            managing: false,
            selected: BTreeSet::new(),
            deleting: false,
            loading: false,
            visible_count: 20,
            scroll: crate::pages::scroll_load::ScrollLoad::new(),
            preview_logged_in: false,
        }
    }

    /// 登录态（settings 里有账号即已登录，与首页同口径）。
    fn logged_in(&self) -> bool {
        self.state.settings().account.is_some() || self.preview_logged_in
    }
}

impl gpui_kit::EventEmitter<HistoryEvent> for HistoryView {}
