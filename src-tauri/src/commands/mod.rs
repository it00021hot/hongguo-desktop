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
pub mod merge_cmd;
pub mod play_cmd;
pub mod rank_cmd;
pub mod series_cmd;
pub mod settings_cmd;
pub mod storage_cmd;
pub mod transcode_cmd;
