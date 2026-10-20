//! 播放后端开关（`VIDEO_BACKEND=system|gpui`）。
//!
//! macOS 默认 `system`（AVPlayer + AVPlayerLayer）；`VIDEO_BACKEND=gpui`
//! 显式回滚到自研解码/上屏链路。进程级读取一次：切换后端要重启客户端，
//! 和 `make gpui` 的进程模型一致。
//!
//! 非 macOS 目标上一律 `gpui`——系统播放器实现只存在于 `av_presenter`
//! （macOS 专属）；Windows 对齐是后续阶段。

use std::sync::OnceLock;

const ENV: &str = "VIDEO_BACKEND";

static SYSTEM: OnceLock<bool> = OnceLock::new();

/// 是否启用系统播放器。macOS 默认启用；`VIDEO_BACKEND=gpui` 显式回滚。
pub(crate) fn is_system() -> bool {
    *SYSTEM.get_or_init(|| {
        let rolled_back = std::env::var(ENV).is_ok_and(|value| value == "gpui");
        let enabled = !rolled_back && cfg!(target_os = "macos");
        if rolled_back && cfg!(target_os = "macos") {
            log::info!("[VideoBackend] VIDEO_BACKEND=gpui：显式回滚到自研上屏");
        }
        log::info!(
            "[VideoBackend] 播放后端 = {}",
            if enabled {
                "system（AVPlayer）"
            } else {
                "gpui"
            }
        );
        enabled
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// macOS 未设置时默认 system；显式 gpui 才回滚。
    #[test]
    fn unset_env_means_system_on_macos() {
        if std::env::var(ENV).is_err() {
            assert_eq!(is_system(), cfg!(target_os = "macos"));
        }
    }
}
