//! 红果桌面版 —— 应用装配入口。
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
mod diagnostics;
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
    // panic 取证要抢在一切事故之前装好：主线程 FFI 回调里的 panic 会直接
    // abort，stderr 又常常没接着终端，crash.log 是唯一能留下的现场。
    diagnostics::install_panic_hook();

    // ⚠️ 启动顺序：**数据库在 .setup() 里打开，必须排在单实例插件之后**。
    // 插件在 Builder::build 阶段初始化并劝退第二个实例；若把开库放在这
    // 之前，再次启动（上一实例还活着——隐身隐藏/小窗模式现在很常见）
    // 的进程会先撞上库锁，被误报成「数据文件损坏」然后退出。
    let builder = tauri::Builder::default()
        // 单实例锁要第一个注册：抢在窗口创建与数据库打开之前，第二个进程
        // 才不会拉起第二套 UI / 撞库锁。双开的直接危害是两个进程互写同一
        // 个存储——下载记录会随机消失。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            bring_back_active_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // 窗口关闭拦截的「前端就绪」闸门（见 setup 里的 CloseRequested 处理）
        .manage(commands::app_cmd::WindowCloseGate::default());

    // dev 专属自动化端口（127.0.0.1:4445）：macOS 的 WKWebView 没有对外调试
    // 协议，W3C WebDriver 由 app 内嵌服务器实现（依赖无条件编译，cargo 不支持
    // 按 debug_assertions 选依赖）。release 构建此行整体不存在，不留任何自动化面。
    #[cfg(debug_assertions)]
    let builder = builder.plugin(tauri_plugin_webdriver::init());

    // 自定义协议要在 setup 之前注册（Builder 阶段）
    let builder = protocol::register::register(builder);

    builder
        .setup(|app| {
            // 启动装配的顺序即依赖顺序：
            // 先建应用状态（含数据库），再从磁盘补回丢失的任务记录，然后把
            // 待跑任务推入调度，最后探测转码能力。
            app.manage(build_app_state());
            bootstrap::store::init(app.handle())?;
            bootstrap::rescan::init(app.handle())?;
            bootstrap::downloader::init(app.handle())?;
            bootstrap::transcoder::init()?;

            // 窗口关闭一律先问前端：自绘 ×、Alt+F4、任务栏关闭都走
            // CloseRequested，前端 ready 后拦截并让它弹退出确认框
            // （退出会掐断正在跑的下载任务）；没 ready（前端白屏/崩溃）
            // 就放行——宁可少问一句，绝不能把窗口变成关不掉。
            use std::sync::atomic::Ordering;
            use tauri::{Emitter, Manager};
            if let Some(window) = app.get_webview_window("main") {
                let ready = app
                    .state::<commands::app_cmd::WindowCloseGate>()
                    .inner()
                    .clone();
                let win = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        if ready.load(Ordering::Acquire) {
                            api.prevent_close();
                            let _ = win.emit("close-requested", ());
                        }
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
            commands::app_cmd::enter_mini_screen,
            commands::app_cmd::exit_mini_screen,
            commands::app_cmd::set_always_on_top,
            commands::app_cmd::set_incognito,
            // 设置
            commands::settings_cmd::get_settings,
            commands::settings_cmd::save_settings,
            commands::settings_cmd::test_proxy,
            // 剧集
            commands::series_cmd::get_series_list,
            commands::series_cmd::get_series_episodes,
            commands::series_cmd::resolve_series,
            commands::series_cmd::related_series,
            commands::series_cmd::series_meta,
            commands::series_cmd::remove_series,
            commands::series_cmd::remove_all_series,
            // 发现（首页推荐流 / 找剧筛选浏览）
            commands::discover_cmd::recommend_feed,
            commands::discover_cmd::browse_panel,
            commands::discover_cmd::browse_page,
            // 排行榜 / 新剧 / 搜索 / 预约（2026-10 抓包端点）
            commands::rank_cmd::rank_list,
            commands::rank_cmd::new_drama_list,
            commands::rank_cmd::search_series_cmd,
            commands::rank_cmd::search_suggest_cmd,
            commands::rank_cmd::reservation_list,
            commands::rank_cmd::reservation_reserve,
            commands::rank_cmd::new_drama_calendar,
            // 云端观看历史
            commands::history_cmd::watch_history_list,
            commands::history_cmd::cloud_report_progress,
            // 登录
            commands::login_cmd::login_send_code,
            commands::login_cmd::login_sms_login,
            commands::login_cmd::login_mfa_verify,
            commands::login_cmd::login_mfa_cancel,
            commands::login_cmd::login_status,
            commands::login_cmd::login_user_info,
            commands::login_cmd::login_logout,
            // 弹幕
            commands::danmaku_cmd::danmaku_list,
            commands::danmaku_cmd::comment_list,
            commands::danmaku_cmd::series_comment_list,
            // 互动（点赞/收藏/发弹幕/回复，2026-10-05/06 抓包端点）
            commands::interact_cmd::danmaku_send,
            commands::interact_cmd::comment_send,
            commands::interact_cmd::comment_reply,
            commands::interact_cmd::video_digg,
            commands::interact_cmd::comment_digg,
            commands::interact_cmd::series_collect,
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
        .build(tauri::generate_context!())
        .expect("启动失败")
        .run(|app, event| {
            // macOS 点 Dock 图标：隐身模式把窗口整个藏起来后，鼠标唤不回
            // （隐藏窗口不参与命中测试），Dock 是系统级的恢复入口。
            // `Reopen` 变体只存在于 macOS 构建。
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                bring_back_active_window(app);
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (app, &event);
            }
        });
}

/// 把主窗口带回来（隐身 hide 后的恢复入口，Dock 点击/再次启动共用）。
fn bring_back_active_window(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// 打开数据库并组装应用状态。失败即终止启动（不带病起窗口：用户在空库
/// 上改的设置，等下次旧库恢复时会静默丢掉）。
///
/// 只在 `.setup()` 里调用——必须晚于单实例插件，否则上一实例还活着时
/// （隐身隐藏/小窗模式）这里会撞库锁。锁冲突给一次短暂重试（真双开的
/// 败者本该在插件阶段就被劝退，走到这里多半是极端竞态），仍锁着就带着
/// 明确指引退出，而不是误报「数据文件损坏」。
fn build_app_state() -> std::sync::Arc<app_state::AppStateInner> {
    let db = match open_store_with_lock_retry() {
        Ok(db) => db,
        Err(error::AppError::StoreLocked(e)) => {
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
    // 设备档案：库里没有就落一份静态兜底档案（设备注册 M2b 落位后，
    // 这里读到的会是注册产物）。旧档案的版本身份要对齐到当前客户端
    // 版本——服务端按自报 version_code 分发功能 schema（排行榜选项表
    // 在老版本号下退化为扁平结构），真实设备升级 app 也是同理。
    let device = match db.device_profile() {
        Ok(Some(mut p)) => {
            signer::device::align_app_version(&mut p);
            p
        }
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

/// 开库；被锁时稍等重试一次（覆盖两实例几乎同时启动、败者尚未退出的窗口期）。
fn open_store_with_lock_retry() -> error::AppResult<store::Store> {
    match store::Store::open(store::paths::db_file()) {
        Ok(db) => Ok(db),
        Err(error::AppError::StoreLocked(first)) => {
            log::warn!("[Store] 数据库暂时被锁（{first}），1s 后重试一次");
            std::thread::sleep(std::time::Duration::from_secs(1));
            store::Store::open(store::paths::db_file())
        }
        Err(e) => Err(e),
    }
}

/// 并发分配压力测试：与业务无关的纯 alloc/free 风暴。
///
/// 背景：`rusty_h264-common` 默认给整个测试进程强装了第三方分配器
/// `rusty_alloc`（feature unification 关不掉），它在高并发下存在
/// 间歇性崩溃（表现为荒唐大小的分配失败 → abort）。这个测试用来
/// 复现与回归验证：**只要它崩，就是分配器的锅**，别在业务代码里找。
#[cfg(test)]
mod alloc_stress {
    use std::sync::Barrier;

    /// 模拟 mp4 流式解密测试的分配指纹：高频短命小块（几十~几百字节）、
    /// 精确 with_capacity、Vec 增长 realloc、周期性整块清空。
    /// **与业务无关**——它若崩，唯一嫌疑就是进程级第三方分配器
    /// （rusty_h264-common 默认强装的 rusty_alloc）。
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
                        // 精确容量分配 + 局部写满（streaming_tests 的主模式）
                        let mut v: Vec<(u64, u64)> = Vec::with_capacity(n / 16);
                        for i in 0..n / 16 {
                            v.push((rng.wrapping_add(i as u64), i as u64));
                        }
                        total = total.wrapping_add(v.len());
                        // 短命小块换手
                        let s = vec![0x5Au8; 8 + (rng % 128) as usize];
                        total = total.wrapping_add(s.len());
                        // 偶发增长式 realloc
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
