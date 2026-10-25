//! 片尾与剧终接力：连播下一集、跨季/信息流接力、看完自动删。

use super::*;

impl super::HomeView {
    /// 连播：下一集；末集走剧终接力（下一季 → 信息流下一条推荐）。
    pub(in crate::home) fn advance_after_end(
        &mut self,
        cx: &mut Context<Self>,
        source: &'static str,
    ) {
        // 片尾前进幂等：一次 Ended 只允许推进一集。AVQueuePlayer 接力不换
        // 播放器，不能用播放器实例/单纯布尔位区分旧事件；用代次：
        // - 当前代次必须已经出过首帧（旧 Ended 不能推进新目标）；
        // - 同一代次只推进一次（Ended 回调重复/乱序只认第一次）。
        let generation = self.play_generation;
        log::info!(
            "[Home][尾进] 请求 source={source} gen={generation} first_frame_gen={} ended_gen={} now_ep={}",
            self.first_frame_generation,
            self.ended_generation,
            self.now.as_ref().map(|now| now.vid_index).unwrap_or(0)
        );
        if self.first_frame_generation != generation {
            log::info!(
                "[Home][尾进] 忽略 source={source} gen={generation}（首帧代次={}，旧播放器事件）",
                self.first_frame_generation
            );
            return;
        }
        if self.ended_generation == generation {
            log::info!("[Home][尾进] 忽略重复 source={source} gen={generation}");
            return;
        }
        // 看完自动删（Tauri handleEnded 的 autoDelete 同序：先删刚看完这集的
        // 本地文件，再决定连播——下载面板开着也照删，删除不依赖连播）。
        // 没下载过的集本来就没有文件，`delete_episode` 返回 false，不提示。
        self.auto_delete_finished_episode(cx);
        // 下载面板开着不自动连播（Tauri `autoNext && !downloading` 同款：
        // 面板挂着用户刚选完的集，切集/切剧会把勾选连同面板一起吃掉）——
        // 停在末帧等用户。
        if self.downloads_open {
            return;
        }
        // 帧不清：同剧连播由 `play_target` 按「同剧切集」保住末帧顶住画面
        //（Tauri use-frame-hold 同款；切下一季/信息流下一条时那边会清）。
        // 防重入：Ended 只在时钟走完才发，但引擎收尾、通道重连都可能让同一集
        // 的 Ended 再来一次——那时 vid_index 还停在旧值，会把刚起播的下一集
        // 又推进一格（连着跳集，看着就是「内容来回变」）。切换途中直接吞掉。
        if self.switching {
            return;
        }
        // 真正开始推进才记账：switching/下载面板挡下的调用不能消费代次。
        self.ended_generation = generation;
        let from = self.now.as_ref().map(|now| now.vid_index).unwrap_or(0);
        log::info!(
            "[Home][尾进] 片尾前进 gen={generation} ep {from}→{} source={source}",
            from + 1
        );
        // 自动连播静默起步（Tauri quietStart 同款）：新集首帧或用户真实输入
        // 之前，悬浮层一律不亮（首帧/on_player_event、wake_on_input 解除）。
        // 8s 硬上限（Tauri QUIET_WINDOW_MS 同款）：起播失败/首帧迟迟不来时，
        // 静默不能把悬浮层永久困死
        self.silent_until_input = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(8)).await;
            let _ = this.update(cx, |view, cx| {
                if view.silent_until_input {
                    view.silent_until_input = false;
                    cx.notify();
                }
            });
        })
        .detach();
        if let Some(now) = self.now.clone()
            && (now.vid_index as usize) < now.episode_count
        {
            let next_idx = now.vid_index + 1;
            if let Some(item) = self
                .display_items()
                .iter()
                .find(|i| i.series_id == now.series_id)
                .cloned()
            {
                // 自动连播完成本集 = 进入选中剧连播态（Tauri player-page
                // autoNext 分支 setBinge 同款）：滚轮/↑↓ 从这里起切集
                self.binge = Some(now.series_id.clone());
                self.play_target(item, Some(next_idx), None, cx);
                return;
            }
        }
        self.advance_after_series_end(cx);
    }

    /// 剧终接力（Tauri useBingeRelay.advanceAfterSeriesEnd 同款）：优先接
    /// **下一季**第 1 集（相关作品按季号挑「大于当前季的最小已上线季」），
    /// 没有下一季则跟随宿主（信息流）刷下一条推荐。本壳宿主恒为信息流，
    /// Tauri 无宿主签名下的「猜你喜欢」分支不适用。
    pub(in crate::home) fn advance_after_series_end(&mut self, cx: &mut Context<Self>) {
        let Some(now) = self.now.clone() else {
            // 没有在播目标时 step 不会再回到本函数，但连播锁仍要清
            self.binge = None;
            self.step(1, cx);
            return;
        };
        // 下载面板开着不接力（同连播闸）——退回提示，静默不动像坏了
        if self.downloads_open {
            self.show_toast(
                crate::i18n::tf(
                    "player.lastEpisode",
                    &[("index", &now.episode_count.to_string())],
                ),
                cx,
            );
            return;
        }
        // 下一季数据面：起播时已预取（plan/v 相关作品；未到/拉失败 = 空表）
        let works = self
            .related_seasons
            .get(&now.series_id)
            .cloned()
            .unwrap_or_default();
        if let Some(next) =
            crate::home::season::pick_next_season(&works, &now.series_id, &now.title)
        {
            // toast「即将播放下一季」+ 直接开播第 1 集，
            // 连播锁定延续到新季
            self.show_toast(
                crate::i18n::tf("player.playNextSeason", &[("title", next.title.as_str())]),
                cx,
            );
            self.binge = Some(next.series_id.clone());
            self.play_target(
                StreamItem {
                    series_id: next.series_id.clone(),
                    title: next.title.clone(),
                    cover: next.cover.clone(),
                    ..Default::default()
                },
                Some(1),
                None,
                cx,
            );
            return;
        }
        // 没有下一季：跟随信息流刷下一条推荐（已到底时宿主原地驻留）。
        //
        // **先解除连播锁定**：连播态下 `step` 会改走 `step_episode`，而末集
        // 的 `step_episode` 又回到本函数——末集不死循环（实测 5 千层递归
        // 后主线程栈溢出，见 2026-10-20 的崩溃取证）。
        self.binge = None;
        self.step(1, cx);
    }

    /// 看完自动删（设置页「看完自动删除」）：删掉刚看完那集的本地文件，
    /// 删到了才提示。文件删除 + 任务记录摘除都在 service 的 cleanup 里，
    /// 与 Tauri `deleteEpisode` 同源；同步文件 IO 走阻塞池，不堵 UI 线程。
    fn auto_delete_finished_episode(&mut self, cx: &mut Context<Self>) {
        if !self.state.settings().auto_delete_after_play {
            return;
        }
        let Some(now) = &self.now else {
            return;
        };
        let (series_id, vid_index) = (now.series_id.clone(), now.vid_index);
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            tokio::task::spawn_blocking(move || {
                hongguo_service::storage_service::cleanup::delete_episode(
                    &state, &series_id, vid_index,
                )
            })
            .await
            .unwrap_or(Ok(false))
        });
        cx.spawn(async move |this, cx| {
            let deleted = rx.await.unwrap_or(Ok(false)).unwrap_or(false);
            if !deleted {
                return;
            }
            let _ = this.update(cx, |view, cx| {
                view.show_toast(
                    crate::i18n::tf(
                        "player.autoDeleteDone",
                        &[("index", &vid_index.to_string())],
                    ),
                    cx,
                );
            });
        })
        .detach();
    }
}
