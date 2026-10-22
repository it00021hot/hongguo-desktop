//! 观看历史上报节拍 + 播放进度落盘（GPUI `history_cadence.rs` /
//! `playback/progress.rs` 的同规则移植）。
//!
//! 1 秒心跳按播放位置判节拍（不挂 5 秒落盘窗——那套写法实测出「首报落在
//! 5–10 秒 / 云端进度整集停在首报值」两个错位，见 GPUI 同名文件头）：
//! 1. 新剧播满 5 秒才建行（信息流快滑不落噪点行）；
//! 2. 同剧切集立即翻页（历史行按剧聚合）；
//! 3. 同一集每 60 秒一拍，回跳超容差立即补；
//! 4. 暂停不计时；暂停沿 / 切集 / 换剧 / 外部播放请求由 [`flush`] force 收尾。
//!
//! 上报走 `history_service::record`（本地必记 + 登录态顺路上报云端）；
//! 进度另按 5 秒节流写 `store` 播放位置表（续播锚点、收藏/追更的
//! 「继续播放」都吃它）。

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use nsh_core::ui::prelude::*;

use super::Stage;
use super::cadence::FIRST_REPORT_MS;

/// 播放进度落盘节流（秒）。
const POSITION_SAVE_EVERY_SECS: u64 = 5;

/// 当前集的 (vid, vid_index)（分集表就位才有；空 vid = 还没就位，不报）。
fn current_episode(s: &Stage) -> Option<(String, u32)> {
    let index = s.current_index.get();
    s.episodes
        .get()
        .into_iter()
        .find(|e| e.vid_index == index)
        .map(|e| (e.vid, e.vid_index))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 1 秒心跳：暂停沿 force 收尾；播放中按节流落进度、按节拍记历史。
pub(crate) fn spawn_heartbeat(s: Stage) {
    let was_playing = stored(false);
    let last_save = stored(Instant::now());
    spawn(async move {
        loop {
            timer(std::time::Duration::from_secs(1)).await;
            let playing = s.playing.get();
            // 播放→暂停沿：force 收尾一条（用户按停 / 隐身暂停都落到这）。
            if was_playing.replace(playing) && !playing {
                flush(&s);
            }
            if !playing {
                continue;
            }
            if last_save.with(|t| t.elapsed().as_secs()) >= POSITION_SAVE_EVERY_SECS {
                last_save.replace(Instant::now());
                save_position(s.clone());
            }
            record(s.clone(), false);
        }
    });
}

/// force 收尾：进度立即落盘 + 历史立即记一条（切集/换剧/暂停沿/外部请求）。
pub(crate) fn flush(s: &Stage) {
    save_position(s.clone());
    record(s.clone(), true);
}

/// 进度落盘（`store` 播放位置表）：续播锚点、收藏/追更「继续播放」的数据源。
fn save_position(s: Stage) {
    let Some((vid, vid_index)) = current_episode(&s) else {
        return;
    };
    let series_id = s.series_id.get();
    let position = s.time.get() as f64;
    let duration = s.duration.get() as f64;
    if series_id.is_empty() || vid.is_empty() || position <= 0.0 {
        return;
    }
    spawn(async move {
        let state = crate::state::state();
        let store = state.store.clone();
        let result = background(async move {
            tokio::task::spawn_blocking(move || {
                hongguo_service::play_service::position::save_store(
                    &store, &series_id, vid_index, position, duration,
                )
            })
            .await
            .map_err(|e| format!("进度落盘任务失败: {e}"))
            .and_then(|inner| inner.map_err(|e| e.to_string()))
        })
        .await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("[Player] 进度落盘失败: {e}"),
            Err(e) => log::warn!("[Player] 进度落盘任务失败: {e}"),
        }
    });
}

/// 观看历史记录（本地必记 + 登录态顺路上报云端，`history_service::record`）。
/// `force`（暂停/切集/离开）越过节拍，但仍守「<5 秒的掠过不建行」——除非
/// 本集已经报过（同剧切集的立即翻页走的就是这条，0.2 秒也要落）。
pub(crate) fn record(s: Stage, force: bool) {
    let Some((vid, vid_index)) = current_episode(&s) else {
        return;
    };
    if vid.trim().is_empty() {
        return;
    }
    let series_id = s.series_id.get();
    if series_id.is_empty() {
        return;
    }
    // 拖动进度条期间位置在连跳：交给节拍的回跳容差兜，不在拖动里刷行。
    let position_ms = (s.time.get() * 1000.0).round() as i64;
    let duration_ms = (s.duration.get() * 1000.0).round() as i64;
    let episode_key = format!("{series_id}:{vid_index}");
    let due = {
        let mut cadence = s.history_cadence.with(|v| v.clone());
        if force {
            let eligible = position_ms >= FIRST_REPORT_MS || cadence.has_reported(&episode_key);
            if !eligible {
                return;
            }
            cadence.note(&episode_key, &series_id, position_ms);
            s.history_cadence.replace(cadence);
            true
        } else {
            let due = cadence.due(&episode_key, &series_id, position_ms);
            if due {
                s.history_cadence.replace(cadence);
            }
            due
        }
    };
    if !due {
        return;
    }
    let item = hongguo_common::model::LocalHistoryItem {
        series_id: series_id.clone(),
        title: s.info.title.get(),
        cover: s.backdrop.get(),
        vid_index,
        vid,
        position_ms,
        duration_ms,
        episode_cnt: s.info.episode_count.get(),
        updated_at_ms: now_ms(),
    };
    log::info!(
        "[Player] 观看历史记录（{}:{}, {}ms, force={force}）",
        item.series_id,
        item.vid_index,
        item.position_ms
    );
    spawn(async move {
        let state = crate::state::state();
        let result =
            background(
                async move { hongguo_service::history_service::record(&state, &item).await },
            )
            .await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("[Player] 观看历史记录失败: {e}"),
            Err(e) => log::warn!("[Player] 观看历史记录任务失败: {e}"),
        }
    });
}
