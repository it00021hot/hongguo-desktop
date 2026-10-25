//! 官方 App 接口客户端（hongguo-api crate）。
//!
//! 每个接口域一个目录（`mod` + `model` [+ `parse`]）；改任何端点或参数前
//! 必读 `docs/hongguo-api-endpoints.md`（抓包唯一事实源）。
//!
//! 签名由 `hongguo-signer` 产出；改动 url / body / query 时注意「签完不能再动」
//! 的铁律（见 signer）。本 crate 不依赖 tauri。

pub mod calendar;
pub mod client;
pub mod danmaku;
pub mod detail;
pub mod discover;
pub mod emoticon;
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
