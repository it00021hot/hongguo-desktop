#[cfg(target_os = "ios")]
fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let directory = std::path::PathBuf::from(objc2_foundation::NSHomeDirectory().to_string())
        .join("Library")
        .join("Application Support")
        .join("hongguo-nsh");
    if let Err(error) = hongguo_nsh::configure_mobile_data_root(&directory) {
        panic!("NSH iOS host initialization failed: {error}");
    }
    nsh_platform_ios::set_ui_sets(vec![nsh_base::registry(), hongguo_nsh::registry()]);
    nsh_platform_ios::set_components(hongguo_nsh::__nsh_native_components_ios);
    nsh_platform_ios::run_app(
        nsh_platform_ios::AppSource::Rust(
            nsh_platform_ios::Root::of::<hongguo_nsh::App>().app_id("com.hongguo.nsh"),
        ),
        nsh_platform_ios::AppOptions {
            data_dir: Some(directory),
            ..Default::default()
        },
    );
}

#[cfg(not(target_os = "ios"))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(2)
}
