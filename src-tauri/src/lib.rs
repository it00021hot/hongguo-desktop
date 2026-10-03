//! 红果短剧下载器 —— 应用装配入口。
//!
//! 本文件只做「装配」：注册插件、注入状态、注册协议、挂载 command。
//! 具体业务逻辑一律在 [`signer`] / [`domain`] / [`service`] 各自的模块里，
//! 不在这里出现——入口文件一旦开始写业务，规模就会失控。

// 全部模块都私有：本 crate 只有一个消费方（本文件的 `run`），没有 bin 目标
// 也没有外部依赖 crate，把 `domain` / `signer` 之类暴露成 `pub` 只会让人
// 以为可以从 crate 外部直接调它们。
mod app_state;
mod bootstrap;
mod commands;
mod domain;
mod error;
mod media;
mod protocol;
mod service;
mod signer;
mod store;

/// 运行应用。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 不装 logger 的话全树 `log::` 调用都是空操作，抓取/下载失败会静默消失。
    // 调试时用 RUST_LOG=debug 打开详细日志。
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // 数据库打不开（目录建不了、schema 迁移失败）就别带病起窗口：
    // 用户在空库上改的设置，等下次旧库恢复时会静默丢掉。
    let state = match store::Store::open(store::paths::db_file()) {
        Ok(db) => {
            // 设备档案：库里没有就落一份静态兜底档案（设备注册 M2b 落位后，
            // 这里读到的会是注册产物）。
            let device = match db.device_profile() {
                Ok(Some(p)) => p,
                Ok(None) => {
                    let fallback = signer::video_device();
                    if let Err(e) = db.save_device_profile(&fallback) {
                        log::warn!("[Store] 静态设备档案落库失败（不影响启动）: {e}");
                    }
                    fallback
                }
                Err(e) => {
                    log::warn!("[Store] 设备档案读取失败，用静态兜底: {e}");
                    signer::video_device()
                }
            };
            std::sync::Arc::new(app_state::AppStateInner::with_device(db, device))
        }
        Err(e) => {
            log::error!("[Store] 数据库打开失败，终止启动: {e}");
            std::process::exit(1);
        }
    };

    let builder = tauri::Builder::default()
        // 单实例锁要第一个注册：抢在窗口创建之前，第二个进程才不会拉起第二套 UI。
        // 双开的直接危害是两个进程互写同一个存储——下载记录会随机消失。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state);

    // 自定义协议要在 setup 之前注册（Builder 阶段）
    let builder = protocol::register::register(builder);

    builder
        .setup(|app| {
            // 启动装配的顺序即依赖顺序：
            // 先加载数据，再从磁盘补回丢失的任务记录，然后把待跑任务推入调度，
            // 最后探测转码能力。
            bootstrap::store::init(app.handle())?;
            bootstrap::rescan::init(app.handle())?;
            bootstrap::downloader::init(app.handle())?;
            bootstrap::transcoder::init()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // 应用
            commands::app_cmd::select_folder,
            commands::app_cmd::open_folder,
            commands::app_cmd::open_external_page,
            // 设置
            commands::settings_cmd::get_settings,
            commands::settings_cmd::save_settings,
            commands::settings_cmd::test_proxy,
            // 剧集
            commands::series_cmd::get_series_list,
            commands::series_cmd::get_series_episodes,
            commands::series_cmd::resolve_series,
            commands::series_cmd::get_series_extras,
            commands::series_cmd::remove_series,
            commands::series_cmd::remove_all_series,
            // 发现（推荐信息流）
            commands::discover_cmd::discover_feed,
            commands::discover_cmd::web_cover,
            // 排行榜 / 新剧 / 搜索 / 预约（2026-10 抓包端点）
            commands::rank_cmd::rank_list,
            commands::rank_cmd::new_drama_list,
            commands::rank_cmd::search_series_cmd,
            commands::rank_cmd::reservation_list,
            // 弹幕
            commands::danmaku_cmd::danmaku_list,
            // 浏览与搜索
            commands::browse_cmd::browse_categories,
            commands::browse_cmd::browse_list,
            commands::browse_cmd::search_series,
            // 下载
            commands::download_cmd::get_download_tasks,
            commands::download_cmd::get_queue_status,
            commands::download_cmd::download_batch,
            commands::download_cmd::pause_all,
            commands::download_cmd::resume_all,
            commands::download_cmd::stop_download,
            commands::download_cmd::retry_task,
            commands::download_cmd::retry_tasks,
            commands::download_cmd::delete_tasks,
            commands::download_cmd::rescan_downloads,
            // 合并
            commands::merge_cmd::get_merge_tasks,
            commands::merge_cmd::get_merge_candidates,
            commands::merge_cmd::delete_merge_task,
            commands::merge_cmd::open_merge_output,
            commands::merge_cmd::merge_preflight,
            commands::merge_cmd::merge_series,
            // 播放
            commands::play_cmd::play_series,
            commands::play_cmd::save_playback_position,
            commands::play_cmd::get_playback_history,
            commands::play_cmd::remove_playback_record,
            commands::play_cmd::clear_playback_history,
            // 转码（合并功能用；播放兜底已下线）
            commands::transcode_cmd::decode_capability,
            commands::transcode_cmd::redetect_capability,
            commands::transcode_cmd::transcode_for_playback,
            commands::transcode_cmd::clear_compat_cache,
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
