//! 播放后端开关（`VIDEO_BACKEND=system|gpui`）。
//!
//! 默认 `gpui`（自研解码/上屏链路）；`system` 走 macOS 系统播放器
//! （AVPlayer + AVPlayerLayer），是 opt-in。进程级读取一次：切换后端要重启
//! 客户端，和 `make gpui` 的进程模型一致。
//!
//! 非 macOS 目标上 `system` 静默回落 `gpui`——系统播放器实现只存在于
//! `av_presenter`（macOS 专属）；Windows 对齐是后续阶段。

use std::sync::OnceLock;

const ENV: &str = "VIDEO_BACKEND";

static SYSTEM: OnceLock<bool> = OnceLock::new();

/// 是否启用系统播放器。只认 `VIDEO_BACKEND=system`，其余（含未设置）都是
/// GPUI 后端。
pub(crate) fn is_system() -> bool {
    *SYSTEM.get_or_init(|| {
        let requested = std::env::var(ENV).is_ok_and(|value| value == "system");
        let enabled = requested && cfg!(target_os = "macos");
        if requested && !enabled {
            log::warn!("[VideoBackend] 当前平台没有系统播放器实现，回落 gpui");
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

    /// 默认值必须是 gpui：VIDEO_BACKEND 未设置时回滚路径不依赖任何平台分支。
    #[test]
    fn unset_env_means_gpui() {
        if std::env::var(ENV).is_err() {
            assert!(!is_system());
        }
    }
}
