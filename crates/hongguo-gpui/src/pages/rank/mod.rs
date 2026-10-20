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

mod list;
mod render;
mod rows;
mod tabs;

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
    /// 滚动哨兵：滚到底自动续页（按钮留作兜底）。
    scroll: crate::pages::scroll_load::ScrollLoad,
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
            scroll: crate::pages::scroll_load::ScrollLoad::new(),
        }
    }
}

impl gpui_kit::EventEmitter<RankEvent> for RankView {}
