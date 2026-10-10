//! 官方 App 接口。

pub mod calendar;
pub mod client;
pub mod danmaku;
pub mod detail;
pub mod discover;
pub mod history;
pub mod interact;
pub mod login;
pub mod new_drama;
pub mod params;
pub mod play_url;
pub mod rank;
pub mod recommend;
pub mod register;
pub mod reservation;
pub mod search;
pub mod stream_pick;

/// 抓包/调试转储目录：`HG_CAPTURE_DIR` 环境变量可覆盖，默认系统临时目录
/// 下的 `hg_capture/`（跨平台，Windows 旧机的绝对路径已废弃）。
pub fn capture_dir() -> std::path::PathBuf {
    std::env::var_os("HG_CAPTURE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("hg_capture"))
}
