//! Tauri 壳层装配：插件注册、全局状态注入、自定义协议挂载、command 挂载——[`prepare`] 产出装好的 `Builder`；`build(generate_context!)` 与 `.run()` 留在 `crates/hongguo-tauri-app`（`generate_context` 必须与 tauri.conf.json 同 crate）。
//!
//! GPUI 壳（`hongguo-gpui`）不消费本 crate——它直接构造 `hongguo_service::AppStateInner` 并调用 service，绕开 IPC 与 WebView 供数。

pub mod bootstrap;
pub mod commands;
mod diagnostics;
mod events;
mod protocol;
#[cfg(test)]
mod testing;
mod window_state;

use hongguo_common::{AppError, AppResult};

/// 装配 Tauri 应用（插件、状态、协议、command）。在 `build(context)` 之前调用。
pub fn prepare() -> tauri::Builder<tauri::Wry> {
    // 不装 logger 全树 log:: 调用都是空操作、失败会静默消失；调试用 RUST_LOG=debug 开详细日志
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    // panic 取证抢在一切事故之前装好：主线程 FFI 回调里的 panic 直接 abort，stderr 常没接着终端，crash.log 是唯一能留下的现场
    diagnostics::install_panic_hook();

    // ⚠️ 启动顺序：数据库在 .setup() 里打开，必须排在单实例插件之后——否则二次启动（上一实例还活着）先撞库锁，被误报「数据文件损坏」。
    // 移动端门控：单实例与全局快捷键插件官方在 Android/iOS 整 crate cfg 掉，注册必须按平台门控（移动端无多实例与系统级快捷键语义）。
    let builder = tauri::Builder::default();
    // 单实例锁第一个注册：抢在窗口创建与数据库打开之前
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        bring_back_active_window(app);
    }));
    let builder = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // 更新：安装包经 GitHub Releases 的 latest.json 分发；进程插件只用到 relaunch
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init());
    // 老板键全局快捷键：注册/注销走 Rust 侧，不经前端 → 无需 capabilities
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());
    let builder = builder
        // 窗口关闭拦截的「前端就绪」闸门（见 setup 里的 CloseRequested 处理）
        .manage(commands::app_cmd::WindowCloseGate::default());

    // dev 专属自动化端口（127.0.0.1:4445）：WKWebView 没有对外调试协议，W3C WebDriver 由 app 内嵌服务器实现；
    // release 构建此行整体不存在，不留任何自动化面。
    #[cfg(debug_assertions)]
    let builder = builder.plugin(tauri_plugin_webdriver::init());

    // 自定义协议要在 setup 之前注册（Builder 阶段）
    let builder = protocol::register(builder);

    builder
        .setup(|app| {
            // Android 没有桌面数据目录语义（dirs::data_dir() 返回 None），不注入数据根会落到不可写的 /data/local/tmp；必须在 build_app_state 之前钉到 app 私有目录
            #[cfg(target_os = "android")]
            {
                use tauri::Manager as _;
                let root = app.path().app_data_dir()?;
                hongguo_store::paths::set_absolute_data_root(&root);
                // tempfile 在 Unix 只认 TMPDIR（Android 缺省 /tmp 不存在且 app 无权写，迁移 SQL 直接 I/O error 被误报「数据损坏」；
                // SQLITE_TMPDIR 是 SQLite C 库的约定，turso 不读），钉到 app 私有 tmp 目录
                let tmpdir = root.join("tmp");
                let _ = std::fs::create_dir_all(&tmpdir);
                // SAFETY: setup 在主线程、任何并发读 env 的线程尚未启动，set_var 的多线程 UB 条件不成立；tempfile 建临时目录时读 TMPDIR，必须在首次开库前设置
                unsafe {
                    std::env::set_var("TMPDIR", &tmpdir);
                }
                log::info!(
                    "[Store] Android 数据根: {}（TMPDIR: {}）",
                    root.display(),
                    tmpdir.display()
                );
            }
            // 启动装配的顺序即依赖顺序；设备健康复检 / 磁盘补登记 / 转码探测不拦启动，各自在 init 里后台化（见 bootstrap/mod.rs）
            let app_state = build_app_state();
            app.manage(app_state.clone());
            bootstrap::store::init(app.handle())?;
            bootstrap::device::init(app.handle())?;
            bootstrap::events::init(app.handle())?;
            bootstrap::rescan::init(app.handle())?;
            bootstrap::downloader::init(app.handle())?;
            bootstrap::transcoder::init()?;
            bootstrap::status_log::init()?;
            // 追更更新检查：启动查一轮 + 每 20 分钟一轮（未登录空转，登录后自动生效）
            hongguo_service::follow_service::start_notice_loop(app_state.clone());
            // 老板键按持久化设置注册（被占用只记日志，不拦启动）
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::boss_key_cmd::init(app.handle(), &app_state.settings().boss_key);
            // 本地观看历史补传（issue #10）：带账号启动时把离线攒下的行推云端
            if app_state.settings().account.is_some() {
                let st = app_state.clone();
                hongguo_service::runtime::spawn(async move {
                    hongguo_service::history_service::upload_pending(&st).await;
                });
            }

            // 窗口关闭一律先问前端：CloseRequested 被拦截后前端弹退出确认框（退出会掐断正在跑的下载任务）；
            // 前端没 ready（白屏/崩溃）就放行——宁可少问一句，绝不能把窗口变成关不掉
            use std::sync::atomic::Ordering;
            use tauri::{Emitter, Manager};
            if let Some(window) = app.get_webview_window("main") {
                // 窗口还藏着（mark_window_ready 才亮出）就先恢复上次几何，再挂防抖落盘
                window_state::restore(&window);
                // 红绿灯按 Cursor 公式钉进 44px 顶栏（tao 配置位不够，见 window_cmd）
                #[cfg(target_os = "macos")]
                commands::window_cmd::align_traffic_lights(app.handle());
                let ready = app
                    .state::<commands::app_cmd::WindowCloseGate>()
                    .inner()
                    .clone();
                let win = window.clone();
                // WebView2 最小化时不派发 visibilitychange，前端拿不到——Rust 侧在 Resized 里查 is_minimized，状态翻转才 emit
                let minimized = std::sync::atomic::AtomicBool::new(false);
                // 几何防抖的 epoch：移动/缩放事件递增，saver 线程 500ms 轮询、变了才快照写盘
                let geometry_epoch = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
                window_state::spawn_saver(win.clone(), std::sync::Arc::clone(&geometry_epoch));
                // 兜底显示用的副本先克隆：win/ready 都要 move 进下面的事件闭包
                let fallback_ready = ready.clone();
                let fallback_window = win.clone();
                let close_save_window = win.clone();
                let close_save_epoch = std::sync::Arc::clone(&geometry_epoch);
                #[cfg(target_os = "macos")]
                let align_app = app.handle().clone();
                window.on_window_event(move |event| {
                    match event {
                        tauri::WindowEvent::CloseRequested { .. } => {
                            // 关闭即存（防抖线程未必轮到下一拍进程就退了）
                            window_state::save(&close_save_window);
                        }
                        tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                            close_save_epoch.fetch_add(1, std::sync::atomic::Ordering::Release);
                            // AppKit 在缩放时把红绿灯打回默认位，再钉回 44px 正中
                            #[cfg(target_os = "macos")]
                            commands::window_cmd::align_traffic_lights(&align_app);
                        }
                        // 窗口获焦（含启动亮窗后首次成为 key）时 AppKit 会重排标题栏，灯被打回默认位
                        #[cfg(target_os = "macos")]
                        tauri::WindowEvent::Focused(true) => {
                            commands::window_cmd::align_traffic_lights(&align_app);
                        }
                        _ => {}
                    }
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event
                        && ready.load(Ordering::Acquire)
                    {
                        api.prevent_close();
                        let _ = win.emit("close-requested", ());
                        return;
                    }
                    if let tauri::WindowEvent::Resized(_) = event
                        && let Ok(now) = win.is_minimized()
                    {
                        let prev = minimized.swap(now, std::sync::atomic::Ordering::AcqRel);
                        if prev != now
                            && let Err(e) = win.emit("window-minimize", now)
                        {
                            log::warn!("[Window] 最小化事件发送失败: {e}");
                        }
                    }
                });

                // 兜底显示：窗口改由 mark_window_ready（前端挂载）亮出后，前端崩溃/加载失败会让它永远藏着——超时强制亮出，
                // 宁可亮出空窗（能关、能被单实例二次启动拉回）也不能无窗可关。dev 档拉长（Vite 冷启动要转换整个模块图，
                // 10s 必误触发亮黑窗）；release 前端秒级就绪，保持 10s。
                const FRONTEND_READY_FALLBACK_SECS: u64 =
                    if cfg!(debug_assertions) { 120 } else { 10 };
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(
                        FRONTEND_READY_FALLBACK_SECS,
                    ));
                    if !fallback_ready.load(Ordering::Acquire) {
                        log::warn!(
                            "[Window] 前端 {FRONTEND_READY_FALLBACK_SECS}s 未就绪，兜底显示窗口"
                        );
                        let _ = fallback_window.show();
                        let _ = fallback_window.set_focus();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // 应用
            commands::app_cmd::select_folder,
            commands::app_cmd::open_folder,
            commands::app_cmd::open_external_page,
            commands::app_cmd::mark_window_ready,
            commands::app_cmd::exit_app,
            commands::boss_key_cmd::set_boss_key,
            commands::window_cmd::enter_mini_screen,
            commands::window_cmd::exit_mini_screen,
            commands::window_cmd::set_always_on_top,
            commands::window_cmd::enter_portrait_screen,
            commands::window_cmd::exit_portrait_screen,
            commands::window_cmd::fit_portrait_screen,
            commands::window_cmd::set_portrait_full,
            commands::window_cmd::set_traffic_lights_visible,
            commands::incognito_cmd::set_incognito,
            // 设置
            commands::settings_cmd::get_settings,
            commands::settings_cmd::save_settings,
            commands::settings_cmd::default_cache_dir,
            commands::settings_cmd::test_proxy,
            // 剧集
            commands::series_cmd::get_series_episodes,
            commands::series_cmd::get_series_snapshot,
            commands::series_cmd::resolve_series,
            commands::series_cmd::related_series,
            commands::series_cmd::series_meta,
            // 发现（首页推荐流 / 找剧筛选浏览）
            commands::discover_cmd::recommend_tabs,
            commands::discover_cmd::recommend_feed,
            commands::discover_cmd::browse_panel,
            commands::discover_cmd::browse_page,
            // 排行榜 / 新剧 / 搜索 / 预约
            commands::rank_cmd::rank_list,
            commands::rank_cmd::new_drama_list,
            commands::rank_cmd::search_series_cmd,
            commands::rank_cmd::search_suggest_cmd,
            commands::rank_cmd::reservation_list,
            commands::rank_cmd::reservation_reserve,
            commands::rank_cmd::reservations_delete,
            commands::rank_cmd::new_drama_calendar,
            // 云端观看历史
            commands::history_cmd::watch_history_list,
            commands::history_cmd::watch_history_delete,
            // 登录
            commands::login_cmd::login_send_code,
            commands::login_cmd::login_sms_login,
            commands::login_cmd::login_mfa_verify,
            commands::login_cmd::login_mfa_cancel,
            commands::login_cmd::login_status,
            commands::login_cmd::login_user_info,
            commands::login_cmd::login_logout,
            // 设备生命周期（状态 / 手动重注册）
            commands::device_cmd::device_status,
            commands::device_cmd::device_retry_register,
            // 弹幕
            commands::danmaku_cmd::danmaku_window,
            commands::danmaku_cmd::comment_list,
            commands::danmaku_cmd::series_comment_list,
            commands::danmaku_cmd::series_review_send,
            commands::danmaku_cmd::comment_replies,
            commands::danmaku_cmd::review_replies,
            commands::danmaku_cmd::creator_profile,
            commands::danmaku_cmd::creator_works,
            // 互动（点赞/收藏/发弹幕/回复）
            commands::interact_cmd::danmaku_send,
            commands::interact_cmd::comment_send,
            commands::interact_cmd::comment_reply,
            commands::interact_cmd::video_digg,
            commands::interact_cmd::comment_digg,
            commands::interact_cmd::review_digg,
            commands::interact_cmd::review_reply_send,
            commands::interact_cmd::comment_delete,
            commands::interact_cmd::series_collect,
            commands::interact_cmd::series_collect_batch,
            // @小果AI 提及（评论区快捷图标）
            commands::interact_cmd::mention_list,
            // 表情（官方表情 / 第三方 GIF / 自定义表情）
            commands::emoticon_cmd::emoticon_list,
            commands::emoticon_cmd::emoticon_search,
            commands::emoticon_cmd::emoticon_upload,
            commands::emoticon_cmd::emoticon_pick_image,
            commands::emoticon_cmd::emoticon_collect,
            commands::emoticon_cmd::emoticon_delete,
            // 追更（开关 / 列表 / 是否已追 / 更新提醒）
            commands::follow_cmd::follow_update,
            commands::follow_cmd::follow_updates,
            commands::follow_cmd::is_following,
            commands::follow_cmd::follow_notices,
            commands::follow_cmd::follow_notices_read,
            commands::follow_cmd::follow_notices_clear,
            commands::follow_cmd::follow_check_updates,
            // 观看历史（云端 + 本地离线记录，issue #10）
            commands::history_cmd::record_watch_history,
            commands::history_cmd::local_history_list,
            commands::history_cmd::local_history_delete,
            commands::interact_cmd::interaction_state,
            commands::interact_cmd::bookshelf_list,
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
            commands::play_cmd::play_prefetch,
            commands::play_cmd::save_playback_position,
            commands::play_cmd::series_progress,
            // 播放解码能力（探测 / HEVC 扩展按需安装）
            commands::capability_cmd::playback_capability,
            commands::capability_cmd::install_hevc_extension,
            // 转码（合并功能用；播放兜底已下线）
            commands::transcode_cmd::decode_capability,
            commands::transcode_cmd::redetect_capability,
            commands::transcode_cmd::transcode_for_playback,
            // 存储
            commands::storage_cmd::get_storage_usage,
            commands::storage_cmd::get_storage_series,
            commands::storage_cmd::get_cache_usage,
            commands::storage_cmd::clear_cache,
            commands::storage_cmd::delete_series_files,
            commands::storage_cmd::delete_episode_file,
            commands::storage_cmd::delete_all_downloaded,
        ])
}

/// `.run()` 的事件回调（由壳转交）：处理需要进程级入口的事件。
pub fn on_run_event(app: &tauri::AppHandle, event: tauri::RunEvent) {
    // macOS 点 Dock 图标：隐身模式把窗口整个藏起来后鼠标唤不回（隐藏窗口不参与命中测试），Dock 是系统级的恢复入口
    #[cfg(target_os = "macos")]
    if let tauri::RunEvent::Reopen { .. } = event {
        bring_back_active_window(app);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, &event);
    }
}

/// 把主窗口带回来（隐身 hide 后的恢复入口，Dock 点击/再次启动共用）。
/// 只在桌面构建被引用（单实例回调 / macOS Reopen），移动端 cfg 掉避免 dead_code。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn bring_back_active_window(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// 打开数据库并组装应用状态，失败即终止启动（不带病起窗口）。只在 `.setup()` 里调用——必须晚于单实例插件；锁冲突给一次短暂重试，仍锁着就带明确指引退出，不误报「数据文件损坏」。
fn build_app_state() -> std::sync::Arc<hongguo_service::AppStateInner> {
    let db = match open_store_with_lock_retry() {
        Ok(db) => db,
        Err(AppError::StoreLocked(e)) => {
            log::error!(
                "[Store] 数据库被占用，终止启动: {e}\n\
                 另一个实例可能还在运行（隐身模式会把窗口整个藏掉）。\
                 在 Dock/任务栏里找到它，或结束残留进程后再启动。"
            );
            std::process::exit(1);
        }
        Err(e) => {
            log::error!("[Store] 数据库打开失败，终止启动: {e}");
            std::process::exit(1);
        }
    };
    // 设备档案：库里没有就落一份静态兜底档案；旧档案的版本身份要对齐到当前客户端版本——服务端按自报 version_code 分发功能 schema
    // （排行榜选项表在老版本号下退化为扁平结构）。阵亡名单淘汰重铺与启动后的健康探针、失效轮换在 [`bootstrap::device`]。
    let (device, device_source) = hongguo_service::device::adopt_sync(&db);
    log::info!(
        "[Device] 现役档案 iid={}（{device_source}）",
        device.get("iid")
    );
    std::sync::Arc::new(hongguo_service::AppStateInner::with_device(db, device))
}

fn open_store_with_lock_retry() -> AppResult<hongguo_store::Store> {
    hongguo_store::Store::open_with_lock_retry(hongguo_store::paths::db_file())
}

/// 并发分配压力测试：与业务无关的纯 alloc/free 风暴——它若崩，先怀疑分配/并发本身，别在业务代码里找。
#[cfg(test)]
mod alloc_stress {
    use std::sync::Barrier;

    #[test]
    fn concurrent_alloc_free_storm_is_stable() {
        const THREADS: usize = 16;
        const ITERS: usize = 200_000;
        let barrier = std::sync::Arc::new(Barrier::new(THREADS));
        let handles: Vec<_> = (0..THREADS)
            .map(|t| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let mut rng = (t as u64).wrapping_mul(0x9E3779B97F4A7C15) | 1;
                    let mut total = 0usize;
                    for _ in 0..ITERS {
                        rng ^= rng << 13;
                        rng ^= rng >> 7;
                        rng ^= rng << 17;
                        let n = 32 + (rng % 960) as usize;
                        let mut v: Vec<(u64, u64)> = Vec::with_capacity(n / 16);
                        for i in 0..n / 16 {
                            v.push((rng.wrapping_add(i as u64), i as u64));
                        }
                        total = total.wrapping_add(v.len());
                        let s = vec![0x5Au8; 8 + (rng % 128) as usize];
                        total = total.wrapping_add(s.len());
                        if rng.is_multiple_of(61) {
                            let mut g = Vec::new();
                            for _ in 0..64 {
                                g.extend_from_slice(&s);
                            }
                            total = total.wrapping_add(g.len());
                        }
                    }
                    total
                })
            })
            .collect();
        let sum: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
        assert!(sum > 0);
    }
}
