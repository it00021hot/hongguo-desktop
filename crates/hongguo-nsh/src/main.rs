//! Runs the Rust application directly on the machine's native backend.

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(target_os = "windows")]
use nsh_platform_windows as platform;

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn main() -> impl std::process::Termination {
    env_logger::init();
    #[cfg(target_os = "macos")]
    platform::set_components(hongguo_nsh::__nsh_native_components_macos);
    #[cfg(target_os = "windows")]
    platform::set_components(hongguo_nsh::__nsh_native_components_windows);
    platform::run_app(
        platform::AppSource::Rust(
            platform::Root::of::<hongguo_nsh::App>().app_id(&hongguo_nsh::metadata().id),
        ),
        platform::AppOptions {
            title: hongguo_nsh::metadata().name.clone(),
            data_dir: std::env::var_os("GUAVA_DATA_DIR").map(std::path::PathBuf::from),
            app_id: hongguo_nsh::metadata().id.clone(),
            #[cfg(target_os = "windows")]
            output_color_mode: platform::OutputColorMode::Auto,
            size: (
                hongguo_nsh::metadata().window.default_size.width,
                hongguo_nsh::metadata().window.default_size.height,
            ),
            ..Default::default()
        },
    )
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(2)
}
