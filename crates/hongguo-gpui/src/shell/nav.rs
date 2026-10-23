use super::*;

use gpui_kit::assets::IconName;

use crate::detail::DetailEvent;

/// 侧栏导航目的地（与 Tauri `app-sidebar.tsx` 的 NAV_ITEMS 逐项同序）。
/// 播放不在列：它由各页「播放/继续播放」进入，首页本身就是沉浸流。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dest {
    Home,
    Browse,
    Rank,
    New,
    History,
    Collections,
    Liked,
    Follows,
    Reservations,
    Tasks,
    Merge,
    Storage,
    Settings,
    /// 应用内详情页（不在侧栏：播放页点剧名 / 播放入口进入）。
    Detail,
    /// 创作者主页（不在侧栏：详情页出品方/演职员芯片进入）。
    Creator,
}

impl Dest {
    pub(super) const ALL: [Dest; 13] = [
        Dest::Home,
        Dest::Browse,
        Dest::Rank,
        Dest::New,
        Dest::History,
        Dest::Collections,
        Dest::Liked,
        Dest::Follows,
        Dest::Reservations,
        Dest::Tasks,
        Dest::Merge,
        Dest::Storage,
        Dest::Settings,
    ];

    pub(super) fn label(self) -> String {
        match self {
            Dest::Home => crate::i18n::t("nav.home.title"),
            Dest::Browse => crate::i18n::t("nav.browse.title"),
            Dest::Rank => crate::i18n::t("nav.rank.title"),
            Dest::New => crate::i18n::t("nav.new.title"),
            Dest::History => crate::i18n::t("nav.history.title"),
            Dest::Collections => crate::i18n::t("nav.collections.title"),
            Dest::Liked => crate::i18n::t("nav.liked.title"),
            Dest::Follows => crate::i18n::t("nav.follows.title"),
            Dest::Reservations => crate::i18n::t("nav.reservations.title"),
            Dest::Tasks => crate::i18n::t("nav.tasks.title"),
            Dest::Merge => crate::i18n::t("nav.merge.title"),
            Dest::Storage => crate::i18n::t("nav.storage.title"),
            Dest::Settings => crate::i18n::t("nav.settings.title"),
            // 不在侧栏（播放页进入），仅兜匹配
            Dest::Detail => crate::i18n::t("nav.detail.title"),
            Dest::Creator => crate::i18n::t("nav.creator.title"),
        }
    }

    pub(super) fn icon(self) -> IconName {
        match self {
            Dest::Home => IconName::Flame,
            Dest::Browse => IconName::Compass,
            Dest::Rank => IconName::Trophy,
            Dest::New => IconName::Sparkles,
            Dest::History => IconName::ClockArrowDown,
            Dest::Collections => IconName::Star,
            Dest::Liked => IconName::Heart,
            Dest::Follows => IconName::Tv,
            Dest::Reservations => IconName::BellRing,
            Dest::Tasks => IconName::ListChecks,
            Dest::Merge => IconName::Combine,
            Dest::Storage => IconName::HardDrive,
            Dest::Settings => IconName::Settings,
            Dest::Detail => IconName::Info,
            Dest::Creator => IconName::UserRound,
        }
    }

    /// 取证开关取值 → 目的地（`HONGGUO_PAGE=browse|new|…`，与 HONGGUO_RANK
    /// 同族：无人值守点检任一页面）。
    pub(super) fn from_key(key: &str) -> Option<Dest> {
        match key {
            "home" => Some(Dest::Home),
            "browse" => Some(Dest::Browse),
            "rank" => Some(Dest::Rank),
            "new" => Some(Dest::New),
            "history" => Some(Dest::History),
            "collections" => Some(Dest::Collections),
            "liked" => Some(Dest::Liked),
            "follows" => Some(Dest::Follows),
            "reservations" => Some(Dest::Reservations),
            "tasks" => Some(Dest::Tasks),
            "merge" => Some(Dest::Merge),
            "storage" => Some(Dest::Storage),
            "settings" => Some(Dest::Settings),
            _ => None,
        }
    }

    /// 目的地 → 壳内视图（13 页 + 详情全部实现，无空态兜底）。
    pub(super) fn view(self) -> View {
        match self {
            Dest::Home => View::Home,
            Dest::Rank => View::Rank,
            Dest::Browse => View::Browse,
            Dest::New => View::New,
            Dest::Liked => View::Liked,
            Dest::Collections => View::Collections,
            Dest::Follows => View::Follows,
            Dest::History => View::History,
            Dest::Reservations => View::Reservations,
            Dest::Tasks => View::Tasks,
            Dest::Settings => View::Settings,
            Dest::Storage => View::Storage,
            Dest::Merge => View::Merge,
            Dest::Detail => View::Detail,
            Dest::Creator => View::Creator,
        }
    }
}

/// 壳内视图（Dest::view 的落点；Detail 不在侧栏，由播放页点剧名进入）。
pub(super) enum View {
    Home,
    Rank,
    Browse,
    New,
    Liked,
    Collections,
    Follows,
    History,
    Reservations,
    Tasks,
    Settings,
    Storage,
    Merge,
    Detail,
    Creator,
}

impl super::Shell {
    /// 切换目的地并触发进入钩子（侧栏点击与 `HONGGUO_PAGE` 取证开关共用）。
    /// 首页播放器的生命周期跟着走（Tauri 路由卸载/重挂载的同语义）：离开
    /// 首页停播落进度，回首页从落盘进度接上。
    pub(super) fn enter_dest(&mut self, dest: Dest, cx: &mut Context<Self>) {
        if dest == Dest::Home {
            self.feed.update(cx, |home, cx| home.resume_if_idle(cx));
        } else {
            self.feed.update(cx, |home, cx| home.release_for_leave(cx));
        }
        self.dest = dest;
        match dest {
            Dest::Rank => self.rank.update(cx, RankView::load),
            Dest::Browse => self.browse.update(cx, BrowseView::load),
            Dest::New => self.new_drama.update(cx, NewDramaView::load),
            Dest::Liked => self.liked.update(cx, LikedView::load),
            Dest::Collections => self.collections.update(cx, CollectionView::load),
            Dest::Follows => {
                self.follows.update(cx, FollowsView::load);
                self.refresh_follow_unread(cx);
            }
            Dest::History => self.history.update(cx, HistoryView::load),
            Dest::Reservations => self.reservations.update(cx, ReservationView::load),
            Dest::Tasks => self.tasks.update(cx, TasksView::load),
            Dest::Settings => self.settings_page.update(cx, SettingsView::load),
            Dest::Storage => self.storage.update(cx, StorageView::load),
            Dest::Merge => self.merge.update(cx, MergeView::load),
            _ => {}
        }
        cx.notify();
    }

    /// 追更未读角标刷新（本地通知未读数；未登录恒 0，Tauri 单入口口径）。
    pub(super) fn refresh_follow_unread(&mut self, cx: &mut Context<Self>) {
        let count = if self.state.settings().account.is_some() {
            hongguo_service::follow_service::notices(&self.state)
                .map(|ns| ns.iter().filter(|n| !n.read).count())
                .unwrap_or(0)
        } else {
            0
        };
        self.follow_unread = count;
        cx.notify();
    }

    /// 页面「继续播放」的统一入口：切回首页沉浸流起播（详情页播放同通道，
    /// 页面间不互相持有）。
    pub(super) fn start_play(
        &mut self,
        series_id: String,
        vid_index: u32,
        start_secs: f64,
        cx: &mut Context<Self>,
    ) {
        self.dest = Dest::Home;
        self.feed.update(cx, |home, cx| {
            home.play_from_detail(series_id, vid_index, start_secs, cx)
        });
        cx.notify();
    }

    /// 打开应用内详情页（播放页点剧名）：同剧重复点击不重载。
    pub(super) fn open_detail(&mut self, series_id: String, cx: &mut Context<Self>) {
        self.open_detail_with_prefill(series_id, None, cx);
    }

    /// 打开详情页（可带来源页 prefill：未上线剧走「即将上线」降级视图）。
    pub(crate) fn open_detail_with_prefill(
        &mut self,
        series_id: String,
        prefill: Option<crate::detail::DetailPrefill>,
        cx: &mut Context<Self>,
    ) {
        if self.dest == Dest::Detail && self.detail_series.as_deref() == Some(&series_id) {
            return;
        }
        // 进详情页 = 离开首页沉浸流（Tauri /detail 是独立路由，进入即卸载
        // 首页）：停播落进度。从别的页进来时引擎多半已被 enter_dest 释放，
        // 这里的无条件调用只是兜「首页还没切走」的路径。
        self.feed.update(cx, |home, cx| home.release_for_leave(cx));
        let view = match &self.detail {
            Some(v) => v.clone(),
            None => {
                let v = cx.new(|cx| DetailView::new(self.state.clone(), cx));
                // 详情页事件（返回/播放）→ 壳裁决目的地与起播
                cx.subscribe(&v, |this, _, event: &DetailEvent, cx| {
                    this.on_detail_event(event.clone(), cx);
                })
                .detach();
                self.detail = Some(v.clone());
                v
            }
        };
        view.update(cx, |detail, cx| {
            detail.load_with_prefill(series_id.clone(), prefill, cx)
        });
        self.detail_series = Some(series_id);
        self.dest = Dest::Detail;
        cx.notify();
    }

    /// 详情页的返回/播放入口（DetailView 发事件，壳裁决目的地）。
    pub(super) fn on_detail_event(&mut self, event: DetailEvent, cx: &mut Context<Self>) {
        match event {
            DetailEvent::Back => {
                self.dest = Dest::Home;
                // 回首页续播（Tauri 返回重挂载「挂载默认开闸」同语义）
                self.feed.update(cx, |home, cx| home.resume_if_idle(cx));
                cx.notify();
            }
            DetailEvent::Play {
                series_id,
                vid_index,
                start_secs,
            } => {
                self.dest = Dest::Home;
                self.feed.update(cx, |home, cx| {
                    home.play_from_detail(series_id, vid_index, start_secs, cx)
                });
                cx.notify();
            }
            DetailEvent::OpenSettings => {
                self.enter_dest(Dest::Settings, cx);
            }
            // 出品方/演职员芯片 → 创作者主页（详情实体保留，返回原地恢复）
            DetailEvent::OpenCreator(user_id) => {
                self.creator.update(cx, |view, cx| view.load(user_id, cx));
                self.dest = Dest::Creator;
                cx.notify();
            }
        }
    }

    /// 创作者主页返回：回来源详情页（实体保留原样）；没有详情来源时回首页
    /// 续播（与详情返回同语义）。
    pub(super) fn close_creator(&mut self, cx: &mut Context<Self>) {
        if self.detail.is_some() && self.detail_series.is_some() {
            self.dest = Dest::Detail;
        } else {
            self.dest = Dest::Home;
            self.feed.update(cx, |home, cx| home.resume_if_idle(cx));
        }
        cx.notify();
    }
}
