//! 事件契约：[`PlayerEvent`] 引擎通报与 [`OpenOptions`] 打开参数。

use tokio::sync::mpsc;

/// 引擎事件（经调用方给的通道送达；通道关闭 = 宿主已弃，引擎自停）。
#[derive(Debug, Clone)]
pub enum PlayerEvent {
    /// 首帧已进上屏槽（宿主据此触发下一部预取）。
    FirstFrame { width: usize, height: usize },
    /// 新一帧可上屏，**帧随事件一起送达**。
    ///
    /// 不带帧、让宿主回头去 [`PlayerHandle::latest_frame`](super::PlayerHandle::latest_frame) 取，会有致命竞态：
    /// 宿主在「引擎就位」时清一次帧槽，首帧事件若先于就位到达，清空就把
    /// 「已出画」的事实抹掉了，封面永远收不掉。帧跟着事件走，宿主只认事件
    /// 里的帧，顺序天然正确。
    Frame(hongguo_media_platform::platform::video::VideoFrame),
    /// 缓冲断流：设备已暂停、时钟冻结（水位见 [`BUFFER_LOW_SECS`](super::feeder::BUFFER_LOW_SECS)）。
    Buffering,
    /// 断流恢复：设备重新起跑。
    Ready,
    /// 自然播完。
    Ended,
    /// 致命失败（引擎已停）。
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

/// 打开参数。
pub struct OpenOptions {
    pub source: PlaySource,
    /// 起播媒体时间（秒）：续播位置。0 = 从头。
    pub start_secs: f64,
    /// 起播倍速（用户偏好）：引擎**出生即按它走**——WSOLA 从第一包就按目标
    /// 倍速伸缩、时钟轴从 0 起就是目标倍速。若留给宿主起播后补发
    /// `set_rate`，环里已预填的 1x 内容会触发分级换轴（时钟按旧速率推进
    /// 到残量放完），起播头一截画面就是慢的——「每集刚播放时倍速不对」。
    pub rate: f64,
    /// 事件通道。
    pub events: mpsc::Sender<PlayerEvent>,
}
