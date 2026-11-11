//! 设备生命周期装配。启动注册和档案复用由两壳共用的 [`hongguo_service::device`] 处理。

use serde::Serialize;

use hongguo_service::app_state::AppState;
use hongguo_service::device::{BootstrapMeta, read_meta};

/// 设备状态（前端/诊断可见）。
#[derive(Debug, Clone, Serialize)]
pub struct DeviceStatus {
    pub iid: String,
    pub device_id: String,
    pub source: String,
    pub previous_iid: Option<String>,
    /// 备份行里的上一代档案 iid（与 previous_iid 互证；None = 没换过档）。
    pub backup_iid: Option<String>,
    pub last_register_attempt_ms: Option<i64>,
    pub last_register_error: Option<String>,
}

/// 启动装配入口：后台补齐缺失的注册身份。
pub fn init(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::Manager;
    let state = app.state::<AppState>().inner().clone();
    hongguo_service::device::arm_cold_start();
    tauri::async_runtime::spawn(async move {
        hongguo_service::device::ensure_registered(&state).await;
    });
    Ok(())
}

/// 汇总当前状态（状态命令用）。
pub fn status(state: &AppState) -> DeviceStatus {
    let meta = read_meta(&state.store);
    status_inner(state, &meta)
}

fn status_inner(state: &AppState, meta: &BootstrapMeta) -> DeviceStatus {
    let device = state.device();
    let backup_iid = state
        .store
        .device_profile_backup()
        .ok()
        .flatten()
        .map(|p| p.get("iid").to_string());
    DeviceStatus {
        iid: device.get("iid").to_string(),
        device_id: device.get("device_id").to_string(),
        source: if meta.source.is_empty() {
            "adopted".into()
        } else {
            meta.source.clone()
        },
        previous_iid: meta.previous_iid.clone(),
        backup_iid,
        last_register_attempt_ms: meta.last_register_attempt_ms,
        last_register_error: meta.last_register_error.clone(),
    }
}

/// 手动重试注册（设置页命令）：注册一次，成功换档 registered，
/// 失败维持现役并记录错误（错误已由 register_and_swap 写进元数据）。
pub async fn retry_register(state: &AppState) -> DeviceStatus {
    if hongguo_common::offline_qa::enabled() {
        log::info!("[Device] 离线 QA：忽略手动重试，不发起注册");
        return status(state);
    }
    let _ = hongguo_service::device::register_and_swap(state, "手动重试").await;
    status(state)
}
