//! 应用内剧集详情页（Tauri `/detail` 路由 = `series-detail-page.tsx` 的
//! GPUI 对齐）。
//!
//! 播放页点剧名、互动卡片进入。结构与 Tauri 一致：返回栏 → 头部（封面 +
//! 标题 + 评分/热度/追剧/播放统计行 + 季徽/全集数/题材标签 + 简介 + 动作行
//! 播放/收藏/点赞）→ 备案号 → 三 tab（选集 / 剧评 / 相关推荐）。数据全部
//! 走官方 App 接口（分集档案 / video_detail meta / 剧评分页 / plan 相关
//! 推荐）——「继续看第 N 集」的 N 按 Tauri 详情页 continueIndex 同口径三段
//! 决策：本地断点 >3 秒优先，云端观看历史 >3 秒兜底，本地有记录（≤3 秒）
//! 仍视作看到那一集从头播。
//!
//! 未上线剧：Tauri 靠来源页 prefill 合成 0 集档案走降级视图；GPUI 的入口
//! （首页/排行榜）只传 series_id 没有 prefill，解析失败一律落错误态 +
//! 重试。等壳有 prefill 通道再对齐（不影响已上线剧的主路径）。
//!
//! 页面间不互相持有：加载与播放入口经事件交壳裁决（Shell 订阅
//! [`DetailEvent`] 切目的地/调 feed 起播）。

mod actions;
mod header;
mod load;
#[cfg(feature = "screenshot")]
mod preview_seed;
mod related;
mod render;
mod reviews;
mod tabs;

use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{App, Entity, Window, div};
use hongguo_api::danmaku::CommentItem;
use hongguo_api::detail::RelatedSeries;
use hongguo_api::history::WatchHistoryItem;
use hongguo_api::interact::InteractionItem;
use hongguo_common::model::Series;
use hongguo_service::app_state::AppStateInner;

use crate::bridge;
use crate::ui_button::button as Button;

/// 详情页发往壳的事件（壳订阅后切目的地/起播）。
#[derive(Debug, Clone)]
pub(crate) enum DetailEvent {
    Back,
    Play {
        series_id: String,
        vid_index: u32,
        start_secs: f64,
    },
    /// 互动登录墙：与 Tauri 详情页 requireLogin 同序，提示后进设置页账号卡。
    OpenSettings,
    /// 点「出品方/演职员」芯片 → 创作者主页（壳切页；详情页实体保留，
    /// 返回时原地恢复）。
    OpenCreator(String),
}

/// 来源页（预约列表/榜单行）带来的档案快照。未上线剧解析分集必然失败
/// （平台无分集可给），带 prefill 时走「即将上线」降级视图——与 Tauri
/// `series-detail-page.tsx` 的 prefill/generateContext 同口径。
#[derive(Debug, Clone)]
pub(crate) struct DetailPrefill {
    pub title: String,
    pub cover: String,
    pub tags: Vec<String>,
    pub desc: String,
    /// 当前账号是否已预约（预约列表行带来；降级视图的预约按钮据此起态）。
    pub reserved: bool,
}

/// 下半区三个 tab（Tauri Tabs defaultValue="episodes" 同序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailTab {
    Episodes,
    Reviews,
    Related,
}

pub(crate) struct DetailView {
    state: Arc<AppStateInner>,
    series_id: String,
    /// 档案（resolve_series：库缓存优先，未命中拉官方接口并落库）。
    series: Option<Arc<Series>>,
    /// 详情 meta（video_detail：简介/追剧/播放/热度/季徽/题材标签/备案号）。
    meta: Option<hongguo_api::detail::SeriesMeta>,
    loading: bool,
    /// 档案装载失败（错误态 + 重试；Tauri ErrorState + ResolveButton 同位）。
    load_failed: bool,
    /// 未上线降级视图：prefill 合成 0 集档案（动作行换「即将上线 + 预约」）。
    prefill: Option<DetailPrefill>,
    upcoming: bool,
    /// 降级视图的预约态与本页按钮（已预约 = 按钮可再点取消）。
    reserved: bool,
    reserve_pending: bool,
    tab: DetailTab,
    // —— 剧评（fetch_series_comments_page 分页；评分摘要取自首页 extra）——
    reviews: Arc<Vec<CommentItem>>,
    reviews_total: i64,
    reviews_score: String,
    reviews_score_cnt: i64,
    /// 剧评标签统计（`extra.filter_tag`：评分卡里「爽剧 26」胶囊行）
    reviews_tag_stats: Vec<hongguo_api::danmaku::CommentTagStat>,
    reviews_cursor: String,
    reviews_has_more: bool,
    reviews_loading: bool,
    // —— 相关推荐（plan 接口两格）——
    related: Option<Arc<RelatedSeries>>,
    /// 相关推荐卡上的预约态（未上线剧的「预约」按钮；进入相关 tab 时拉取，
    /// Tauri `useReservations(false/true)` 两个 tab 都查的口径）
    related_reserved: std::collections::HashSet<String>,
    /// 相关推荐预约在途（防连点）
    related_reserve_pending: std::collections::HashSet<String>,
    /// 预约列表已拉过（会话内一次）
    related_reserve_loaded: bool,
    // —— 互动（收藏/点赞）：mget 回显列表是 best-effort，未命中按未互动 ——
    interact_items: Arc<Vec<InteractionItem>>,
    collect_pending: bool,
    digg_pending: bool,
    /// 追更态（跟随列表首页本地判成员；未登录/未查到 = None）与在途翻转。
    follow_is_on: Option<bool>,
    follow_pending: bool,
    /// 云端观看历史（登录可见；「继续看」的兜底源，Tauri 详情页同款）。
    history: Vec<WatchHistoryItem>,
    // —— 剧评发评框（Tauri ReviewComposer：评分 + 表情 + 文本）——
    /// 剧评输入框（懒建：DetailView::new 没有 window，首个 render 里补）。
    /// 剧评正文：多行文本域（Tauri ReviewComposer 的 Textarea 同款）
    review_input: Option<Entity<gpui_kit::component::input::TextareaState>>,
    /// 我的评分（1–5 星，0 = 未选——未选禁发，score 是剧评身份的一部分）。
    review_stars: u8,
    review_hover_stars: u8,
    review_emoji_open: bool,
    review_sending: bool,
    /// 发送成功后的清空（set_value 要 window，render 期补执行）。
    review_clear_input: bool,
    /// Enter 提交请求（订阅回调没有 window，转交 render 执行）。
    review_submit_pending: bool,
    // —— 剧评互动（Tauri ReviewList 同款：点赞 / 回复线程 / 删除）——
    /// 剧评点赞的本地乐观覆盖（key = 剧评 id）。
    review_digg_override: std::collections::HashMap<String, bool>,
    /// 剧评回复线程：review_id → 已拉回复（懒加载）与游标/展开/加载态。
    review_replies: std::collections::HashMap<String, Arc<Vec<hongguo_api::danmaku::ReplyItem>>>,
    review_replies_cursor: std::collections::HashMap<String, String>,
    review_replies_open: std::collections::HashSet<String>,
    review_replies_loading: std::collections::HashSet<String>,
    /// 回复目标（review_id, 回复「回复」时的 (reply_id, 名字)）。
    review_reply_target: Option<(String, Option<(String, String)>)>,
    review_reply_input: Option<Entity<gpui_kit::component::input::InputState>>,
    review_reply_clear: bool,
    review_reply_submit_pending: bool,
    review_reply_sending: bool,
    review_deleting: bool,
}

impl DetailView {
    pub(crate) fn new(state: Arc<AppStateInner>, _cx: &mut App) -> Self {
        Self {
            state,
            series_id: String::new(),
            series: None,
            meta: None,
            loading: false,
            load_failed: false,
            prefill: None,
            upcoming: false,
            reserved: false,
            reserve_pending: false,
            tab: DetailTab::Episodes,
            reviews: Arc::new(Vec::new()),
            reviews_total: 0,
            reviews_score: String::new(),
            reviews_score_cnt: 0,
            reviews_tag_stats: Vec::new(),
            reviews_cursor: String::new(),
            reviews_has_more: false,
            reviews_loading: false,
            related: None,
            related_reserved: std::collections::HashSet::new(),
            related_reserve_pending: std::collections::HashSet::new(),
            related_reserve_loaded: false,
            interact_items: Arc::new(Vec::new()),
            collect_pending: false,
            digg_pending: false,
            follow_is_on: None,
            follow_pending: false,
            history: Vec::new(),
            review_input: None,
            review_stars: 0,
            review_hover_stars: 0,
            review_emoji_open: false,
            review_sending: false,
            review_clear_input: false,
            review_submit_pending: false,
            review_digg_override: std::collections::HashMap::new(),
            review_replies: std::collections::HashMap::new(),
            review_replies_cursor: std::collections::HashMap::new(),
            review_replies_open: std::collections::HashSet::new(),
            review_replies_loading: std::collections::HashSet::new(),
            review_reply_target: None,
            review_reply_input: None,
            review_reply_clear: false,
            review_reply_submit_pending: false,
            review_reply_sending: false,
            review_deleting: false,
        }
    }
}

impl gpui_kit::EventEmitter<DetailEvent> for DetailView {}
