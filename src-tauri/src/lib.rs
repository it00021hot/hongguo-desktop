//! 红果短剧下载器 —— 应用装配入口。
//!
//! 本文件只做「装配」：注册插件、注入状态、注册协议、挂载 command。
//! 具体业务逻辑一律在 [`signer`] / [`domain`] / [`service`] 各自的模块里，
//! 不在这里出现——入口文件一旦开始写业务，规模就会失控。

// `domain` 与 `signer` 对 bin 目标（probe_api / verify_episode）公开，
// 内部模块仍保持私有，避免 command 层之外误用。
pub mod domain;
pub mod signer;

mod app_state;
mod bootstrap;
mod commands;
mod error;
mod media;
mod protocol;
mod service;
mod sniff;
mod store;

/// 运行应用。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 不装 logger 的话全树 `log::` 调用都是空操作，嗅探/下载失败会静默消失。
    // 调试时用 RUST_LOG=debug 打开详细日志。
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(app_state::AppState::default());

    // 自定义协议要在 setup 之前注册（Builder 阶段）
    let builder = protocol::register::register(builder);

    builder
        .setup(|app| {
            // 启动装配的顺序即依赖顺序：
            // 先加载数据，再注册嗅探窗口，最后恢复未完成的下载。
            bootstrap::store::init(app.handle())?;
            bootstrap::sniff::init(app.handle())?;
            bootstrap::downloader::init(app.handle())?;
            bootstrap::transcoder::init(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // 应用
            commands::app_cmd::get_app_info,
            commands::app_cmd::open_external_url,
            commands::app_cmd::select_folder,
            commands::app_cmd::show_in_folder,
            commands::app_cmd::open_folder,
            // 设置
            commands::settings_cmd::get_settings,
            commands::settings_cmd::save_settings,
            commands::settings_cmd::get_proxy_status,
            commands::settings_cmd::test_proxy,
            commands::settings_cmd::proxy_presets,
            // 剧集
            commands::series_cmd::get_series_list,
            commands::series_cmd::get_series_episodes,
            commands::series_cmd::resolve_series,
            commands::series_cmd::get_series_extras,
            commands::series_cmd::remove_series,
            commands::series_cmd::restore_dismissed_series,
            commands::series_cmd::dismissed_count,
            commands::series_cmd::purge_empty_series,
            // 浏览与搜索
            commands::browse_cmd::browse_categories,
            commands::browse_cmd::browse_list,
            commands::browse_cmd::search_series,
            commands::browse_cmd::set_search_window_visible,
            // 下载
            commands::download_cmd::get_download_tasks,
            commands::download_cmd::get_queue_status,
            commands::download_cmd::download_batch,
            commands::download_cmd::download_single_episode,
            commands::download_cmd::pause_all,
            commands::download_cmd::resume_all,
            commands::download_cmd::stop_download,
            commands::download_cmd::retry_task,
            commands::download_cmd::retry_tasks,
            commands::download_cmd::delete_tasks,
            commands::download_cmd::rescan_downloads,
            // 合并
            commands::merge_cmd::get_merge_tasks,
            commands::merge_cmd::delete_merge_task,
            commands::merge_cmd::cancel_merge,
            commands::merge_cmd::merge_preflight,
            commands::merge_cmd::merge_series,
            // 播放
            commands::play_cmd::play_series,
            commands::play_cmd::save_playback_position,
            commands::play_cmd::get_playback_position,
            commands::play_cmd::get_playback_history,
            // 转码（合并功能用；播放兜底已下线）
            commands::transcode_cmd::decode_capability,
            commands::transcode_cmd::compat_cache_status,
            commands::transcode_cmd::clear_compat_cache,
            commands::transcode_cmd::online_cache_status,
            commands::transcode_cmd::clear_online_cache,
            // 存储
            commands::storage_cmd::get_storage_usage,
            commands::storage_cmd::delete_series_files,
            commands::storage_cmd::delete_episode_file,
            commands::storage_cmd::delete_all_downloaded,
        ])
        .run(tauri::generate_context!())
        .expect("启动失败");
}
