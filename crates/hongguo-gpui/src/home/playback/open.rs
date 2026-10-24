//! 起播链：起播请求/系统播放器打开参数，以及 tokio 线程上的
//! `play_task` 与手动重试入口。

use super::*;

/// 一次起播请求的完整参数（结构体承载，[`Self::play_task`] 的参数面收敛）。
pub(super) struct PlayRequest {
    pub(super) item: StreamItem,
    /// 连播指定集号（None = 走起点决策）。
    pub(super) forced_next_ep: Option<u32>,
    /// 用户显式选择的清晰度（None = 自动/沿用），传给 prepare。
    pub(super) preferred_definition: Option<u32>,
    /// 起播倍速。
    pub(super) rate: f64,
    /// 切清晰度保进度的位置覆盖（None = 走起点决策）。
    pub(super) start_override: Option<f64>,
}

/// 系统播放器的打开参数：视频位是渲染线程的私有对象，只能在主线程创建，
/// 所以起播链把它带回来。
pub(super) struct SystemSpec {
    pub(super) source: hongguo_player::PlaySource,
    pub(super) start_secs: f64,
    pub(super) rate: f64,
    pub(super) events: mpsc::Sender<PlayerEvent>,
    pub(super) on_stall: Option<super::player::StallHook>,
}

impl super::HomeView {
    /// 起播链（tokio 线程）：解析档案 → 起点决策 → prepare → 打开参数带回
    /// 主线程。与 Tauri 同一段服务面；起点 >3 秒续播，本地档案优先、云端
    /// 历史兜底。
    pub(super) async fn play_task(
        state: Arc<AppStateInner>,
        history: Vec<WatchHistoryItem>,
        req: PlayRequest,
        events: mpsc::Sender<PlayerEvent>,
    ) -> AppResult<(SystemSpec, super::NowPlaying)> {
        let PlayRequest {
            item,
            forced_next_ep,
            preferred_definition,
            rate,
            start_override,
        } = req;
        let env = state.api_env();

        let series = match state.store.series_by_id(&item.series_id)? {
            Some(s) if !s.episodes.is_empty() => s,
            _ => {
                let s = hongguo_service::series_service::resolver::resolve_series(
                    &item.series_id,
                    &env,
                )
                .await?;
                state.store.upsert_series(&s)?;
                s
            }
        };
        let episode_count = series.episodes.len();
        // 切档保进度：覆盖值优先于起点决策（覆盖必然伴随显式集号——
        // 切清晰度 = 同集同位重播）
        let (vid_index, start_secs) = match (start_override, forced_next_ep) {
            (Some(pos), forced) => (forced.unwrap_or(1), pos),
            (None, Some(idx)) => (idx, 0.0),
            (None, None) => hongguo_service::play_service::resume::resolve_entry(
                &state.store,
                &item.series_id,
                &history,
            )?,
        };
        // 找集号：连播是「上一集接着往下」——目标集不存在时必须报错，绝不能
        // 回落到第 1 集（那是「又从头播一遍」的来源，用户看到的正是重复）。
        // 只有起点决策（forced_next_ep 为空）才允许回落到第 1 集。
        let ep = match series.episodes.iter().find(|e| e.vid_index == vid_index) {
            Some(ep) => ep,
            None if forced_next_ep.is_some() => {
                return Err(hongguo_common::AppError::Media(crate::i18n::tf(
                    "player.episodeMissing",
                    &[
                        ("index", &vid_index.to_string()),
                        ("total", &series.episodes.len().to_string()),
                    ],
                )));
            }
            None => series
                .episodes
                .iter()
                .find(|e| e.vid_index == 1)
                .ok_or_else(|| {
                    hongguo_common::AppError::Media(crate::i18n::t("player.noPlayableEpisode"))
                })?,
        };
        if ep.vid.trim().is_empty() {
            return Err(hongguo_common::AppError::Media(crate::i18n::t(
                "player.episodeNoVid",
            )));
        }
        let progress_key = format!("{}:{}", item.series_id, ep.vid_index);
        // 实际要播的 vid：比对「同一集被播了两遍」与「恰好落在第一集」的
        // 唯一凭证（重复内容投诉时先看这行，再看起播目标那行）
        log::info!(
            "[Home] 起播 {} 第 {} 集（vid={}）起点 {:.1}s",
            item.series_id,
            ep.vid_index,
            ep.vid,
            start_secs
        );
        // 本地成品优先（Tauri `play_service::local::resolve_play` 同款）：这一集
        // 已经下好就直接读本地文件——离线可播、秒开，且没有档位概念（本地只有
        // 一版，清晰度菜单该是灰的）。记录里路径存在但文件没了（陈旧记录）时
        // 回落在在线流；在线分流（prepare）只在真的没有本地成品时才走。
        let local_path = state
            .queue()
            .completed_path(&item.series_id, ep.vid_index)
            .filter(|p| !p.trim().is_empty() && std::path::Path::new(p).is_file());
        let (source, definitions, definition) = match local_path {
            Some(path) => {
                log::info!("[Home] 本地成品命中，走本地播放：{path}");
                (
                    hongguo_player::PlaySource::Local { path: path.into() },
                    Vec::new(),
                    0u32,
                )
            }
            None => {
                let prepared =
                    prepare(&state, &ep.vid, &progress_key, preferred_definition).await?;
                // 与 Tauri `play_service::local::resolve_play` 共用同一调度：
                // 在线起播后把本剧后续 1..=prefetch_episodes 集同时排进流
                // 预取与（无 HEVC 时的）兼容预转。GPUI 自己起播曾漏掉这步，
                // 只预取了信息流下一部剧，自动连播到下一集仍然冷启动。
                hongguo_service::play_service::schedule_following_episodes(
                    &state,
                    &item.series_id,
                    ep.vid_index,
                );
                (
                    hongguo_player::PlaySource::Online {
                        vid: ep.vid.clone(),
                        definition: prepared.definition,
                    },
                    prepared.definitions,
                    prepared.definition,
                )
            }
        };
        let on_stall: Option<Arc<dyn Fn() + Send + Sync>> = {
            let state = Arc::clone(&state);
            let vid = ep.vid.clone();
            let progress_key = progress_key.clone();
            Some(Arc::new(move || {
                // loader 读到头部预算之外时按需续填；resume 内部有
                // 取流权退避重试，不会在预取收尾窗口里丢一次就放弃。
                hongguo_service::play_service::online::resume_fill(
                    &state,
                    &vid,
                    definition,
                    &progress_key,
                );
            }))
        };
        let now = NowPlaying {
            series_id: item.series_id,
            title: if item.title.is_empty() {
                series.title.clone()
            } else {
                item.title.clone()
            },
            vid_index: ep.vid_index,
            episode_count,
            heat_text: item.heat_text,
            season_tag: item.season_tag,
            badge: item.badge,
            definitions,
            definition,
            vid: ep.vid.clone(),
            // 封面：调用方的条目没带就用档案的（档案是权威源）——观看历史
            // 记的封面不再取决于「谁发起的这次起播」。
            cover: if item.cover.is_empty() {
                series.cover.clone()
            } else {
                item.cover.clone()
            },
            // 全分集表（集号 → vid）：预取下一集与片尾接力都从这里取 vid，
            // 主线程不必再读库（库是同步阻塞门面）。
            episode_vids: Arc::new(
                series
                    .episodes
                    .iter()
                    .map(|episode| (episode.vid_index, episode.vid.clone()))
                    .collect(),
            ),
        };
        // 视频位是渲染线程的私有对象，只能在主线程创建：把打开参数原样带回
        // 主线程，由调用方在实体借用解除后创建（见 `play_target` 的两段式
        // update——open 过程中的 STA COM 调用会泵窗口消息，重入撞借用）。
        Ok((
            SystemSpec {
                source,
                start_secs,
                rate,
                events,
                on_stall,
            },
            now,
        ))
    }

    /// 手动重试（失败卡的「重试」按钮）：起播失败重走当前游标位的起播链
    ///（集号/起点重新走起点决策）；播放中失败（引擎自停、now 就是失败的
    /// 那部）则同集重播——play_target 的同剧早退只拦 forced_next_ep=None
    /// 的路径，带集号才能重进去。
    pub(in crate::home) fn retry_play(&mut self, cx: &mut Context<Self>) {
        let cursor_item = self.display_items().get(self.cursor()).cloned();
        if let (Some(now), Some(item)) = (self.now.clone(), cursor_item.as_ref())
            && now.series_id == item.series_id
        {
            self.play_target(item.clone(), Some(now.vid_index), None, cx);
            return;
        }
        if let Some(item) = cursor_item {
            self.play_target(item, None, None, cx);
        }
    }
}
