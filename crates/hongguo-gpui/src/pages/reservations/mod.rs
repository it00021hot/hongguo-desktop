//! 我的预约页（Tauri `/reservations` = `reservation-page.tsx` 的 GPUI 对齐）。
//!
//! 「已上线/待上线」两个 tab（带计数）+ 搜索 + 管理批量删除；行卡片 =
//! 封面（左下上线日期角标）+ 标题/评分/简介/标签/上线时间 + 动作列
//! （已上线 = 播放 + 详情；未上线 = 取消预约）。未登录给登录入口。
//! 数据与操作走 `hongguo_api::reservation`（fetch_reservations /
//! reserve_series / delete_reservations），与 Tauri 的预约 command 同一段。

use std::collections::{BTreeSet, HashSet};
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
use gpui_kit::{App, ElementId, Entity, SharedString, Window, div};
use hongguo_api::calendar::CalendarItem;
use hongguo_service::app_state::AppStateInner;

use crate::bridge;
use crate::detail::DetailPrefill;
use crate::pages::list_common::{BatchBar, pick_dot, search_box};

mod actions;
mod cards;
mod list;
mod render;

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
/// 列表单页懒翻页（issue #11：串行拉全曾让「已上线」首屏等 5–8 秒）——
/// 游标三件套记翻页状态，加载更多续拉。
#[derive(Default)]
struct TabData {
    items: Option<Vec<CalendarItem>>,
    total: i64,
    loading: bool,
    failed: Option<String>,
    /// 翻页游标（首页响应下发，翻页续传；空 = 首页请求不带）。
    cursor: String,
    /// 下一页 offset 与是否有下页（服务端下发）。
    next_offset: i64,
    has_more: bool,
    loading_more: bool,
}

impl TabData {
    /// 角标计数：服务端 total 优先，条数兜底（`*_total_count` 在部分
    /// 形态下缺失，解析层缺省 0）。
    fn count(&self) -> i64 {
        self.total
            .max(self.items.as_deref().map_or(0, |items| items.len() as i64))
    }

    /// 续拉追加（series_id 去重：相邻页可能重叠条目）。
    fn extend_page(&mut self, new_items: Vec<CalendarItem>) {
        let items = self.items.get_or_insert_with(Vec::new);
        for item in new_items {
            if items.iter().any(|i| i.series_id == item.series_id) {
                continue;
            }
            items.push(item);
        }
    }
}

pub(crate) struct ReservationView {
    state: Arc<AppStateInner>,
    /// true = 已上线 tab。
    online_tab: bool,
    online: TabData,
    offline: TabData,
    search: Entity<InputState>,
    managing: bool,
    selected: BTreeSet<String>,
    deleting: bool,
    /// 单条取消预约进行中（按钮转圈/禁用）。
    cancelling: HashSet<String>,
    /// 预览种子用的登录态开关（真实壳恒 false，以 settings 的 account 为准）。
    preview_logged_in: bool,
    /// 滚动哨兵：滚到底自动续页（两个 tab 共用一个滚动容器；按钮留兜底）。
    scroll: crate::pages::scroll_load::ScrollLoad,
}

impl ReservationView {
    /// 语言切换：刷新搜索框 placeholder，保留 tab 与输入。
    pub(crate) fn refresh_locale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |input, cx| {
            input.set_placeholder(crate::i18n::t("reservation.searchPlaceholder"), window, cx);
        });
        cx.notify();
    }

    pub(crate) fn new(
        state: Arc<AppStateInner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::i18n::t("reservation.searchPlaceholder"))
        });
        cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.on_search_changed(cx);
            }
        })
        .detach();
        Self {
            state,
            online_tab: true,
            online: TabData::default(),
            offline: TabData::default(),
            search,
            managing: false,
            selected: BTreeSet::new(),
            deleting: false,
            cancelling: HashSet::new(),
            preview_logged_in: false,
            scroll: crate::pages::scroll_load::ScrollLoad::new(),
        }
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

    fn current_mut(&mut self) -> &mut TabData {
        if self.online_tab {
            &mut self.online
        } else {
            &mut self.offline
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
}

impl gpui_kit::EventEmitter<ReservationEvent> for ReservationView {}
