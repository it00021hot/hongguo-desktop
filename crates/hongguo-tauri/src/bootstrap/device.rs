//! 设备生命周期（adopt → 健康探针 →
//! 失效轮换 → 旧档留底）。
//!
//! 三层：
//! 1. **adopt（启动同步）**：读库里的现役档案；命中阵亡名单 / 缺 cdid 的
//!    远古快照直接重铺静态兜底（不走网络）。
//! 2. **健康探针（启动后台）**：库里档案 ≠ 当前静态兜底时，拿现役档案打
//!    一次 reading 探针（找剧搜索是风控最严的一支，联想反而松，不能当
//!    探针）。空响应 = 死设备信号；再用静态档案对照一次，静态活而现役死
//!    才轮换——网络/服务端整体故障时按兵不动。
//! 3. **轮换**：优先注册新设备（[`register_device`]），被拒（风控冷却期
//!    `device_id=0`）退静态兜底。注册尝试带 24h 退避（服务端按量风控，
//!    冷却期内反复试只会加重），退避期内直接静态兜底。手动重试绕过退避。
//!
//! 每次换档：旧档复制进 `device_profile` 备份行（`device.json.bak`
//! 同款），轮换元数据（来源 / 上次注册尝试与错误）落
//! 元数据行，供状态命令与退避判定使用。

use serde::Serialize;

use hongguo_api::client::ApiEnv;
use hongguo_api::register::register_device;
use hongguo_api::search::search_series;
use hongguo_common::AppError;
use hongguo_common::utils::time::now_ms;
use hongguo_service::app_state::AppState;
use hongguo_service::device::{BootstrapMeta, read_meta, write_meta};
use hongguo_signer::device::{DeviceProfile, align_app_version};

/// 健康探针用的搜索词（探针只看「有没有非空响应」，词本身无关紧要）。
const PROBE_QUERY: &str = "热门";

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

/// 启动装配入口：拉起后台健康探针。
pub fn init(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::Manager;
    let state = app.state::<AppState>().inner().clone();
    tauri::async_runtime::spawn(async move {
        ensure_healthy(&state).await;
    });
    Ok(())
}

/// 健康探针结论。
enum Health {
    /// 非空响应，设备活着。
    Ok,
    /// 空响应（重试耗尽）——死设备信号。
    Dead,
    /// 网络/服务端问题或其它错误，分不清是谁的锅。
    Inconclusive,
}

async fn probe(env: &ApiEnv) -> Health {
    match search_series(PROBE_QUERY, 0, "", env).await {
        Ok(_) => Health::Ok,
        Err(AppError::EmptyResponse(_)) => Health::Dead,
        Err(_) => Health::Inconclusive,
    }
}

/// 后台健康复检：只探**带 iid 的注册档案**（匿名档案无设备身份可死，
/// 官方 7.4.2.32 匿名读播全量放行，无需复检）。现役探针空响应即判死，
/// 直接重铺匿名档案——register2 端点已废弃（device_id=0），不再尝试注册。
async fn ensure_healthy(state: &AppState) {
    let device = state.device();
    if device.get("iid").is_empty() {
        return;
    }
    let env = state.api_env();
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    if !matches!(probe(&env).await, Health::Dead) {
        return;
    }
    log::warn!(
        "[Device] 现役档案探针空响应（iid={}），重铺匿名档案",
        device.get("iid")
    );
    let mut meta = read_meta(&state.store);
    repave_anonymous(state, &mut meta).await;
}

/// 轮换：注册已退场（register2 被服务端废弃，恒 device_id=0），死设备
/// 一律重铺匿名档案。`force` 仅保留签名兼容（手动重试命令走 [`retry_register`]）。
/// 返回轮换后的状态。
pub async fn rotate(state: &AppState, force: bool, reason: &str) -> DeviceStatus {
    let _ = force;
    let mut meta = read_meta(&state.store);
    log::info!("[Device] 轮换（{reason}）：重铺匿名档案");
    repave_anonymous(state, &mut meta).await
}

/// 匿名档案重铺。现役已是匿名（无 iid）时不折腾。
async fn repave_anonymous(state: &AppState, meta: &mut BootstrapMeta) -> DeviceStatus {
    if state.device().get("iid").is_empty() {
        return status_inner(state, meta);
    }
    let profile = hongguo_signer::anonymous_profile(&hongguo_signer::fresh_cdid());
    swap(state, meta, profile, "anonymous").await
}

/// 换档三连：旧档进备份行 → 新档落库+热替换 → 元数据更新。
async fn swap(
    state: &AppState,
    meta: &mut BootstrapMeta,
    profile: DeviceProfile,
    source: &str,
) -> DeviceStatus {
    let old_iid = state.device().get("iid").to_string();
    let new_iid = profile.get("iid").to_string();
    if old_iid != new_iid {
        if let Err(e) = state.store.backup_device_profile() {
            log::warn!("[Device] 旧档备份失败（继续换档）: {e}");
        }
        if let Err(e) = state.store.save_device_profile(&profile) {
            log::warn!("[Device] 新档落库失败（本次会话仍生效）: {e}");
        }
        log::info!("[Device] 换档 {} → {}（{source}）", old_iid, new_iid);
    }
    meta.source = source.to_string();
    meta.previous_iid = Some(old_iid).filter(|i| i != &new_iid);
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

/// 手动重试注册（设置页命令）。签名档案用现役档案；register2 端点已废弃
/// （恒 device_id=0），失败维持现役并记录错误。
pub async fn retry_register(state: &AppState) -> DeviceStatus {
    let mut meta = read_meta(&state.store);
    let reg_env = ApiEnv {
        proxy: state.settings().proxy,
        device: state.device(),
        cookie: None,
        x_tt_token: None,
    };
    meta.last_register_attempt_ms = Some(now_ms());
    write_meta(&state.store, &meta);
    match register_device(&reg_env).await {
        Ok(r) => {
            let mut profile = hongguo_signer::anonymous_profile(&r.cdid);
            profile.set("device_id", &r.device_id);
            profile.set("iid", &r.install_id);
            profile.set("openudid", &r.openudid);
            align_app_version(&mut profile);
            profile.set_ttreq(Some(r.ttreq.clone()));
            swap(state, &mut meta, profile, "registered").await
        }
        Err(e) => {
            meta.last_register_error = Some(e.to_string());
            write_meta(&state.store, &meta);
            status_inner(state, &meta)
        }
    }
}

// 重铺/换档逻辑由 hongguo_service::device 的测试覆盖（adopt_sync 同一套
// store 语义）；壳侧只多出「探针判定 + 异步装配」，无独立可测纯函数。
// 直连真实接口的注册探针已随 register2 废除一并移除。

#[cfg(test)]
mod probe {
    use super::*;
    use hongguo_common::model::ProxyConfig;

    /// 全链路试注（预算内 1 次注册，register2 已废弃、恒 device_id=0，
    /// 仅留作端点存活性诊断）：注册 → 组档案 → 探针验活性。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_register_and_use() {
        let reg_env = ApiEnv {
            proxy: ProxyConfig::default(),
            device: hongguo_signer::anonymous_profile(&hongguo_signer::fresh_cdid()),
            cookie: None,
            x_tt_token: None,
        };
        match register_device(&reg_env).await {
            Ok(r) => {
                println!(
                    "[dev-boot] 注册成功 device_id={} install_id={} cdid={} ttreq={}",
                    r.device_id,
                    r.install_id,
                    r.cdid,
                    if r.ttreq.is_empty() { "无" } else { &r.ttreq }
                );
                let mut profile = hongguo_signer::anonymous_profile(&r.cdid);
                profile.set("device_id", &r.device_id);
                profile.set("iid", &r.install_id);
                profile.set("openudid", &r.openudid);
                align_app_version(&mut profile);
                profile.set_ttreq(Some(r.ttreq));
                let env = ApiEnv {
                    proxy: ProxyConfig::default(),
                    cookie: Some(hongguo_signer::device::anonymous_cookie(&profile)),
                    device: profile,
                    x_tt_token: None,
                };
                match search_series("热门", 0, "", &env).await {
                    Ok(p) => println!("[dev-boot] 新档案搜索 {} 条，档案可用", p.items.len()),
                    Err(e) => println!("[dev-boot] 新档案搜索失败: {e}"),
                }
            }
            Err(e) => println!("[dev-boot] 注册失败（风控冷却期的正常路径）: {e}"),
        }
    }
}
