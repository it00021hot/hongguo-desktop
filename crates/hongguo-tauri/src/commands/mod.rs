//! Tauri command 薄层：只做参数校验与转调 service，不含业务。
//!
//! 与 `service/` 同名同构，看目录就能定位 command → service 的对应关系。

pub mod app_cmd;
pub mod boss_key_cmd;
pub mod capability_cmd;
// 下载 command 拆成 mod.rs（查询）与 actions.rs（变更）
pub use download as download_cmd;
pub mod danmaku_cmd;
pub mod device_cmd;
pub mod discover_cmd;
pub mod download;
pub mod emoticon_cmd;
pub mod follow_cmd;
pub mod history_cmd;
pub mod incognito_cmd;
pub mod interact_cmd;
pub mod login_cmd;
pub mod merge_cmd;
pub mod play_cmd;
pub mod rank_cmd;
pub mod series_cmd;
pub mod settings_cmd;
pub mod storage_cmd;
pub mod transcode_cmd;
pub mod window_cmd;

#[cfg(test)]
mod tests {
    /// 2026-10-07 闪退回归：同步 command 在主线程的 IPC/协议回调里执行，
    /// 那里没有 Tokio 线程上下文——裸 `tokio::spawn` / `Handle::current` /
    /// `block_on` 会当场 panic，而 panic 穿不过 objc 边界，直接 abort 全
    /// 进程（隐身模式开启即闪退那次就是这么来的）。命令层起后台任务优先
    /// 走 service 的共用任务运行时（`hongguo_service::runtime`，任意线程
    /// 可调）；需要 Tauri 设施（窗口/对话框等）时用 Tauri 的 async_runtime。
    /// async fn 里 `tokio::time::sleep` 这类**在运行时内**的调用不在禁止
    /// 之列；测试代码允许自建 runtime。
    ///
    /// 源码清单由目录自动发现（[`crate::testing::source_guard`]），不再手工登记——
    /// 新增 command 文件漏登记曾让本守护静默失效。
    #[test]
    fn commands_do_not_touch_tokio_context_directly() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands");
        let sources = crate::testing::source_guard::collect_sources(&dir);
        assert!(!sources.is_empty(), "没扫到命令层源码：{}", dir.display());
        for (path, text) in &sources {
            // 内联测试段允许自建 runtime，先截掉再扫
            let runtime_src = text.split("#[cfg(test)").next().unwrap_or(text);
            for banned in [
                "tokio::spawn(",
                "tokio::runtime",
                "Handle::current(",
                "block_on(",
            ] {
                assert!(
                    !runtime_src.contains(banned),
                    "命令层不允许裸用 {banned}：主线程无 Tokio 上下文，会 panic→abort。\n\
                     起后台任务/等异步结果请用 service 的共用任务运行时（hongguo_service::runtime）。出处：{}",
                    path.display()
                );
            }
        }
    }
}
