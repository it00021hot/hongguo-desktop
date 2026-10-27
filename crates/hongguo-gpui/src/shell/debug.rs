use super::*;

#[cfg(feature = "screenshot")]
use gpui_kit::App;
#[cfg(feature = "screenshot")]
use hongguo_service::events::ServiceEvent;

impl super::Shell {
    /// 首页当前 tab 源（预览交互探针读，断言切源生效）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn active_source(&self, cx: &App) -> crate::home::Source {
        self.feed.read(cx).active_source()
    }

    /// 取证辅助（screenshot feature）：暂停播放 + 拉长控制条可见窗，让
    /// 无人值守抓帧能拍到控制条/互动栏。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_pause_player(&self, cx: &mut App) {
        self.feed.update(cx, |home, cx| home.debug_pause_player(cx));
    }

    /// 取证辅助（screenshot feature）：打开下载面板。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_downloads(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.feed
            .update(cx, |home, cx| home.debug_open_downloads(window, cx));
    }

    /// 取证辅助（screenshot feature）：打开弹幕设置面板。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_dm_settings(&self, cx: &mut App) {
        self.feed
            .update(cx, |home, cx| home.debug_open_dm_settings(cx));
    }

    /// 取证辅助（screenshot feature）：打开选集浮层系列剧 tab。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_picker(&self, cx: &mut App) {
        self.feed.update(cx, |home, cx| home.debug_open_picker(cx));
    }

    /// 取证辅助（screenshot feature）：打开评论面板与表情选择器。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_comments(&self, cx: &mut App) {
        self.feed
            .update(cx, |home, cx| home.debug_open_comments(cx));
    }

    /// 取证辅助（screenshot feature）：评论面板 + 表情面板直达 GIF 档。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_comments_gif(&self, cx: &mut App) {
        self.feed
            .update(cx, |home, cx| home.debug_open_comments_gif(cx));
    }

    /// 取证辅助（screenshot feature）：开评论面板并聚焦输入框。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_comments_caret(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.feed
            .update(cx, |home, cx| home.debug_open_comments_caret(window, cx));
    }

    /// 取证辅助（screenshot feature）：打开评论面板并派发 @提及弹层。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_mention(&self, cx: &mut App) {
        self.feed.update(cx, |home, cx| home.debug_open_mention(cx));
    }

    /// 取证辅助（screenshot feature）：打开登录面板。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_login(&self, cx: &mut App) {
        self.feed.update(cx, |home, cx| home.debug_open_login(cx));
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=suggest`，配
    /// `HONGGUO_PAGE=browse`）：往找剧页搜索框灌关键词——联想走真实接口，
    /// 抓帧拍的是真实下拉列表（含封面缩略图与命中高亮）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_fill_browse_keyword(&self, window: &mut Window, cx: &mut App) {
        self.browse
            .update(cx, |view, cx| view.debug_fill_keyword("斗罗", window, cx));
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=boss-set`）：
    /// 走真实保存路径写老板键（注册 → 落库；冲突则拦截）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_set_boss_key(&self, window: &mut Window, cx: &mut App) {
        let spec = std::env::var("HONGGUO_BOSS_KEY").unwrap_or_default();
        self.settings_page
            .update(cx, |view, cx| view.debug_set_boss_key(&spec, window, cx));
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=portrait`）：
    /// 进竖屏（几何按当前在播档位收窄），抓帧拍竖条形态。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_enter_portrait(&self, window: &mut Window, cx: &mut App) {
        self.feed
            .update(cx, |home, cx| home.enter_portrait(window, cx));
        // 顺带逼一次重适配路径（清零「上次比例」→ 下一帧 render 里的
        // refit_portrait 必跑），验「render 期派发窗口操作」这条不炸
        self.feed
            .update(cx, |home, _| home.debug_force_portrait_refit());
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=portrait-full`）：
    /// 进竖屏后切铺满档（验撑满工作区高的几何与铺满态按钮）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_portrait_full(&self, window: &mut Window, cx: &mut App) {
        self.feed.update(cx, |home, cx| {
            home.enter_portrait(window, cx);
            home.toggle_portrait_full(window, cx);
        });
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=auto-delete`）：
    /// 打开「看完自动删除」（走真实保存路径），供播放到底的行为取证。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_set_auto_delete(&self, window: &mut Window, cx: &mut App) {
        self.settings_page
            .update(cx, |view, cx| view.debug_set_auto_delete(window, cx));
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=next-series`）：
    /// 切到信息流下一条（换剧）——验切剧走封面、不留上一部的定格帧。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_next_series(&mut self, cx: &mut App) {
        self.dest = Dest::Home;
        self.feed.update(cx, |home, cx| home.step(1, cx));
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=mini`）：
    /// 进小屏（480×270），验铺满裁切与小屏布局。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_enter_mini(&self, window: &mut Window, cx: &mut App) {
        self.feed.update(cx, |home, cx| home.enter_mini(window, cx));
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=related-upcoming`，
    /// 配 `HONGGUO_DETAIL=<sid>`）：在当前详情页走一次「未上线相关推荐」点击
    /// 路径，验 prefill 兜住「解析失败 → 即将上线降级视图」。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_upcoming_related(&self, cx: &mut App) {
        if let Some(detail) = &self.detail {
            detail.update(cx, |view, cx| view.debug_open_upcoming_related(cx));
        } else {
            log::warn!("[Shot] related-upcoming 需要 HONGGUO_DETAIL=<sid> 先打开详情页");
        }
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=detail-login-wall`，
    /// 配 `HONGGUO_DETAIL=<sid>` 且数据根未登录）：走真实收藏点击，验
    /// 登录提示与设置页跳转。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_trigger_detail_collect(&self, window: &mut Window, cx: &mut App) {
        if let Some(detail) = &self.detail {
            detail.update(cx, |view, cx| view.debug_trigger_collect(window, cx));
        } else {
            log::warn!("[Shot] detail-login-wall 需要 HONGGUO_DETAIL=<sid> 先打开详情页");
        }
    }

    /// 取证辅助（Phase 1b `HONGGUO_SHOT_PANEL=next-episode`）：走真实连播
    /// step_episode，量系统播放器切集首帧。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_next_episode(&self, cx: &mut App) {
        self.feed.update(cx, |home, cx| {
            home.step_episode(1, cx);
        });
    }

    /// 取证辅助（`HONGGUO_SHOT_PANEL=rate-2x`）：按控制条同一路径切 2x。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_rate_2x(&self, cx: &mut App) {
        self.feed.update(cx, |home, cx| home.set_rate(2.0, cx));
    }

    /// 取证辅助（`HONGGUO_SHOT_PANEL=immersive`）：进播放器沉浸全屏。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_immersive(&self, window: &mut Window, cx: &mut App) {
        self.feed.update(cx, |home, cx| {
            home.toggle_immersive_fullscreen(window, cx);
        });
    }

    /// 取证辅助（`HONGGUO_SHOT_PANEL=locale-en`）：走真实语言切换路径，
    /// 验持久化、状态型控件刷新与整窗重绘。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_set_locale_en(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_locale(crate::i18n::Locale::EnUs, window, cx);
    }

    /// 取证辅助（`HONGGUO_SHOT_PANEL=notify`）：走真实 FollowNotices 事件
    /// 路径弹一条系统通知，验 GPUI 平台层通知通道。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_show_notification(&mut self, cx: &mut Context<Self>) {
        self.on_service_event(
            ServiceEvent::FollowNotices(
                hongguo_service::follow_service::events::FollowNoticesEvent {
                    notices: vec![hongguo_common::model::FollowNotice {
                        id: -1,
                        kind: hongguo_common::model::NOTICE_KIND_FOLLOW.into(),
                        series_id: "system-notification-probe".into(),
                        title: crate::i18n::t("follow.checkUpdate"),
                        detail: crate::i18n::t("follow.checkNone"),
                        at: chrono::Utc::now().timestamp(),
                        read: false,
                    }],
                },
            ),
            cx,
        );
    }

    /// 取证辅助（`HONGGUO_SHOT_PANEL=binge-state`）：走「观看完整短剧」的
    /// 同一状态迁移，拍横条消失、左上集数胶囊出现的第二帧。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_enter_binge(&mut self, cx: &mut Context<Self>) {
        self.feed.update(cx, |view, cx| view.enter_binge(cx));
    }

    /// 取证辅助（screenshot feature）：按 `HONGGUO_PAGE` 的键切目的地
    ///（验「去播放再回来」的浏览位置保留）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_switch_dest(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(dest) = Dest::from_key(key) {
            self.enter_dest(dest, cx);
        }
    }
}
