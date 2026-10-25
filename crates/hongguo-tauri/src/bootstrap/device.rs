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
use hongguo_signer::device::{DeviceProfile, align_app_version, video_device};

/// 注册尝试的退避间隔。冷却期一次注册就是一次风控暴露面，24h 内绝不重试；
/// 手动重试（设置页按钮 / 命令）不受此限。
const REGISTER_BACKOFF_MS: i64 = 24 * 60 * 60 * 1000;

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

/// 后台健康复检：现役档案与静态兜底不同源才值得探（探针每次 1-2 个请求，
/// 静态现役时轮换目标就是自己，没有信息量）。只在「现役死、静态活」时轮换。
async fn ensure_healthy(state: &AppState) {
    let device = state.device();
    let current = video_device();
    if device.get("iid") == current.get("iid") {
        return;
    }
    let env = state.api_env();
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    if !matches!(probe(&env).await, Health::Dead) {
        return;
    }
    log::warn!(
        "[Device] 现役档案探针空响应（iid={}），静态档案对照中…",
        device.get("iid")
    );
    let static_env = ApiEnv {
        proxy: env.proxy.clone(),
        device: current.clone(),
        cookie: Some(hongguo_signer::device::anonymous_cookie(&current)),
        x_tt_token: None,
    };
    match probe(&static_env).await {
        Health::Ok => {
            log::warn!("[Device] 静态对照可用 → 现役档案已死，进入轮换");
            rotate(state, false, "探针判定死设备").await;
        }
        _ => {
            log::warn!("[Device] 静态对照也不可用（网络/服务端整体异常），按兵不动");
        }
    }
}

/// 轮换：注册优先（带退避），失败退静态兜底。`force` 绕过退避（手动重试）。
/// 返回轮换后的状态。
pub async fn rotate(state: &AppState, force: bool, reason: &str) -> DeviceStatus {
    let mut meta = read_meta(&state.store);
    let now = now_ms();
    let backoff_left = meta
        .last_register_attempt_ms
        .map(|t| REGISTER_BACKOFF_MS - (now - t).max(0))
        .unwrap_or(0);
    if !force && backoff_left > 0 {
        log::info!(
            "[Device] 注册退避中（剩 {}h），按 {} 处理：直接静态兜底",
            backoff_left / 3_600_000,
            reason
        );
        return repave_static(state, &mut meta).await;
    }

    meta.last_register_attempt_ms = Some(now);
    meta.last_register_error = None;
    write_meta(&state.store, &meta);

    // 签名档案用静态兜底（服务端要求注册请求带「已激活设备上下文」，
    // 见 register_device 注释）；body 指纹是全新的
    let reg_env = ApiEnv {
        proxy: state.settings().proxy,
        device: video_device(),
        cookie: None,
        x_tt_token: None,
    };
    match register_device(&reg_env).await {
        Ok(r) => {
            log::info!(
                "[Device] 注册成功 device_id={} install_id={} ttreq={}（{}）",
                r.device_id,
                r.install_id,
                if r.ttreq.is_empty() { "无" } else { "有" },
                reason
            );
            let mut profile = video_device();
            profile.set("device_id", &r.device_id);
            profile.set("iid", &r.install_id);
            profile.set("cdid", &r.cdid);
            profile.set("openudid", &r.openudid);
            align_app_version(&mut profile);
            profile.set_ttreq(Some(r.ttreq.clone()));
            swap(state, &mut meta, profile, "registered").await
        }
        Err(e) => {
            log::warn!("[Device] 注册失败（{}）：{e}", reason);
            meta.last_register_error = Some(e.to_string());
            write_meta(&state.store, &meta);
            repave_static(state, &mut meta).await
        }
    }
}

/// 静态兜底重铺。现役已是这份静态档案（iid 相同）时不折腾。
async fn repave_static(state: &AppState, meta: &mut BootstrapMeta) -> DeviceStatus {
    let current = video_device();
    if state.device().get("iid") == current.get("iid") {
        return status_inner(state, meta);
    }
    swap(state, meta, current, "static_fallback").await
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

/// 手动重试注册（绕过退避）。成功换注册档案；失败维持现役并记录错误。
pub async fn retry_register(state: &AppState) -> DeviceStatus {
    let mut meta = read_meta(&state.store);
    let reg_env = ApiEnv {
        proxy: state.settings().proxy,
        device: video_device(),
        cookie: None,
        x_tt_token: None,
    };
    meta.last_register_attempt_ms = Some(now_ms());
    write_meta(&state.store, &meta);
    match register_device(&reg_env).await {
        Ok(r) => {
            let mut profile = video_device();
            profile.set("device_id", &r.device_id);
            profile.set("iid", &r.install_id);
            profile.set("cdid", &r.cdid);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 退避判定：刚试过注册（24h 内）的元数据让非强制轮换直接走静态兜底，
    /// 不打注册接口（当前网络环境下打不通，退避的意义就是别反复撞）。
    #[test]
    fn rotate_respects_backoff_without_network() {
        // 同步部分可测：退避窗口的算术。真实轮换走全链路探针用例。
        let meta = BootstrapMeta {
            last_register_attempt_ms: Some(now_ms() - 3_600_000), // 1h 前
            ..BootstrapMeta::default()
        };
        let left = meta
            .last_register_attempt_ms
            .map(|t| REGISTER_BACKOFF_MS - (now_ms() - t).max(0))
            .unwrap_or(0);
        assert!(left > 0, "1h 前试过，24h 退避应仍在窗口内");
        assert!(left <= REGISTER_BACKOFF_MS);
    }
}

#[cfg(test)]
mod probe {
    use super::*;
    use hongguo_common::model::ProxyConfig;

    /// 全链路试注（预算内 1 次注册）：注册 → 组档案 → 探针验活性。
    /// 注册被风控拒时打印拒绝原文（退避逻辑的真实路径）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_register_and_use() {
        let reg_env = ApiEnv {
            proxy: ProxyConfig::default(),
            device: video_device(),
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
                let mut profile = video_device();
                profile.set("device_id", &r.device_id);
                profile.set("iid", &r.install_id);
                profile.set("cdid", &r.cdid);
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
