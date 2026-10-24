//! 转码能力查询与播放兼容兜底。
//!
//! 真正的转码实现在 [`hongguo_media::mux`] 与平台硬编 crate，由合并功能
//! （`merge_service::compat`）与播放兼容兜底（`play_service::compat_play`）驱动。
//! 兜底编排（下载/转码两段进度）在 service 的
//! `play_service::compat_play::compat_for_playback`；这里只留能力查询与转调。
//! 缓存清理已并入 storage_cmd（`get_cache_usage` / `clear_cache`）。

use tauri::State;

use hongguo_common::AppResult;
use hongguo_media_platform::capability::probe::DecodeCapability;
use hongguo_service::app_state::AppState;
use hongguo_service::play_service::compat_play::CompatPlayResult;

/// 硬解/软解能力。
#[tauri::command]
pub fn decode_capability() -> DecodeCapability {
    hongguo_media_platform::capability::probe::detect()
}

/// 丢弃缓存重新探测一次 ffmpeg 与编码器。
///
/// 探测一次要跑 `-version` + `-encoders` + 逐个试编一帧，一到两秒。**用户装完
/// ffmpeg 之后进程的环境变量不会更新**，不重探就一直报「未检测到」——只能重启
/// 应用才生效。给出这条路后设置页就能点一下解决。
#[tauri::command]
pub fn redetect_capability() -> DecodeCapability {
    hongguo_media_platform::capability::probe::redetect()
}

/// 把一集转成 H.264 供本机解不了的机器播放。
///
/// 播放器确认「有声音没画面」后调它；转好的产物落进兼容缓存，同一集只转一次。
/// 已下载的走本地文件，没下载的现取现解；进度经 `compat-play-progress` 推送。
#[tauri::command]
pub async fn transcode_for_playback(
    state: State<'_, AppState>,
    series_id: String,
    vid_index: u32,
    vid: Option<String>,
) -> AppResult<CompatPlayResult> {
    hongguo_service::play_service::compat_play::compat_for_playback(
        state.inner(),
        &series_id,
        vid_index,
        vid,
    )
    .await
}
