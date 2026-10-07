//! Tauri command 薄层：只做参数校验与转调 service，不含业务。
//!
//! 与 `service/` 同名同构，看目录就能定位 command → service 的对应关系。

pub mod app_cmd;
pub mod browse_cmd;
// 下载 command 拆成 mod.rs（查询）与 actions.rs（变更）
pub use download as download_cmd;
pub mod danmaku_cmd;
pub mod discover_cmd;
pub mod download;
pub mod history_cmd;
pub mod interact_cmd;
pub mod login_cmd;
pub mod merge_cmd;
pub mod play_cmd;
pub mod rank_cmd;
pub mod series_cmd;
pub mod settings_cmd;
pub mod storage_cmd;
pub mod transcode_cmd;

#[cfg(test)]
mod tests {
    /// 2026-10-07 闪退回归：同步 command 在主线程的 IPC/协议回调里执行，
    /// 那里没有 Tokio 线程上下文——裸 `tokio::spawn` / `Handle::current` /
    /// `block_on` 会当场 panic，而 panic 穿不过 objc 边界，直接 abort 全
    /// 进程（隐身模式开启即闪退那次就是这么来的）。命令层起后台任务、
    /// 等异步结果一律走 `tauri::async_runtime`（内部用全局运行时，任意
    /// 线程可调）。async fn 里 `tokio::time::sleep` 这类**在运行时内**的
    /// 调用不在禁止之列；测试代码允许自建 runtime。
    #[test]
    fn commands_do_not_touch_tokio_context_directly() {
        const SOURCES: &[&str] = &[
            include_str!("app_cmd.rs"),
            include_str!("browse_cmd.rs"),
            include_str!("danmaku_cmd.rs"),
            include_str!("discover_cmd.rs"),
            include_str!("history_cmd.rs"),
            include_str!("interact_cmd.rs"),
            include_str!("login_cmd.rs"),
            include_str!("merge_cmd.rs"),
            include_str!("play_cmd.rs"),
            include_str!("rank_cmd.rs"),
            include_str!("series_cmd.rs"),
            include_str!("settings_cmd.rs"),
            include_str!("storage_cmd.rs"),
            include_str!("transcode_cmd.rs"),
            include_str!("download/mod.rs"),
            include_str!("download/actions.rs"),
            include_str!("download/cleanup.rs"),
        ];
        for src in SOURCES {
            let runtime_src = src.split("#[cfg(test)").next().unwrap_or(src);
            for banned in [
                "tokio::spawn(",
                "tokio::runtime",
                "Handle::current(",
                "block_on(",
            ] {
                assert!(
                    !runtime_src.contains(banned),
                    "命令层不允许裸用 {banned}：主线程无 Tokio 上下文，会 panic→abort。\n\
                     起后台任务/等异步结果请用 tauri::async_runtime。出处片段：\n{runtime_src}"
                );
            }
        }
    }
}
