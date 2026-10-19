//! 预览种子注入（`screenshot` feature）：把展示用假快照写进 HomeView。
//!
//! 必须长在 home 树里：种子要写的 `feeds`/`now`/面板开关都是视图私有
//! 字段。除数据源外一切照旧——布局、主题、组件与真实运行同一段代码，
//! 预览图因此可直接当视觉验收依据。场景键与矩阵见 `preview/scenes.rs`。

use std::sync::Arc;
use std::time::Instant;

use gpui_kit::{Context, Window};

use super::feed::{Source, StreamItem};
use super::{HomeView, NowPlaying};
use crate::preview::fixtures as fx;

impl HomeView {
    /// 按场景键注入假状态。错误只来自未知的场景键（矩阵写错键名启动即
    /// 报，开发工具宁可显性失败）。
    pub(crate) fn debug_preview_seed(
        &mut self,
        scene: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        // 失败/空态是「无剧可播」的分支：不灌流，画面落在提示 + 重试上
        if !matches!(scene, "error" | "empty") {
            self.seed_feed_state();
            self.seed_now_playing();
        }
        match scene {
            "feed" => {}
            "picker" => self.picker_open = true,
            "picker-series" => {
                self.picker_open = true;
                self.picker_tab_series = true;
                let sid = self.now.as_ref().expect("在播态已种子").series_id.clone();
                self.related_seasons
                    .insert(sid, Arc::new(fx::related_seasons()));
            }
            "downloads" => {
                self.downloads_open = true;
                // 假任务灌进**真实队列**：面板快照由 Render::render 每帧从
                // 队列读（state.queue().of_series），直接种 self.downloads
                // 会被下一帧刷新覆盖。不调 kick，调度不会被触发。
                let limit = self.state.settings().max_concurrency;
                self.state.replace_queue(
                    hongguo_service::download_service::queue::DownloadQueue::restore(
                        fx::download_tasks(),
                        limit,
                    ),
                );
            }
            "comments" => {
                self.comments_panel_open = true;
                self.emoji_picker_open = true;
                self.reply_to = Some(("c1".into(), None, "追剧到三点的猫".into()));
                self.comments_total = 3716;
                self.comments = Arc::new(fx::comments());
                self.set_input(&self.comment_input, "前方高能预警！", window, cx);
            }
            "login" => {
                self.login_panel_open = true;
                self.set_input(&self.login_mobile, "13800001234", window, cx);
            }
            "dm" => {
                self.dm_panel_open = true;
                self.set_input(&self.danmaku_input, "名场面来了", window, cx);
            }
            "mini" => {
                self.mini = Some(crate::mini::preview_geometry());
                crate::mini::set_active(true);
            }
            "portrait" => {
                // 竖屏窄条：几何快照是占位（预览不还原窗口），active 旗子
                // 让壳把侧栏藏掉、控制条走竖屏形态（Tauri portrait 同列）
                self.portrait = Some(crate::mini::preview_geometry());
                crate::portrait::set_active(true);
            }
            "error" => {
                let feed = self
                    .feeds
                    .get_mut(&Source::recommend())
                    .expect("源已初始化");
                feed.failed = Some("推荐流加载失败：网络请求超时（预览示例）".into());
            }
            "empty" => {}
            other => anyhow::bail!("未知预览场景键: {other}"),
        }
        cx.notify();
        Ok(())
    }

    /// 三个 tab 源全部灌满（滚轮/切 tab 的数据面）+ 游标归零 + 合成封面。
    fn seed_feed_state(&mut self) {
        let items: Vec<StreamItem> = fx::SERIES
            .into_iter()
            .enumerate()
            .map(|(i, (sid, title, heat))| {
                let (badge, season_tag) = fx::stream_tags(i);
                StreamItem {
                    series_id: sid.into(),
                    title: title.into(),
                    heat_text: heat.into(),
                    season_tag: season_tag.into(),
                    badge: badge.into(),
                    ..Default::default()
                }
            })
            .collect();
        for source in Source::fallback_tabs() {
            let feed = self.feeds.get_mut(&source).expect("源已初始化");
            feed.items = items.clone();
            feed.has_more = true;
            feed.loading = false;
            feed.failed = None;
            *self.indexes.get_mut(&source).expect("游标已初始化") = 0;
        }
        for (i, (sid, _, _)) in fx::SERIES.into_iter().enumerate() {
            self.covers.insert(sid.into(), fx::cover(i));
        }
    }

    /// 在播叠层：游标第一条视为在播，互动栏计数、清晰度菜单、控制条
    /// 常显一并就位（控制条常显复用取证口径 [`super::controls::
    /// CONTROLS_HOLD_SECS`]）。
    fn seed_now_playing(&mut self) {
        let (sid, title, heat) = fx::SERIES[0];
        self.preview_engine = true;
        self.now = Some(NowPlaying {
            series_id: sid.into(),
            title: title.into(),
            vid_index: 3,
            episode_count: 80,
            heat_text: heat.into(),
            season_tag: "全80集".into(),
            badge: String::new(),
            definitions: fx::definitions(),
            definition: 1080,
            vid: "preview-vid".into(),
            cover: String::new(),
        });
        self.controls_until = Some(
            Instant::now() + std::time::Duration::from_secs(super::controls::CONTROLS_HOLD_SECS),
        );
        self.ep_comment_count = 3716;
        self.ep_digg_count = 5678;
        self.follow_count = 999;
        // 追更键显隐（series_status=0 连载中）与已追态：预览里给全开
        self.series_status = Some(0);
        self.follow_is_on = Some(true);
    }

    /// 预填输入框（`set_value` 要 window，种子阶段正好有）。
    fn set_input(
        &self,
        input: &Option<gpui_kit::Entity<gpui_kit::component::input::InputState>>,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = input {
            input.update(cx, |state, cx| state.set_value(value, window, cx));
        }
    }
}
