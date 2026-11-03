//! 磁盘占用与文件清理。

use tauri::State;

use hongguo_common::AppResult;
use hongguo_service::app_state::AppState;
use hongguo_service::storage_service::{
    cache::{self, CacheClearResult, CacheUsage},
    cleanup, usage,
    usage::{StorageSeriesUsage, StorageUsage},
};

/// 本地占用统计。
#[tauri::command]
pub fn get_storage_usage(state: State<'_, AppState>) -> StorageUsage {
    usage::usage(&state)
}

/// 三处缓存（在线内存流 / 兼容转码 / 封面转码）的占用总览。
#[tauri::command]
pub fn get_cache_usage() -> CacheUsage {
    cache::usage()
}

/// 一键清空全部缓存。cover-cache 此前没有任何清理入口，别再拆回多个按钮。
#[tauri::command]
pub fn clear_cache() -> CacheClearResult {
    cache::clear()
}

/// 按剧聚合的本地占用（清理页剧列表，只含磁盘上真有文件的剧）。
#[tauri::command]
pub fn get_storage_series(state: State<'_, AppState>) -> Vec<StorageSeriesUsage> {
    usage::series_usage(&state)
}

/// 删除某部剧的全部本地文件（含合并产物与残留临时文件）；剧集档案保留，之后仍可在线播放或重新下载。
#[tauri::command]
pub fn delete_series_files(state: State<'_, AppState>, series_id: String) -> AppResult<usize> {
    cleanup::delete_series(&state, &series_id)
}

/// 删除单集文件。
#[tauri::command]
pub fn delete_episode_file(
    state: State<'_, AppState>,
    series_id: String,
    vid_index: u32,
) -> AppResult<bool> {
    cleanup::delete_episode(&state, &series_id, vid_index)
}

/// 删除全部已下载。
#[tauri::command]
pub fn delete_all_downloaded(state: State<'_, AppState>) -> AppResult<usize> {
    cleanup::delete_all(&state)
}
