//! 播放后端开关（`VIDEO_BACKEND=system|gpui`）。
//!
//! macOS 与 Windows 默认 `system`：前者 `AVPlayer` + `AVPlayerLayer`、后者
//! Media Foundation Media Engine + DirectComposition 视频位（都已实测出画，
//! 见 `docs/gpui-system-player-status.md` 与 `docs/gpui-system-player-windows.md`）。
//! `VIDEO_BACKEND=gpui` 显式回滚到自研上屏。进程级读取一次：切换后端要重启
//! 客户端，和 `make gpui` 的进程模型一致。
//!
//! 其它目标一律 `gpui`——系统播放器实现只存在于各平台的 presenter 模块。

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

const ENV: &str = "VIDEO_BACKEND";

/// 有系统播放器实现的目标。
const SUPPORTED: bool = cfg!(any(target_os = "macos", target_os = "windows"));

static SYSTEM: AtomicBool = AtomicBool::new(false);
static INIT: OnceLock<()> = OnceLock::new();

/// 是否启用系统播放器。macOS/Windows 默认启用；`VIDEO_BACKEND=gpui` 回滚。
pub(crate) fn is_system() -> bool {
    INIT.get_or_init(|| {
        let rolled_back = std::env::var(ENV).is_ok_and(|value| value == "gpui");
        let enabled = !rolled_back && SUPPORTED;
        if rolled_back && SUPPORTED {
            log::info!("[VideoBackend] VIDEO_BACKEND=gpui：回滚到自研上屏");
        }
        log::info!(
            "[VideoBackend] 播放后端 = {}",
            match (enabled, cfg!(target_os = "macos")) {
                (true, true) => "system（AVPlayer）",
                (true, false) => "system（MediaEngine）",
                (false, _) => "gpui",
            }
        );
        SYSTEM.store(enabled, Ordering::SeqCst);
    });
    SYSTEM.load(Ordering::SeqCst)
}

/// 系统播放器起不来时的后端降级：进程内永久翻到 gpui。
/// 返回 true 表示这次真的翻了（调用方据此重走一次起播）。
pub(crate) fn fall_back_to_gpui() -> bool {
    is_system() && SYSTEM.swap(false, Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 未设置时默认走系统后端；显式 gpui 才回滚。
    #[test]
    fn unset_env_means_system() {
        if std::env::var(ENV).is_err() {
            assert_eq!(is_system(), SUPPORTED);
        }
    }
}
