//! 应用服务层。
//!
//! 与 [`crate::commands`] 同名同构：看目录就能定位 command → service 的对应关系。

pub mod download_service;
pub mod merge_service;
pub mod play_service;
pub mod series_service;
pub mod settings_service;
pub mod storage_service;
pub mod transcode_service;
