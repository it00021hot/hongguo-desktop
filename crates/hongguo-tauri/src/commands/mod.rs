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
    /// 同步 command 在主线程 IPC/协议回调里没有 Tokio 上下文——裸 `tokio::spawn`/`Handle::current`/`block_on` 会 panic，穿不过 objc 边界直接 abort 全进程。
    /// 命令层起后台任务优先走 service 共用运行时 `hongguo_service::runtime`（需要 Tauri 设施时用 Tauri 的 async_runtime）；源码清单由 [`crate::testing::source_guard`] 目录自动发现。
    #[test]
    fn commands_do_not_touch_tokio_context_directly() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands");
        let sources = crate::testing::source_guard::collect_sources(&dir);
        assert!(!sources.is_empty(), "没扫到命令层源码：{}", dir.display());
        for (path, text) in &sources {
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
