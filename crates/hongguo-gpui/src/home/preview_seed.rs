//! 预览种子注入（`screenshot` feature）：把展示用假快照写进 HomeView。
//!
//! 必须长在 home 树里：种子要写的 `feeds`/`now`/面板开关都是视图私有
//! 字段。除数据源外一切照旧——布局、主题、组件与真实运行同一段代码，
//! 预览图因此可直接当视觉验收依据。场景键与矩阵见 `preview/scenes.rs`。

use std::sync::Arc;
use std::time::Instant;

use gpui_kit::{Context, Window};

use super::feed::{Source, StreamItem};
use super::stage::StageCover;
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
        // 失败/空态/首拉加载态是「无剧可播」的分支：不灌流；失败与空态
        // 落在提示 + 重试，加载态只转 Spinner（启动时不该有重试钮）
        if !matches!(scene, "error" | "empty" | "feed-loading") {
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
                // 种子带表情码：内联 token 渲染（Tauri RichEmojiInput 对齐）
                // 的验收面——场景图里输入框应出现表情图而非 `[名字]` 文案
                self.set_input(
                    &self.comment_input,
                    "前方高能预警！[微笑][偷笑]",
                    window,
                    cx,
                );
            }
            "login" => {
                self.login_panel_open = true;
                self.set_input(&self.login_mobile, "13800001234", window, cx);
            }
            "dm" => {
                self.dm_panel_open = true;
                self.set_input(&self.danmaku_input, "名场面来了[笑][鼓掌]", window, cx);
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
            "feed-loading" => {
                let feed = self
                    .feeds
                    .get_mut(&Source::recommend())
                    .expect("源已初始化");
                feed.items.clear();
                feed.loading = true;
                feed.failed = None;
            }
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
            let url = format!("preview://home/{i}");
            crate::preview::fixtures::put_preview_image(&url, fx::cover(i));
            let sid: String = sid.into();
            self.stage_covers
                .insert(sid, StageCover::Ready(fx::cover(i)));
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
            // 预览无真实档案：分集表给一份占位（预取下一集按它查 vid）。
            episode_vids: std::sync::Arc::new(Vec::new()),
        });
        self.controls_until = Some(
            Instant::now() + std::time::Duration::from_secs(super::controls::CONTROLS_HOLD_SECS),
        );
        self.ep_comment_count = 3716;
        self.ep_digg_count = 5678;
        self.follow_count = 999;
        // 追更态 wiring：series_status=0（连载中）/已追。预览是匿名态，
        // 追更键显隐带登录门（follow_visible），矩阵里这个键不出现——这里
        // 只保状态链路有值可画（若未来预览支持假登录，钮会以连载态出现）。
        self.series_status = Some(0);
        self.follow_is_on = Some(true);
        // 信息层简介：长文本（>40 字触发「展开」），验收收起两行钳制与
        // 展开后的换行
        self.intros.insert(
            sid.into(),
            "穿成青云宗外门弟子的林辰，开局伪废灵根。八年困于炼气一层，全宗笑他是吃软饭的废物。十年前他救过的小女孩，如今已是天灵根第一女神苏清瑶。退婚当日，他觉醒万倍暴击系统：一颗筑基丹，暴击出金丹修为；一式基础剑法，暴击出仙阶剑意。从此踩天骄、夺造化，踏诸宗如平地。多年后有人问他逆袭的秘诀，林辰看着远方淡淡一笑：谁说我靠的是系统？".into(),
        );
    }

    /// 预填输入框（`set_value` 要 window，种子阶段正好有）。走
    /// [`crate::danmaku_emoji::rich_content`]：种子里带的表情码以原子
    /// token 附着，场景图验的就是输入框的内联渲染。
    fn set_input(
        &self,
        input: &Option<gpui_kit::Entity<gpui_kit::component::input::InputState>>,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = input {
            input.update(cx, |state, cx| {
                state.set_value(crate::danmaku_emoji::rich_content(value), window, cx)
            });
        }
    }
}
