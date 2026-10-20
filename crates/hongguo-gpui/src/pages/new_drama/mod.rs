//! 新剧页（Tauri `/new` = `new-drama-page.tsx` 的 GPUI 对齐）。
//!
//! 两个视图：新剧推荐（`firstonlinetime_new` 倒序流，频道筛选 全部/男频/
//! 女频，封面网格 + 翻页）与上新日历（日期条 + 当日上新行卡，未上线可
//! 预约）。视图 tab 按壳约定放主体区顶部（Tauri 在顶栏 portal 同位）。
//! 点击分流：未上线进详情（带 prefill 走「即将上线」降级视图），已上线
//! 直接起播。数据走 `hongguo_api::{new_drama, calendar}`，与 Tauri 的
//! new_drama 系列 command 同一段。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::RenderImage;
use hongguo_api::calendar::CalendarItem;
use hongguo_api::rank::RankItem;
use hongguo_service::app_state::AppStateInner;

use crate::detail::DetailPrefill;

mod calendar;
mod cards;
mod feed;
mod filter;
mod view;

/// 新剧页发往壳的事件（起播 / 带 prefill 进详情）。
#[derive(Debug, Clone)]
pub(crate) enum NewDramaEvent {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewTab {
    Recommend,
    Calendar,
}

pub(crate) struct NewDramaView {
    state: Arc<AppStateInner>,
    view: ViewTab,
    gender: i64,
    // —— 推荐流 ——
    items: Vec<RankItem>,
    next_offset: i64,
    session_id: String,
    has_more: bool,
    loading: bool,
    failed: bool,
    // —— 日历 ——
    cal_dates: Vec<String>,
    cal_selected: String,
    cal_items: Vec<CalendarItem>,
    cal_next_offset: i64,
    cal_session_id: String,
    cal_has_more: bool,
    cal_loading: bool,
    cal_failed: bool,
    cal_loaded: bool,
    /// 日历本地预约态（日历响应 has_subscribed 恒 false，本地记已点过的剧）。
    reserved_local: HashSet<String>,
    covers: HashMap<String, Arc<RenderImage>>,
    covers_pending: HashSet<String>,
    downloaded: HashMap<String, u32>,
    /// 滚动哨兵：推荐流与日历各一条（两个滚动容器互不复用；按钮留作兜底）。
    scroll_recommend: crate::pages::scroll_load::ScrollLoad,
    scroll_calendar: crate::pages::scroll_load::ScrollLoad,
}

impl NewDramaView {
    pub(crate) fn new(state: Arc<AppStateInner>) -> Self {
        // 取证开关（HONGGUO_NEW_TAB=calendar）：直接落在上新日历视图
        let start_calendar = std::env::var("HONGGUO_NEW_TAB").is_ok_and(|v| v == "calendar");
        Self {
            state,
            view: if start_calendar {
                ViewTab::Calendar
            } else {
                ViewTab::Recommend
            },
            gender: 2,
            items: Vec::new(),
            next_offset: 0,
            session_id: String::new(),
            has_more: false,
            loading: false,
            failed: false,
            cal_dates: Vec::new(),
            cal_selected: String::new(),
            cal_items: Vec::new(),
            cal_next_offset: 0,
            cal_session_id: String::new(),
            cal_has_more: false,
            cal_loading: false,
            cal_failed: false,
            cal_loaded: false,
            reserved_local: HashSet::new(),
            covers: HashMap::new(),
            covers_pending: HashSet::new(),
            downloaded: HashMap::new(),
            scroll_recommend: crate::pages::scroll_load::ScrollLoad::new(),
            scroll_calendar: crate::pages::scroll_load::ScrollLoad::new(),
        }
    }
}

impl gpui_kit::EventEmitter<NewDramaEvent> for NewDramaView {}

/// 日历条目的日期归属（"20261014" 形态；解析 `publish_time` 本地日期）。
fn selected_date_of(item: &CalendarItem) -> String {
    use chrono::{Datelike, TimeZone};
    match chrono::Local
        .timestamp_millis_opt(item.publish_time)
        .single()
    {
        Some(dt) => format!("{:04}{:02}{:02}", dt.year(), dt.month(), dt.day()),
        None => String::new(),
    }
}
