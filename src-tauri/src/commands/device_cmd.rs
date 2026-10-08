//! 设备生命周期 command：状态查询 + 手动重注册。
//!
//! 前端可拿状态做「设备身份」展示/诊断（hgplayer 同款 registering/ready/
//! error 三态的读侧）；重试按钮对齐它的 RetryDeviceRegister。

use tauri::State;

use crate::app_state::AppState;
use crate::bootstrap::device::{DeviceStatus, retry_register, status};
use crate::error::AppResult;

/// 当前设备状态（来源 / iid / 上次注册尝试与错误）。
#[tauri::command]
pub async fn device_status(state: State<'_, AppState>) -> AppResult<DeviceStatus> {
    Ok(status(state.inner()))
}

/// 手动重试设备注册（绕过 24h 退避）。成功换注册档案；失败维持现役，
/// 错误记入状态。
#[tauri::command]
pub async fn device_retry_register(state: State<'_, AppState>) -> AppResult<DeviceStatus> {
    let state = state.inner().clone();
    Ok(retry_register(&state).await)
}
