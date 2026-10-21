//! Runs the Rust application directly on the machine's native backend.

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(target_os = "windows")]
use nsh_platform_windows as platform;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn main() -> impl std::process::Termination {
    // GUI 子系统（windows_subsystem）没有控制台：Windows 上日志落
    // exe 同目录的 hg-nsh-demo.log，否则测试盲跑。
    #[cfg(target_os = "windows")]
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Pipe(Box::new(
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open("hg-nsh-demo.log")
                .expect("日志文件创建失败"),
        )))
        .init();
    #[cfg(not(target_os = "windows"))]
    env_logger::init();
    platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<hongguo_nsh_demo::App>()),
        platform::AppOptions {
            title: "红果短剧 × NSH".to_owned(),
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            app_id: "dev.hongguo.nsh-demo".to_owned(),
            size: (430, 800),
            script: std::env::var("NSH_SCRIPT").ok(),
            ..Default::default()
        },
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
compile_error!(
    "Run this binary on macOS, Windows or Linux; use the TSX entry with nsh run for iOS and Android."
);
