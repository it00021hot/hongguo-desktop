//! 进度落盘与观看历史：5 秒节流写、强制落盘、历史心跳与节拍上报。

use super::*;

impl super::HomeView {
    /// 5 秒节流落盘（播放档案是真值：续播、详情页「继续看」都吃它）。
    pub(in crate::home) fn save_progress_maybe(&mut self) {
        if self.last_save.elapsed() < Duration::from_secs(PROGRESS_SAVE_EVERY_SECS) {
            return;
        }
        self.last_save = Instant::now();
        self.save_progress_inner();
    }

    /// 立即落一次进度（不带节流）：关小屏等「要走了」的时刻强制落，
    /// 别让最后一段播放丢在节流窗里（Tauri onPause 强落同款）；同时强制
    /// 记一条观看历史（切集/暂停/离开的 force 收尾，Tauri 同款）。
    pub(in crate::home) fn save_progress_now(&mut self) {
        self.save_progress_inner();
        self.record_watch_history(true);
    }

    fn save_progress_inner(&mut self) {
        let Some((handle, now)) = self.player.as_ref().zip(self.now.clone()) else {
            return;
        };
        let (pos, dur) = (handle.position_secs(), handle.duration_secs());
        let store = self.state.store.clone();
        bridge::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                position::save_store(&store, &now.series_id, now.vid_index, pos, dur)
            })
            .await;
        });
    }

    /// 观看历史上报心跳：1 秒一拍，取当前播放位置交给节拍判定。
    ///
    /// 为什么独立成心跳、不挂在 [`Self::save_progress_maybe`]（系统节拍 +
    /// 5 秒落盘窗口）上：首报会被量化到 5–10 秒，60 秒节拍也随落盘
    /// 格点漂移——2026-10-18 用户实测「第七集播完、第八集开播，历史还是
    /// 第七集；第八集十几秒才上报 5 秒」就是这两个错位叠出来的。
    pub(in crate::home) fn spawn_history_heartbeat(&mut self, cx: &mut Context<Self>) {
        let interval = Duration::from_secs(HISTORY_HEARTBEAT_SECS);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                // 视图已销毁（窗口关闭）就退出循环
                if this
                    .update(cx, |view, _| view.record_watch_history(false))
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();
    }

    /// 观看历史记录（issue #10）：本地必记 + 登录态顺路上报云端（走
    /// `history_service::record`，与 Tauri record_watch_history 命令同一段
    /// service）。
    ///
    /// 节拍在 [`super::history_cadence::HistoryCadence`]：新剧 5 秒首报、
    /// 同剧切集立即翻页、每 60 秒一拍；`force`（暂停/切集/离开）越过节拍
    /// 直接收尾，但仍守「<5 秒的掠过不进历史」的建行门槛——除非本集已经
    /// 报过（同剧切集的立即翻页走的就是这条，0.2 秒也要落）。
    fn record_watch_history(&mut self, force: bool) {
        // 拖动进度条期间位置在连跳：等松手落定再报，否则一次拖动打出十几条
        if !force && self.scrubbing.is_some() {
            return;
        }
        let Some((handle, now)) = self.player.as_ref().zip(self.now.clone()) else {
            return;
        };
        if now.vid.trim().is_empty() {
            return;
        }
        let (pos, dur) = (handle.position_secs(), handle.duration_secs());
        let position_ms = (pos * 1000.0).round() as i64;
        let episode_key = format!("{}:{}", now.series_id, now.vid_index);
        if force {
            let eligible =
                position_ms >= FIRST_REPORT_MS || self.history_cadence.has_reported(&episode_key);
            if !eligible {
                return;
            }
            self.history_cadence
                .note(&episode_key, &now.series_id, position_ms);
        } else if !self
            .history_cadence
            .due(&episode_key, &now.series_id, position_ms)
        {
            return;
        }
        let item = hongguo_common::model::LocalHistoryItem {
            series_id: now.series_id.clone(),
            title: now.title.clone(),
            cover: now.cover.clone(),
            vid_index: now.vid_index,
            vid: now.vid.clone(),
            position_ms,
            duration_ms: (dur * 1000.0).round() as i64,
            episode_cnt: now.episode_count as u32,
            updated_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
        };
        let state = Arc::clone(&self.state);
        bridge::spawn(async move {
            if let Err(e) = hongguo_service::history_service::record(&state, &item).await {
                log::warn!("[Home] 观看历史记录失败（忽略）: {e}");
            }
        });
        log::debug!(
            "[Home] 观看历史记录（{}:{}, {pos:.1}s, force={force}）",
            now.series_id,
            now.vid_index
        );
    }
}
