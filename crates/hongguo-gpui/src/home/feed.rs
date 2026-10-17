//! 沉浸流的数据面：顶栏 tab 源（服务端下发 + 回落三件套）、「你的内容」
//! 种子、渲染/交互共用的展示列表与游标。

use std::collections::HashSet;
use std::sync::Arc;

use gpui_kit::prelude::*;
use hongguo_api::discover::FeedItem;
use hongguo_api::history::fetch_watch_history;
use hongguo_api::interact::fetch_bookshelf;
use hongguo_api::recommend::{fetch_feed_tabs, fetch_recommend_feed};

use crate::bridge;

use super::NowPlaying;

/// 沉浸流的 tab 身份（即书城 cell 换一换的 tab_type 契约）。列表由服务端
/// 下发（[`Self::load_tabs`]），回落三件套见 [`Self::fallback_tabs`]。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Source(Arc<str>);

impl Source {
    /// 推荐 tab（种子语义锚点 + 回落表的默认选中）。
    pub(crate) fn recommend() -> Self {
        Self("16".into())
    }

    /// 服务端列表到手前的回落三件套（抓包锁定：推荐16/漫剧36/真人剧39）。
    pub(crate) fn fallback_tabs() -> [Self; 3] {
        [Self("16".into()), Self("36".into()), Self("39".into())]
    }

    pub(crate) fn new(tab_type: &str) -> Self {
        Self(tab_type.into())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// 回落名（服务端标题缺失时显示；16/36/39 有本地名，未知 tab 显示 tab_type）。
    pub(crate) fn fallback_label(&self) -> &str {
        match self.0.as_ref() {
            "16" => "推荐",
            "36" => "漫剧",
            "39" => "真人",
            other => other,
        }
    }
}

/// 沉浸流只消费这几个身份/展示字段（feed 条目、种子、在播钉住三源合一）。
#[derive(Debug, Clone, Default)]
pub(super) struct StreamItem {
    pub(super) series_id: String,
    pub(super) title: String,
    pub(super) cover: String,
    /// 横版封面优先做全屏占位（竖版拉满横屏会糊，Tauri 同口径）。
    pub(super) horiz_cover: String,
    pub(super) heat_text: String,
    pub(super) season_tag: String,
    pub(super) badge: String,
    /// 推荐位的 vid（批量取流预取用；种子条目可能为空）。
    pub(super) vid: String,
}

impl From<&FeedItem> for StreamItem {
    fn from(i: &FeedItem) -> Self {
        Self {
            series_id: i.series_id.clone(),
            title: i.title.clone(),
            cover: i.cover.clone(),
            horiz_cover: i.horiz_cover.clone(),
            heat_text: i.heat_text.clone(),
            season_tag: i.season_tag.clone(),
            badge: i.badge.clone(),
            vid: i.vid.clone(),
        }
    }
}

/// 单个 tab 源的数据（各自持有会话与游标，切 tab 回来还在原位）。
#[derive(Default)]
pub(super) struct SourceFeed {
    pub(super) items: Vec<StreamItem>,
    session: String,
    offset: i64,
    pub(super) has_more: bool,
    /// 首批/换一批在拉（此刻画面还无剧可播）。
    pub(super) loading: bool,
    pub(super) fetching_more: bool,
    pub(super) failed: Option<String>,
    /// 首批（含换一批）落定时刻：「回首页过期换一批」的判定源
    /// （Tauri react-query dataUpdatedAt 同语义）。
    loaded_at: Option<std::time::Instant>,
}

/// 「换一批」过期阈值：回首页时流数据比这更旧就重开会话取新一批
/// （Tauri `FEED_STALE_MS` 同款）。
pub(super) const FEED_STALE: std::time::Duration = std::time::Duration::from_secs(5 * 60);

impl super::HomeView {
    // —— 数据：推荐流 ——

    /// 拉一个 tab 的首批（`restart` = 换一批：重开会话、游标归零；在播剧
    /// 由渲染期的钉住逻辑保持在画面上）。
    pub(super) fn load_feed(&mut self, source: Source, restart: bool, cx: &mut Context<Self>) {
        {
            let feed = self.feeds.get_mut(&source).expect("源已初始化");
            if feed.loading || feed.fetching_more {
                return;
            }
            if restart {
                feed.items.clear();
                feed.session.clear();
                feed.offset = 0;
                feed.has_more = false;
                feed.failed = None;
                *self.indexes.get_mut(&source).expect("游标已初始化") = 0;
            } else if !feed.items.is_empty() {
                return;
            }
            feed.loading = true;
        }
        cx.notify();

        let state = Arc::clone(&self.state);
        let session = self
            .feeds
            .get(&source)
            .map(|f| f.session.clone())
            .unwrap_or_default();
        let tab = source.as_str().to_string();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            fetch_recommend_feed(&tab, &session, 0, &[], &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(page) => {
                        {
                            let feed = view.feeds.get_mut(&source).expect("源已初始化");
                            feed.session = page.session_id;
                            feed.offset = page.next_offset;
                            feed.has_more = page.has_more;
                            feed.items = page.items.iter().map(StreamItem::from).collect();
                            feed.failed = None;
                            feed.loaded_at = Some(std::time::Instant::now());
                        }
                        // 取证：dump 开关时把整页封面都预载落盘（离线对拍用）
                        if std::env::var("HONGGUO_DUMP_COVER").is_ok() {
                            let items: Vec<StreamItem> =
                                view.feeds.get(&source).expect("源已初始化").items.clone();
                            for item in items.iter().take(12) {
                                view.load_cover_of(item, cx);
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("[Home] {source:?} 首批加载失败: {e}");
                        view.feeds.get_mut(&source).expect("源已初始化").failed =
                            Some(format!("推荐流加载失败：{e}"));
                    }
                }
                view.feeds.get_mut(&source).expect("源已初始化").loading = false;
                let feed = view.feeds.get(&source).expect("源已初始化");
                log::info!(
                    "[Home] {source:?} 首批落定：{} 条，failed={:?}",
                    feed.items.len(),
                    feed.failed.as_deref().map(|_| true).unwrap_or(false)
                );
                view.after_feed_loaded(source, cx);
            });
        })
        .detach();
    }

    /// 同会话续拉下一页（游标临近尾部时调用）。
    pub(super) fn load_more(&mut self, source: Source, cx: &mut Context<Self>) {
        let (session, offset) = {
            let feed = self.feeds.get(&source).expect("源已初始化");
            if feed.fetching_more || feed.loading || !feed.has_more {
                return;
            }
            (feed.session.clone(), feed.offset)
        };
        {
            let feed = self.feeds.get_mut(&source).expect("源已初始化");
            feed.fetching_more = true;
        }
        let state = Arc::clone(&self.state);
        let tab = source.as_str().to_string();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            fetch_recommend_feed(&tab, &session, offset, &[], &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                let feed = view.feeds.get_mut(&source).expect("源已初始化");
                feed.fetching_more = false;
                if let Ok(page) = result {
                    feed.session = page.session_id;
                    feed.offset = page.next_offset;
                    feed.has_more = page.has_more;
                    feed.items.extend(page.items.iter().map(StreamItem::from));
                    view.after_feed_loaded(source, cx);
                }
            });
        })
        .detach();
    }

    /// 首批/续页就位后的钩子：冷启动（引擎未起）自动播当前源第一条——对齐
    /// Tauri「起点决策 effect」的自动起播；已在播则只做游标跟随（种子到位、
    /// 换批、翻页都会改列表，游标指到的剧变了就该切过去）。
    fn after_feed_loaded(&mut self, source: Source, cx: &mut Context<Self>) {
        if self.player.is_none() && !self.switching && source == self.active {
            if let Some(item) = self.display_items().first().cloned() {
                self.play_target(item, None, None, cx);
            }
        } else if source == self.active {
            self.follow_cursor(cx);
        }
        cx.notify();
    }

    /// 列表结构变了（种子到位/翻页/换批）后跟上游标：游标指的剧与在播的
    /// 不一致就切过去——Tauri 的起点决策 effect 同语义（currentSeries 变了
    /// 就 setTarget，无闩锁）；切换途中不追，起播落定后各有出口。
    fn follow_cursor(&mut self, cx: &mut Context<Self>) {
        if self.switching || self.now.is_none() {
            return;
        }
        let Some(item) = self.display_items().get(self.cursor()).cloned() else {
            return;
        };
        if self
            .now
            .as_ref()
            .is_some_and(|n| n.series_id == item.series_id)
        {
            return;
        }
        self.play_target(item, None, None, cx);
    }

    /// 顶栏 tab 列表（服务端下发；拉取失败静默保留回落三件套，api 层已有
    /// 日志）。关注 45 一律不显示——追更由收藏页承载（Tauri buildTabList
    /// 同口径）。
    pub(super) fn load_tabs(&mut self, cx: &mut Context<Self>) {
        const FOLLOW_TAB_TYPE: &str = "45";
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            fetch_feed_tabs(&env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let Ok(meta_tabs) = result else {
                return;
            };
            let _ = this.update(cx, |view, cx| {
                let tabs: Vec<Source> = meta_tabs
                    .into_iter()
                    .filter(|t| t.tab_type != FOLLOW_TAB_TYPE)
                    .map(|t| {
                        view.tab_labels.insert(t.tab_type.clone(), t.title);
                        Source::new(&t.tab_type)
                    })
                    .collect();
                // 服务端异常空表：保留回落三件套
                if tabs.is_empty() {
                    return;
                }
                // 新 tab 补源状态（load_feed 的 expect("源已初始化")）
                for s in &tabs {
                    view.feeds.entry(s.clone()).or_default();
                    view.indexes.entry(s.clone()).or_insert(0);
                }
                // 当前选中的 tab 被服务端撤掉了：回推荐
                if !tabs.contains(&view.active) {
                    view.active = Source::recommend();
                    view.load_feed(Source::recommend(), false, cx);
                }
                view.tabs = tabs;
                cx.notify();
            });
        })
        .detach();
    }

    /// 观看历史（推荐 tab 种子 + 起点兜底）——**历史源按登录态切换**
    ///（Tauri `88f5edf` 同款）：登录读云端（跨端续播），匿名读本地记录。
    /// 匿名也发起云端查询会拿回设备级历史行（设备上做过的测试残留），
    /// 命中看过的剧就一律「续播」到那条记录的集数。
    pub(super) fn load_history(&mut self, cx: &mut Context<Self>) {
        if self.history_pending {
            return;
        }
        self.history_pending = true;
        if self.state.settings().account.is_none() {
            let state = Arc::clone(&self.state);
            let rx = bridge::spawn(async move {
                tokio::task::spawn_blocking(move || state.store.local_history_list())
                    .await
                    .expect("阻塞池 join")
            });
            cx.spawn(async move |this, cx| {
                let result = rx.await.expect("tokio 桥不会丢弃发送端");
                let Ok(items) = result else {
                    return;
                };
                let _ = this.update(cx, |view, cx| {
                    log::info!(
                        "[Home] 匿名：历史源取本地记录（{} 条，Tauri 88f5edf 同款）",
                        items.len()
                    );
                    view.history = Some(
                        items
                            .into_iter()
                            .map(|it| hongguo_api::history::WatchHistoryItem {
                                series_id: it.series_id,
                                title: it.title,
                                cover: it.cover,
                                vid_index: it.vid_index as i64,
                                vid: it.vid,
                                position_ms: it.position_ms,
                                duration_ms: it.duration_ms,
                                episode_cnt: it.episode_cnt as i64,
                                updated_at_ms: it.updated_at_ms,
                            })
                            .collect(),
                    );
                    // 「你的内容」种子到位（书架随后续上）
                    view.merge_seeds(cx);
                    view.load_seeds_meta(cx);
                    cx.notify();
                });
            })
            .detach();
            return;
        }
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            fetch_watch_history(0, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            if let Ok(page) = result {
                let _ = this.update(cx, |view, cx| {
                    view.history = Some(page.items);
                    // 「你的内容」种子到位（书架随后续上）
                    view.merge_seeds(cx);
                    view.load_seeds_meta(cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 「你的内容」排前：云端历史种子（书架随后由 [`Self::load_seeds_meta`]
    /// 补进同一段）。写进独立数组，不动流缓存（Tauri seedItems 同款）。
    fn merge_seeds(&mut self, cx: &mut Context<Self>) {
        let Some(history) = &self.history else {
            return;
        };
        let mut seen: HashSet<String> = HashSet::new();
        let mut seeds = Vec::new();
        for h in history {
            if h.series_id.is_empty() || !seen.insert(h.series_id.clone()) {
                continue;
            }
            seeds.push(StreamItem {
                series_id: h.series_id.clone(),
                title: h.title.clone(),
                cover: h.cover.clone(),
                horiz_cover: String::new(),
                ..Default::default()
            });
        }
        self.seeds = seeds;
        self.follow_cursor(cx);
    }

    /// 书架种子（历史之后）：标题/封面缺，起播后由档案解析自然补上
    /// （Tauri 同口径：书架条目只带 series_id）。种子段内部去重。
    fn load_seeds_meta(&mut self, cx: &mut Context<Self>) {
        let state = Arc::clone(&self.state);
        let account = state.settings().account.filter(|a| !a.user_id.is_empty());
        let Some(account) = account else {
            return;
        };
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            fetch_bookshelf(&account.user_id, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            if let Ok(entries) = result {
                let _ = this.update(cx, |view, cx| {
                    let mut seen: HashSet<String> =
                        view.seeds.iter().map(|i| i.series_id.clone()).collect();
                    for e in entries {
                        let series_id = e.series_id.clone();
                        if series_id.is_empty() || !seen.insert(series_id.clone()) {
                            continue;
                        }
                        view.seeds.push(StreamItem {
                            series_id,
                            ..Default::default()
                        });
                    }
                    view.follow_cursor(cx);
                });
            }
        })
        .detach();
    }

    // —— 展示列表：钉住在播 + 种子已并入 ——

    /// 渲染/交互共用的展示列表：种子（仅推荐 tab）+ 流条目，**按 series_id
    /// 去重**——会话轮换后跨页偶发同条目（Tauri 渲染期同款去重），不去重
    /// 就是滚轮里同一部剧连出好几遍。换一批（`pin_hold` 开闸）把在播剧换出
    /// 列表后钉在最前——「在播不动」；跨 tab 切换关闸，首批到位即换片。
    pub(super) fn display_items(&self) -> Vec<StreamItem> {
        let feed = self.feeds.get(&self.active).expect("源已初始化");
        assemble_display_items(
            feed,
            &self.seeds,
            self.active.as_str() == "16",
            self.pin_hold,
            self.now.as_ref(),
        )
    }

    /// 当前 tab 的流是否过期（从未加载 = 不过期——首载由 `load_feed` 负责，
    /// 这里只服务「回首页过期换一批」的判定）。
    pub(super) fn feed_stale(&self) -> bool {
        self.feeds
            .get(&self.active)
            .and_then(|f| f.loaded_at)
            .is_some_and(|t| t.elapsed() >= FEED_STALE)
    }

    /// 游标位置（展示列表下标，夹到已加载范围）。语义**对齐 Tauri**：游标是
    /// 位置不是身份，列表头部插了种子就落在种子上（冷启动正是这样切到
    /// 「你的内容」第一条，见 `player-page/start` 的起点决策 effect）。
    pub(super) fn cursor(&self) -> usize {
        let max = self.display_items().len().saturating_sub(1);
        (*self.indexes.get(&self.active).expect("游标已初始化")).min(max)
    }

    /// 把游标移到展示列表的第 `idx` 项。
    pub(super) fn set_cursor(&mut self, idx: usize) {
        *self.indexes.get_mut(&self.active).expect("游标已初始化") = idx;
    }

    /// 连播锁定中（binge 命中当前剧）。
    pub(super) fn in_binge(&self) -> bool {
        let Some(sid) = &self.binge else {
            return false;
        };
        self.now.as_ref().is_some_and(|n| &n.series_id == sid)
    }
}

/// 展示列表组装（纯函数：`display_items` 的主体抽出来，钉住语义不依赖
/// HomeView/窗口即可单测）。
fn assemble_display_items(
    feed: &SourceFeed,
    seeds: &[StreamItem],
    seeds_active: bool,
    pin_hold: bool,
    now: Option<&NowPlaying>,
) -> Vec<StreamItem> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut items: Vec<StreamItem> = Vec::with_capacity(seeds.len() + feed.items.len());
    // 种子只属于推荐 tab（tab_type 16）：类型 tab 的头一条必须是该类型
    // 的剧（Tauri home-page 同口径）
    if seeds_active {
        for s in seeds {
            if !s.series_id.is_empty() && seen.insert(s.series_id.clone()) {
                items.push(s.clone());
            }
        }
    }
    for i in &feed.items {
        if !i.series_id.is_empty() && seen.insert(i.series_id.clone()) {
            items.push(i.clone());
        }
    }
    // 「在播不动」钉：仅换一批/回首页闸门（pin_hold）开着且在播剧已被换出
    // 列表时插到最前——游标 0 仍是在播，follow_cursor 自然不切；跨 tab 关闸，
    // 首批到位即换游标处的剧（Tauri pinHold 同口径）。
    if pin_hold
        && let Some(now) = now
        && !seen.contains(&now.series_id)
    {
        items.insert(
            0,
            StreamItem {
                series_id: now.series_id.clone(),
                title: now.title.clone(),
                ..Default::default()
            },
        );
    }
    items
}
