//! 事件契约：[`PlayerEvent`] 播放器通报与 [`PlaySource`] 播放源。

/// 播放器事件（经调用方给的通道送达；通道关闭 = 宿主已弃，播放器自停）。
#[derive(Debug, Clone)]
pub enum PlayerEvent {
    /// 首帧已出画（宿主据此触发下一部预取、结算切集耗时、置「画面已就绪」）。
    FirstFrame { width: usize, height: usize },
    /// 缓冲断流：设备已暂停、时钟冻结。
    Buffering,
    /// 断流恢复：设备重新起跑。
    Ready,
    /// 自然播完。
    Ended,
    /// 致命失败（播放器已停）。
    Failed(String),
}

/// 播放源：在线流或本地成品文件。
///
/// 本地成品 = 已下载的那一集（Tauri `play_service::local::resolve_play` 的
/// 「本地优先」对位）：离线可播、秒开，且**没有档位概念**（本地只有一版，
/// 清晰度菜单该是空的）。
#[derive(Clone, Debug)]
pub enum PlaySource {
    /// 在线流：调用方已 `prepare` 过，缓存里有这一档。
    Online { vid: String, definition: u32 },
    /// 本地成品文件路径。
    Local { path: std::path::PathBuf },
}
