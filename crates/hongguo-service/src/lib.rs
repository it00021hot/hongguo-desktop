//! 应用服务层：各域 `*_service`、[`device`]、全局状态 [`app_state`]；后台任务走 [`runtime`]，
//! 业务事件走 [`events`]。本 crate **不依赖任何 UI 框架**，与壳的 commands 同名同构。

pub mod app_state;
pub mod capability_service;
pub mod catalog_service;
mod cold_start;
pub mod comments_service;
pub mod contracts;
pub mod device;
pub mod download_service;
pub mod emoticon_service;
pub mod events;
pub mod follow_service;
pub mod history_service;
pub mod interaction_service;
pub mod library_service;
pub mod login_service;
pub mod merge_service;
pub mod personal_service;
pub mod play_service;
pub mod reservation_service;
pub mod runtime;
pub mod series_service;
pub mod settings_service;
pub mod storage_service;
pub mod transcode_service;

pub use app_state::{AppState, AppStateInner};
