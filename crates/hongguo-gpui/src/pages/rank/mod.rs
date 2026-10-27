//! 排行榜页（Tauri `/rank` = `rank-page.tsx` 的 GPUI 对齐）。
//!
//! 内容 tab（全部/真人剧/漫剧…由接口下发）× 左侧子榜竖排 × 右上筛选面板
//! （子榜自带 panel 行，单选；内联展开形态，Tauri 是弹出层）+ 榜单行卡
//! （名次 | 封面 | 标题/季徽/元信息/简介/热度行 | 播放+详情 / 预约）。
//! 数据走 `hongguo_api::rank::fetch_rank_ex`（selected/sub/panel/offset/
//! session 全参），与 Tauri 的 rank_list command 同一段。

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, SharedString, Window, div};
use hongguo_api::rank::{RankItem, RankTab};
use hongguo_service::app_state::AppStateInner;

use crate::bridge;
use crate::detail::DetailPrefill;
use crate::ui_button::button as Button;

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
    /// 响应下发的版本戳，翻页回传（官方协议）。
    rank_version: String,
    has_more: bool,
    loading: bool,
    failed: bool,
    /// 换键时有首拉在途（load_page 的单飞闸会吞掉直接调用）：登记待重拉，
    /// 在途响应落定后由 render 补发。
    needs_reload: bool,
    /// 非展示键的列表快照（Tauri rank query 按键缓存同口径）：换 tab/子榜/
    /// 筛选时旧态存进来、新键有缓存就立刻恢复展示；每次切换仍会后台重拉
    /// （staleTime 0），拉到整替。没有这份缓存，切 tab 只能清空 + 整页转圈。
    pages: HashMap<(String, String, String), RankListPage>,
    /// 筛选面板展开（内联形态）。
    filter_open: bool,
    /// 逐行预约乐观态（条目自带 reserved 起态）。
    reserved_local: HashMap<String, bool>,
    reserving: std::collections::HashSet<String>,
    /// 榜单行虚拟列表（`list()` 变高虚拟化）：可见行才参与布局/解码/绘制。
    /// 末位固定一个 footer 槽（加载更多/到底提示），随内容翻面。
    rows: crate::pages::list_common::VirtualRows,
}

/// 一个榜单键（selected × sub × panel）的完整列表态。
struct RankListPage {
    items: Vec<RankItem>,
    next_offset: i64,
    session_id: String,
    has_more: bool,
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
            needs_reload: false,
            pages: HashMap::new(),
            filter_open: false,
            reserved_local: HashMap::new(),
            reserving: std::collections::HashSet::new(),
            rows: crate::pages::list_common::VirtualRows::new(),
        }
    }

    /// 当前展示键。
    fn list_key(&self) -> (String, String, String) {
        (self.selected.clone(), self.sub.clone(), self.panel.clone())
    }
}

impl gpui_kit::EventEmitter<RankEvent> for RankView {}
