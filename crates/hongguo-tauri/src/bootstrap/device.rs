//! 设备生命周期装配（adopt → 自动注册/健康探针 → 旧档留底；生命周期实现下沉在 [`hongguo_service::device`]，双壳共用）。
//!
//! 启动后台：现役无设备身份（匿名/全新安装）→ 注册；旧注册代际档案一次性迁移；代际正常打搜索探针，空响应（死设备）→ 转注册；网络类失败随恢复就地重试无退避。每次换档旧档复制进 `device_profile` 备份行，轮换元数据（来源/上次注册尝试与错误）落元数据行供状态命令与诊断使用。

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

/// 启动装配入口：拉起后台设备生命周期（匿名/死档案自动注册）。
pub fn init(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::Manager;
    let state = app.state::<AppState>().inner().clone();
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
    let _ = hongguo_service::device::register_and_swap(state, "手动重试").await;
    status(state)
}

// 重铺/换档逻辑由 hongguo_service::device 的测试覆盖（adopt_sync 同一套 store 语义）；壳侧只多出「探针判定 + 异步装配」，无独立可测纯函数。
