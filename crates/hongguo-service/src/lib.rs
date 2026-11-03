//! 应用服务层：各域 `*_service`、[`device`]、全局状态 [`app_state`]；后台任务走 [`runtime`]，
//! 业务事件走 [`events`]。本 crate **不依赖任何 UI 框架**，与壳的 commands 同名同构。

pub mod app_state;
pub mod capability_service;
pub mod device;
pub mod download_service;
pub mod events;
pub mod follow_service;
pub mod history_service;
pub mod login_service;
pub mod merge_service;
pub mod play_service;
pub mod runtime;
pub mod series_service;
pub mod settings_service;
pub mod storage_service;
pub mod transcode_service;

pub use app_state::{AppState, AppStateInner};

#[cfg(test)]
mod tests {
    /// 红线：service 层禁止裸 `tokio::spawn` / `Handle::current` / `block_on`——无 Tokio 上下文的线程会 panic→abort，后台任务一律走 `crate::runtime::spawn`。
    #[test]
    fn services_do_not_touch_tokio_context_directly() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let sources = collect_sources(&src);
        assert!(
            !sources.is_empty(),
            "没扫到 service 层源码：{}",
            src.display()
        );
        for (path, text) in &sources {
            let runtime_src = text.split("#[cfg(test)").next().unwrap_or(text);
            for banned in ["tokio::spawn(", "Handle::current(", "block_on("] {
                assert!(
                    !runtime_src.contains(banned),
                    "service 层不允许裸用 {banned}：无 Tokio 上下文的调用点会 panic→abort。\n\
                     起后台任务请用 crate::runtime::spawn。出处：{}",
                    path.display()
                );
            }
        }
    }

    /// 红线：service 不引用任何 UI 框架——源码里不允许出现 tauri 路径引用。
    #[test]
    fn service_does_not_reference_tauri() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let sources = collect_sources(&src);
        assert!(!sources.is_empty(), "没扫到 service 层源码");
        for (path, text) in &sources {
            let runtime_src = text.split("#[cfg(test)").next().unwrap_or(text);
            for banned in ["tauri::", "use tauri"] {
                assert!(
                    !runtime_src.contains(banned),
                    "service 层不允许引用 Tauri（{banned}）：壳相关能力请走 events/runtime 或壳侧适配。出处：{}",
                    path.display()
                );
            }
        }
    }

    /// 递归收集 `dir` 下所有生产态 `.rs`（跳过 `*_tests.rs` 兄弟测试文件）；
    /// 与壳的 `testing::source_guard` 同一份逻辑，各自内联，改口径要同步。
    fn collect_sources(dir: &std::path::Path) -> Vec<(std::path::PathBuf, String)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<(std::path::PathBuf, String)>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                    continue;
                }
                let is_rs = path.extension().and_then(|ext| ext.to_str()) == Some("rs");
                let is_test_file = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with("_tests.rs"));
                if is_rs
                    && !is_test_file
                    && let Ok(text) = std::fs::read_to_string(&path)
                {
                    out.push((path, text));
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, &mut out);
        out
    }
}
