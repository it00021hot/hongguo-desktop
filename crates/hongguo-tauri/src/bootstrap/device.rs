//! 设备生命周期（adopt → 自动注册/健康探针 →
//! 失效轮换 → 旧档留底）。
//!
//! 三层（生命周期实现下沉在 [`hongguo_service::device`]，双壳共用）：
//! 1. **adopt（启动同步）**：读库里的现役档案；命中阵亡名单 / 缺 cdid 的
//!    远古快照直接重铺匿名档案（不走网络）。
//! 2. **自动注册（启动后台）**：现役无设备身份（匿名/全新安装）→ 注册；
//!    有身份的打搜索探针，空响应（死设备）→ 转注册。注册失败进 24h 退避。
//! 3. **轮换**：注册（74232 活形态）优先，被rect退出
//!    匿名档案兜底。手动重试绕过退避。
//!
//! 每次换档：旧档复制进 `device_profile` 备份行（`device.json.bak`
//! 同款），轮换元数据（来源 / 上次注册尝试与错误）落
//! 元数据行，供状态命令与退避判定使用。

use serde::Serialize;

use hongguo_service::app_state::AppState;
use hongguo_service::device::{BootstrapMeta, read_meta, write_meta};

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

/// 轮换：优先注册新设备（74232 活形态），被拒
/// （风控冷却期）退匿名档案。`force` 绕过注册退避（手动重试用）。
pub async fn rotate(state: &AppState, force: bool, reason: &str) -> DeviceStatus {
    let mut meta = read_meta(&state.store);
    let reg = hongguo_service::device::register_and_swap(state, force, reason).await;
    if reg.ok {
        return status_inner(state, &meta);
    }
    log::info!("[Device] 注册不可用（{reason}），重铺匿名档案");
    repave_anonymous(state, &mut meta).await
}

/// 匿名档案重铺。现役已是匿名（无 iid）时不折腾。
async fn repave_anonymous(state: &AppState, meta: &mut BootstrapMeta) -> DeviceStatus {
    if state.device().get("iid").is_empty() {
        return status_inner(state, meta);
    }
    let profile = hongguo_signer::anonymous_profile(&hongguo_signer::fresh_cdid());
    let old_iid = state.device().get("iid").to_string();
    if let Err(e) = state.store.backup_device_profile() {
        log::warn!("[Device] 旧档备份失败（继续换档）: {e}");
    }
    if let Err(e) = state.store.save_device_profile(&profile) {
        log::warn!("[Device] 新档落库失败（本次会话仍生效）: {e}");
    }
    meta.source = "anonymous".into();
    meta.previous_iid = Some(old_iid).filter(|i| i != &profile.get("iid").to_string());
    write_meta(&state.store, meta);
    state.replace_device(profile);
    status_inner(state, meta)
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

/// 手动重试注册（设置页命令）：绕过退避注册一次，成功换档 registered，
/// 失败维持现役并记录错误。
pub async fn retry_register(state: &AppState) -> DeviceStatus {
    let mut meta = read_meta(&state.store);
    let reg = hongguo_service::device::register_and_swap(state, true, "手动重试").await;
    if !reg.ok {
        // register_and_swap 已把错误写进元数据
        let _ = &mut meta;
        return status_inner(state, &read_meta(&state.store));
    }
    status_inner(state, &meta)
}

// 重铺/换档逻辑由 hongguo_service::device 的测试覆盖（adopt_sync 同一套
// store 语义）；壳侧只多出「探针判定 + 异步装配」，无独立可测纯函数。
// 直连真实接口的注册探针已随 register2 废除一并移除。
