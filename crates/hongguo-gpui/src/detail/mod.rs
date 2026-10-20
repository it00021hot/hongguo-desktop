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

mod header;
mod related;
mod reviews;
mod tabs;

use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{App, Entity, RenderImage, Window, div};
use hongguo_api::danmaku::CommentItem;
use hongguo_api::detail::RelatedSeries;
use hongguo_api::history::WatchHistoryItem;
use hongguo_api::interact::InteractionItem;
use hongguo_common::model::Series;
use hongguo_service::app_state::AppStateInner;

use crate::bridge;
use crate::cover_img;

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
    /// 图片缓存（封面/相关推荐卡/剧评头像共用），键 = 远端 URL。
    images: std::collections::HashMap<String, Arc<RenderImage>>,
    images_pending: std::collections::HashSet<String>,
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
    /// 语言切换：刷新剧评/回复输入框 placeholder，保留草稿。
    pub(crate) fn refresh_locale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = &self.review_input {
            input.update(cx, |input, cx| {
                input.set_placeholder(crate::i18n::t("detail.commentPlaceholder"), window, cx);
            });
        }
        if let Some(input) = &self.review_reply_input {
            input.update(cx, |input, cx| {
                input.set_placeholder(crate::i18n::t("detail.replyToComment"), window, cx);
            });
        }
        cx.notify();
    }

    pub(crate) fn new(state: Arc<AppStateInner>, _cx: &mut App) -> Self {
        Self {
            state,
            series_id: String::new(),
            series: None,
            meta: None,
            images: std::collections::HashMap::new(),
            images_pending: std::collections::HashSet::new(),
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

    /// 装载一部剧（同剧已装载则跳过；换剧强制重载）。档案与装饰性数据
    /// （meta/剧评/相关推荐/互动态）并行各拉各的，互不拦页面——Tauri
    /// react-query 各 query 独立降级同口径。
    pub(crate) fn load(&mut self, series_id: String, cx: &mut Context<Self>) {
        self.load_with_prefill(series_id, None, cx);
    }

    /// 程序化切换下半区 tab（取证开关 `HONGGUO_DETAIL_TAB` 用）。
    pub(crate) fn select_tab(&mut self, tab: DetailTab, cx: &mut Context<Self>) {
        self.tab = tab;
        if tab == DetailTab::Related {
            self.load_related_reservations(cx);
        }
        cx.notify();
    }

    /// 相关推荐卡的预约态：预约列表（离线 + 在线两个 tab，Tauri ReserveButton
    /// 两处都查同口径）拉一次；未登录不拉（按钮点击走登录回执）。
    pub(super) fn load_related_reservations(&mut self, cx: &mut Context<Self>) {
        if self.related_reserve_loaded || !self.logged_in() {
            return;
        }
        self.related_reserve_loaded = true;
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            let offline = hongguo_api::reservation::fetch_reservations(false, &env).await;
            let online = hongguo_api::reservation::fetch_reservations(true, &env).await;
            let mut ids = std::collections::HashSet::new();
            for page in [offline, online].into_iter().flatten() {
                for item in page.items {
                    ids.insert(item.series_id);
                }
            }
            ids
        });
        cx.spawn(async move |this, cx| {
            let ids = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.related_reserved.extend(ids);
                cx.notify();
            });
        })
        .detach();
    }

    /// 相关推荐卡「预约/已预约」切换（Tauri ReserveButton 同款：乐观翻转、
    /// 失败回滚、成功 toast；按钮点击不触发整卡跳详情）。
    pub(super) fn toggle_related_reserve(
        &mut self,
        series_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.related_reserve_pending.contains(&series_id) {
            return;
        }
        if !self.logged_in() {
            window.push_notification(
                Notification::info(crate::i18n::t("player.interact.loginRequired")),
                cx,
            );
            return;
        }
        let next = !self.related_reserved.contains(&series_id);
        self.related_reserve_pending.insert(series_id.clone());
        if next {
            self.related_reserved.insert(series_id.clone());
        } else {
            self.related_reserved.remove(&series_id);
        }
        cx.notify();
        let state = Arc::clone(&self.state);
        let sid = series_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::reservation::reserve_series(&sid, next, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.related_reserve_pending.remove(&series_id);
                match result {
                    Ok(()) => window.push_notification(
                        Notification::success(if next {
                            crate::i18n::t("reservation.done")
                        } else {
                            crate::i18n::t("reservation.cancelled")
                        }),
                        cx,
                    ),
                    Err(e) => {
                        // 回滚乐观态
                        if next {
                            view.related_reserved.remove(&series_id);
                        } else {
                            view.related_reserved.insert(series_id.clone());
                        }
                        log::warn!("[Detail] 相关推荐预约失败: {e}");
                        let message = crate::i18n::app_error_text(&e);
                        window.push_notification(
                            Notification::error(crate::i18n::tf(
                                "common.failedWith",
                                &[("message", message.as_str())],
                            )),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 装载（可带来源页 prefill）：解析失败且有 prefill 时走「即将上线」
    /// 降级视图（prefill 合成 0 集档案，正常渲染流只换动作行）。
    pub(crate) fn load_with_prefill(
        &mut self,
        series_id: String,
        prefill: Option<DetailPrefill>,
        cx: &mut Context<Self>,
    ) {
        if self.loading || (self.series_id == series_id && self.series.is_some()) {
            return;
        }
        self.series_id = series_id.clone();
        self.series = None;
        self.meta = None;
        self.load_failed = false;
        self.upcoming = false;
        self.reserved = prefill.as_ref().map(|p| p.reserved).unwrap_or(false);
        self.reserve_pending = false;
        self.prefill = prefill.clone();
        self.tab = DetailTab::Episodes;
        self.reviews = Arc::new(Vec::new());
        self.reviews_total = 0;
        self.reviews_score.clear();
        self.reviews_score_cnt = 0;
        self.reviews_tag_stats.clear();
        self.reviews_cursor.clear();
        self.reviews_has_more = false;
        self.related = None;
        self.related_reserved.clear();
        self.related_reserve_pending.clear();
        self.related_reserve_loaded = false;
        self.interact_items = Arc::new(Vec::new());
        self.collect_pending = false;
        self.digg_pending = false;
        self.follow_is_on = None;
        self.follow_pending = false;
        self.history.clear();
        self.review_stars = 0;
        self.review_hover_stars = 0;
        self.review_emoji_open = false;
        self.review_sending = false;
        self.review_submit_pending = false;
        // 换剧清空输入框（懒建之后才有效；值随渲染期 set_value 落）
        self.review_clear_input = self.review_input.is_some();
        // 剧评互动状态随换剧清零
        self.review_digg_override.clear();
        self.review_replies.clear();
        self.review_replies_cursor.clear();
        self.review_replies_open.clear();
        self.review_replies_loading.clear();
        self.review_reply_target = None;
        self.review_reply_sending = false;
        self.review_reply_submit_pending = false;
        self.review_reply_clear = self.review_reply_input.is_some();
        self.review_deleting = false;
        self.loading = true;
        let state = Arc::clone(&self.state);
        let sid = series_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            let cached = state
                .store
                .series_by_id(&sid)?
                .filter(|s| !s.episodes.is_empty());
            // 未上线剧解析必然失败：有 prefill 就用它合成 0 集档案
            let (series, upcoming) = match cached {
                Some(s) => (s, false),
                None => {
                    match hongguo_service::series_service::resolver::resolve_series(&sid, &env)
                        .await
                    {
                        Ok(s) => {
                            state.store.upsert_series(&s)?;
                            (s, false)
                        }
                        Err(e) => match prefill.clone() {
                            Some(p) => (synthesize_upcoming_series(&sid, &p), true),
                            None => return Err(e),
                        },
                    }
                }
            };
            // 装饰性 meta 失败不拦页面（头部相应行不渲染，Tauri 同口径）
            let meta = hongguo_api::detail::fetch_series_meta(&sid, &env)
                .await
                .ok();
            Ok::<_, hongguo_common::AppError>((series, meta, upcoming))
        });
        // 云端观看历史（「继续看」兜底源；登录才有数据，匿名回空表）——
        // 与档案并行各拉各的，晚到只是一拍刷新按钮文案
        let state_history = Arc::clone(&self.state);
        let rx_history = bridge::spawn(async move {
            let env = state_history.api_env();
            hongguo_api::history::fetch_watch_history(0, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx_history.await.expect("tokio 桥不会丢弃发送端");
            if let Ok(page) = result {
                let _ = this.update(cx, |view, cx| {
                    view.history = page.items;
                    cx.notify();
                });
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok((series, meta, upcoming)) => {
                        view.load_image(series.cover.clone(), cx);
                        // 出品方/演职员头像随 meta 预取（头部芯片用；先收集
                        // URL 再改 view，避免同时借 self.meta）
                        let creator_avatars: Vec<String> = meta
                            .as_ref()
                            .map(|m| {
                                m.creators
                                    .iter()
                                    .map(|c| c.avatar.clone())
                                    .filter(|a| !a.is_empty())
                                    .collect()
                            })
                            .unwrap_or_default();
                        view.meta = meta;
                        for url in creator_avatars {
                            view.load_image(url, cx);
                        }
                        view.series = Some(Arc::new(series));
                        view.upcoming = upcoming;
                        if !upcoming {
                            // 头部统计行的评分来自剧评分页首页 extra——剧评
                            // 本来就要拉，这里一并驱动
                            view.load_reviews_first_page(cx);
                            view.load_related(cx);
                            view.load_interact_state(cx);
                            view.load_follow_state(cx);
                        }
                    }
                    Err(e) => {
                        log::warn!("[Detail] {series_id} 档案装载失败: {e}");
                        view.load_failed = true;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 「继续看」目标集与起点（Tauri `series-detail-page.tsx` 的
    /// continueIndex 同口径三段决策）：
    /// 1) 本地断点 >3 秒 → 那集那位置（第一真值，播放中 5 秒一写）；
    /// 2) 云端观看历史 >3 秒 → 云端那集那位置（本地 ≤3 秒也按没看过让位）；
    /// 3) 本地有记录（≤3 秒）→ 仍视作看到这一集，从头播（详情页特例）；
    /// 4) 都没有 → 第 1 集从头。
    fn resume_entry(&self) -> (u32, f64) {
        let local = self
            .state
            .store
            .series_last_position(&self.series_id)
            .ok()
            .flatten();
        let (idx, start, duration) = if let Some((idx, pos)) = &local
            && pos.is_resumable()
        {
            (*idx, pos.current_time, pos.duration)
        } else {
            let threshold_ms =
                (hongguo_common::model::playback::RESUME_THRESHOLD_SECS * 1000.0) as i64;
            if let Some(h) = self.history.iter().find(|h| h.series_id == self.series_id)
                && h.position_ms > threshold_ms
            {
                (
                    h.vid_index.max(1) as u32,
                    h.position_ms as f64 / 1000.0,
                    h.duration_ms as f64 / 1000.0,
                )
            } else if let Some((idx, _)) = local {
                (idx, 0.0, 0.0)
            } else {
                (1, 0.0, 0.0)
            }
        };
        // 剩余 <2s：这一集已经看完，绝不起播在尾部；还有下一集就直接给
        // 下一集 0s。末集由 play_from_detail 按已看完规则跳下一部。
        if duration > 0.0 && duration - start <= 2.0 {
            let count = self
                .series
                .as_ref()
                .map(|series| series.episodes.len() as u32)
                .unwrap_or(0);
            if idx < count {
                log::info!("[Detail] 第 {idx} 集剩余不足 2s → 下一集 {}", idx + 1);
                return (idx + 1, 0.0);
            }
        }
        (idx, start)
    }

    /// 登录与否（settings 里有账号即已登录，与首页互动栏同口径）。
    fn logged_in(&self) -> bool {
        self.state
            .settings()
            .account
            .map(|a| a.user_id)
            .is_some_and(|id| !id.is_empty())
    }

    /// 互动回显（收藏/点赞态）：mget 全量列表，best-effort（失败留空 =
    /// 全按未互动，Tauri useInteractionState 同口径）。登录后才拉。
    pub(super) fn load_interact_state(&mut self, cx: &mut Context<Self>) {
        if !self.logged_in() {
            return;
        }
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::interact::fetch_interaction_state(&env)
                .await
                .map(|s| s.items)
                .unwrap_or_default()
        });
        cx.spawn(async move |this, cx| {
            let items = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, _| {
                view.interact_items = Arc::new(items);
            });
        })
        .detach();
    }

    /// 懒加载一张远端图进 images 表（封面/头像/相关卡共用；管线与全屏
    /// 占位同源，状态各自持有）。
    fn load_image(&mut self, url: String, cx: &mut Context<Self>) {
        if url.is_empty()
            || self.images.contains_key(&url)
            || !self.images_pending.insert(url.clone())
        {
            return;
        }
        let rx = cover_img::fetch(url.clone());
        cx.spawn(async move |this, cx| {
            let bytes = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| match bytes {
                Ok(b) => match image::load_from_memory(&b) {
                    Ok(decoded) => {
                        view.images.insert(
                            url.clone(),
                            Arc::new(crate::image_upload::render_image(decoded.to_rgba8())),
                        );
                        cx.notify();
                    }
                    Err(e) => {
                        log::warn!("[Detail] 图片解码失败: {e}");
                        view.images_pending.remove(&url);
                    }
                },
                Err(e) => {
                    log::warn!("[Detail] 图片加载失败: {e}");
                    view.images_pending.remove(&url);
                }
            });
        })
        .detach();
    }

    // —— 渲染 ——
    fn render_page(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        if self.loading || self.series.is_none() {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .bg(theme.background)
                .text_color(theme.foreground)
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(if self.loading {
                            crate::i18n::t("common.loading")
                        } else if self.load_failed {
                            crate::i18n::t("common.loadFailed")
                        } else {
                            crate::i18n::t("common.empty")
                        }),
                )
                .children(self.load_failed.then(|| {
                    Button::new("detail-retry")
                        .label(crate::i18n::t("common.retry"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            let sid = this.series_id.clone();
                            // 强制重走装载：先清态再 load
                            this.series_id.clear();
                            this.load(sid, cx);
                        }))
                }))
                .into_any_element();
        }
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .id("detail-page")
            .overflow_y_scroll()
            .child(self.render_back_bar(cx))
            .child(
                div()
                    .max_w(gpui_kit::px(1152.0))
                    .mx_auto()
                    .w_full()
                    .px_8()
                    .pb_8()
                    .child(self.render_header(cx))
                    .child(self.render_record_line(cx))
                    .children(self.render_tabs(cx)),
            )
            .into_any_element()
    }

    /// 计数人性化（详情页头部精度）：万/亿带一位小数、整数省略小数点
    /// （Tauri formatCountPrecise / hgplayer 头部同口径）。
    fn fmt_count(n: i64) -> String {
        match crate::i18n::locale() {
            crate::i18n::Locale::ZhCn => {
                if n >= 100_000_000 {
                    return format!("{:.1}亿", n as f64 / 100_000_000.0).replace(".0", "");
                }
                if n >= 10_000 {
                    return format!("{:.1}万", n as f64 / 10_000.0).replace(".0", "");
                }
            }
            crate::i18n::Locale::EnUs => {
                if n >= 1_000_000_000 {
                    return format!("{:.1}B", n as f64 / 1_000_000_000.0).replace(".0", "");
                }
                if n >= 1_000_000 {
                    return format!("{:.1}M", n as f64 / 1_000_000.0).replace(".0", "");
                }
                if n >= 1_000 {
                    return format!("{:.1}K", n as f64 / 1_000.0).replace(".0", "");
                }
            }
        }
        format!("{n}")
    }

    /// 秒 → mm:ss（选集格时长角标；Tauri formatDuration 同形态）。
    fn fmt_duration(secs: i64) -> String {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

/// prefill → 0 集合成档案（未上线剧）。只填展示必需字段；分集为空，
/// 选集 tab 自然落「暂无分集信息」。
fn synthesize_upcoming_series(series_id: &str, prefill: &DetailPrefill) -> Series {
    Series {
        series_id: series_id.to_string(),
        title: prefill.title.clone(),
        cover: prefill.cover.clone(),
        episode_count: 0,
        followed_cnt: 0,
        tags: prefill.tags.clone(),
        episodes: Vec::new(),
        dismissed: false,
    }
}

impl gpui_kit::Render for DetailView {
    fn render(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        // 剧评输入框懒建：DetailView::new 拿不到 window（壳在事件回调里
        // cx.new 创建本视图），首个 render 才有——建一次并挂 Enter 提交。
        if self.review_input.is_none() {
            let input = cx.new(|cx| {
                gpui_kit::component::input::TextareaState::new(window, cx)
                    .placeholder(crate::i18n::t("detail.commentPlaceholder"))
                    // Enter 发表、Shift+Enter 换行（Tauri 富文本输入 onEnter 同款）
                    .submit_on_enter(true)
            });
            cx.subscribe(
                &input,
                |this, _, event: &gpui_kit::component::input::InputEvent, cx| {
                    if matches!(
                        event,
                        gpui_kit::component::input::InputEvent::PressEnter { .. }
                    ) {
                        // 订阅回调没有 window（登录墙/回执要它），转交 render 执行
                        this.review_submit_pending = true;
                    }
                    cx.notify();
                },
            )
            .detach();
            self.review_input = Some(input);
        }
        // 剧评回复输入框（同款懒建；Enter 提交走 pending 转交 render）
        if self.review_reply_input.is_none() {
            let input = cx.new(|cx| {
                gpui_kit::component::input::InputState::new(window, cx)
                    .placeholder(crate::i18n::t("detail.replyToComment"))
            });
            cx.subscribe(
                &input,
                |this, _, event: &gpui_kit::component::input::InputEvent, cx| {
                    if matches!(
                        event,
                        gpui_kit::component::input::InputEvent::PressEnter { .. }
                    ) {
                        this.review_reply_submit_pending = true;
                    }
                    cx.notify();
                },
            )
            .detach();
            self.review_reply_input = Some(input);
        }
        if let Some(input) = &self.review_input {
            if self.review_clear_input {
                self.review_clear_input = false;
                input.update(cx, |state, cx| state.set_value("", window, cx));
            }
            if self.review_submit_pending {
                self.review_submit_pending = false;
                self.submit_review(window, cx);
            }
        }
        if let Some(input) = &self.review_reply_input {
            if self.review_reply_clear {
                self.review_reply_clear = false;
                input.update(cx, |state, cx| state.set_value("", window, cx));
            }
            if self.review_reply_submit_pending {
                self.review_reply_submit_pending = false;
                self.submit_review_reply(window, cx);
            }
        }
        self.render_page(cx)
    }
}

/// 预览种子（screenshot feature）：离屏矩阵的详情页场景——灌固定档案 +
/// meta + 剧评 + 相关推荐 + 假封面，全程零网络。`tab` 选下半区初始 tab
///（"detail" = 选集，"detail-related" = 相关推荐；剧评 tab 与选集共用
/// 一张基准图即可覆盖，不单独出图）。
#[cfg(feature = "screenshot")]
impl DetailView {
    pub(crate) fn debug_preview_seed(&mut self, tab: &str, cx: &mut Context<Self>) {
        use crate::preview::fixtures;
        use hongguo_api::detail::{RelatedItem, RelatedSeries, SeriesMeta};

        self.loading = false;
        self.load_failed = false;
        if tab == "detail-upcoming" {
            // 未上线降级视图：prefill 合成 0 集档案（预约态起「预约」按钮）
            let prefill = DetailPrefill {
                title: "长风渡口".into(),
                cover: "preview://detail-cover".into(),
                tags: vec!["古装".into(), "权谋".into()],
                desc: "战乱年代，一介孤女在渡口撑起商行，卷入朝堂风云。".into(),
                reserved: true,
            };
            self.upcoming = true;
            self.reserved = prefill.reserved;
            self.prefill = Some(prefill.clone());
            self.series_id = "7200000".into();
            self.series = Some(Arc::new(synthesize_upcoming_series("7200000", &prefill)));
            self.images
                .insert("preview://detail-cover".into(), fixtures::cover(6));
            self.meta = None;
            self.tab = DetailTab::Episodes;
            cx.notify();
            return;
        }
        self.tab = if tab == "detail-related" {
            DetailTab::Related
        } else {
            DetailTab::Episodes
        };
        let series = Series {
            series_id: "7100000".into(),
            title: "命运契约".into(),
            cover: "preview://detail-cover".into(),
            episode_count: 80,
            tags: vec!["玄幻".into(), "逆袭".into(), "修真".into()],
            episodes: (1..=80)
                .map(|i| hongguo_common::model::Episode {
                    vid_index: i,
                    vid: format!("7100000:{i}"),
                    duration: 60 + (i as i64 % 7) * 9,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        self.series_id = series.series_id.clone();
        self.series = Some(Arc::new(series));
        let meta = SeriesMeta {
            hot_score: 37_860_000,
            followed_cnt: 271_000,
            play_cnt: 4_030_000,
            season: "第1季".into(),
            record_number: "京网文〔2026〕0000-000号".into(),
            intro: "落魄少年在一场契约游戏里觉醒了读档能力：每一次死亡都回到十分钟前。\
                      他靠着这个能力在豪门倾轧与修真秘境之间反复横跳，把仇人一个个熬成了路人。\
                      可他渐渐发现，所谓契约游戏的庄家，正是十分钟后的自己。"
                .into(),
            ..Default::default()
        };
        self.meta = Some(meta);
        self.images
            .insert("preview://detail-cover".into(), fixtures::cover(0));
        let mut reviews: Vec<CommentItem> = fixtures::comments();
        if let Some(last) = reviews.last_mut() {
            last.score = "9".into();
            last.score_suffix_text = "观看2小时后点评".into();
        }
        self.reviews = Arc::new(reviews.clone());
        self.reviews_total = 3716;
        self.reviews_score = "8.6".into();
        self.reviews_score_cnt = 12_000;
        self.reviews_tag_stats = vec![
            hongguo_api::danmaku::CommentTagStat {
                tag_name: "修仙世界观宏大".into(),
                count: 26,
            },
            hongguo_api::danmaku::CommentTagStat {
                tag_name: "爽".into(),
                count: 18,
            },
        ];
        let mut works = fixtures::related_seasons();
        for (i, item) in works.iter_mut().enumerate() {
            let url = format!("preview://related-{i}");
            self.images.insert(url.clone(), fixtures::cover(i + 1));
            item.cover = url;
        }
        let mut guess = vec![RelatedItem {
            series_id: "7200001".into(),
            title: "重生之商海风云".into(),
            score: 7.9,
            play_cnt: 8_800_000,
            episode_cnt: 66,
            ..Default::default()
        }];
        guess.push(RelatedItem {
            series_id: "7200002".into(),
            title: "闪婚老公是豪门".into(),
            play_cnt: 12_400_000,
            episode_cnt: 99,
            ..Default::default()
        });
        for (i, item) in guess.iter_mut().enumerate() {
            let url = format!("preview://guess-{i}");
            self.images.insert(url.clone(), fixtures::cover(i + 4));
            item.cover = url;
        }
        self.related = Some(Arc::new(RelatedSeries { works, guess }));
        self.interact_items = Arc::new(vec![InteractionItem {
            series_id: "7100000".into(),
            followed: true,
            ..Default::default()
        }]);
        cx.notify();
    }
}

impl gpui_kit::EventEmitter<DetailEvent> for DetailView {}
