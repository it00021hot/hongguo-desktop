//! 红果桌面版 NSH 壳（`docs/nsh-shell-plan.md`）。
//!
//! **操作与 Tauri 客户端一致**：装配序（开库 → 设备 adopt → 全局状态）、接口
//! 调用（`state.api_env()` + `hongguo_api`）与另外两壳逐层同源——本壳不含任何
//! 业务逻辑，只做窗口、装配与渲染。
//!
//! 与 GPUI 壳的差别只有执行模型：nsh 自带引擎线程与 tokio 运行时（
//! `background()` 就是 `tokio::spawn`），不需要 GPUI 那种 oneshot 桥；业务
//! 事件由 `pump` 一个转发任务泵进 nsh 信号。
//!
//! 本轮范围（P1 骨架）：真实 Store + 设备档案 + 事件泵 + 导航栈 + 两档窗口
//! 几何（普通 / 小窗）+ NSH_SCRIPT 冒烟。页面全量迁移按方案 §5 分期。

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(target_os = "windows")]
use nsh_platform_windows as platform;

mod app;
mod i18n;
mod nav;
mod platform_ext;
mod player;
mod pump;
mod state;
mod supply;
mod tokens;

use std::process::Termination;

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn main() -> impl Termination {
    // 数据根最先声明，且必须在单实例与开库之前：`set_shell_data_root` 是
    // OnceLock，只有第一次生效；与 Tauri / GPUI 的数据目录平级独立
    //（%APPDATA%\hongguo-nsh），互不锁库。
    hongguo_store::paths::set_shell_data_root("hongguo-nsh");
    init_logging();
    install_panic_log();
    // 单实例要早于开库：上一个实例可能正持库锁（小窗/后台播放都在跑），
    // 撞锁退出会让用户看到「点图标没反应」。第二个进程请已有实例前置窗口
    // 后安静退出。
    if !platform_ext::claim_single_instance() {
        log::info!("[Shell] 已有实例在运行：已请求它前置窗口，本进程退出");
        return std::process::ExitCode::SUCCESS;
    }
    state::build();
    platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<app::App>().theme(tokens::theme())),
        platform::AppOptions {
            title: "红果".to_owned(),
            size: (1280, 840),
            script: std::env::var("NSH_SCRIPT").ok(),
            limits: platform::Limits::standalone_desktop(),
            #[cfg(target_os = "windows")]
            app_id: "com.hongguo.desktop".to_owned(),
            ..Default::default()
        },
    )
}

/// 日志落数据目录（GUI 子系统没有控制台，这是唯一观测面）。时间戳带毫秒：
/// 卡顿诊断看的是亚秒级间隔，秒级精度会把「停顿 120ms」和「正常」印成一秒。
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn init_logging() {
    let dir = hongguo_store::paths::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("nsh-shell.log");
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(file) => {
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
                .format_timestamp_millis()
                .target(env_logger::Target::Pipe(Box::new(file)))
                .init();
            log::info!("[Shell] 日志输出到 {}", path.display());
        }
        Err(e) => {
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
                .format_timestamp_millis()
                .init();
            log::warn!(
                "[Shell] 日志文件 {} 打不开（{e}），改走 stderr",
                path.display()
            );
        }
    }
}

/// panic 留痕。不 abort：仓规禁止 release 关掉 unwind（`panic = "abort"` 会让
/// 协议层与 objc2 的 catch 静默失效），这里只负责让现场进日志。
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn install_panic_log() {
    std::panic::set_hook(Box::new(|info| {
        log::error!(
            "[Panic] 线程 {:?} panic：{info}",
            std::thread::current().name().unwrap_or("<unnamed>")
        );
    }));
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
compile_error!("NSH 壳只支持 macOS 与 Windows（见 docs/nsh-shell-plan.md）");
