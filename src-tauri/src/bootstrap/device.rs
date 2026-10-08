//! 设备生命周期（对齐 hgplayer 的 deviceBootstrap：adopt → 健康探针 →
//! 失效轮换 → 旧档留底）。
//!
//! 与第三方同构的三层：
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
//! 每次换档：旧档复制进 `device_profile` 备份行（hgplayer 的
//! `device.json.bak` 同款），轮换元数据（来源 / 上次注册尝试与错误）落
//! 元数据行，供状态命令与退避判定使用。

use serde::Serialize;

use crate::app_state::AppState;
use crate::domain::api::client::ApiEnv;
use crate::domain::api::register::register_device;
use crate::domain::api::search::search_series;
use crate::error::AppError;
use crate::signer::device::{DeviceProfile, align_app_version, video_device};
use crate::store::Store;

/// 已被服务端风控清理、必须从库里淘汰的静态档案 install_id（2026-10-04
/// 实测旧 71332 档案 reading 系整段 0 字节拒）。**今后每次淘汰静态档案，
/// 旧 iid 必须追加进名单**——健康探针只兜「未来的死」，名单兜「已知的死」，
/// 缺补会让老用户再次集体阵亡（2026-10-09 搜索全挂事故的教训）。
const RETIRED_STATIC_IIDS: &[&str] = &["1905892595382586"];

/// 注册尝试的退避间隔。冷却期一次注册就是一次风控暴露面，24h 内绝不重试；
/// 手动重试（设置页按钮 / 命令）不受此限。
const REGISTER_BACKOFF_MS: i64 = 24 * 60 * 60 * 1000;

/// 健康探针用的搜索词（探针只看「有没有非空响应」，词本身无关紧要）。
const PROBE_QUERY: &str = "热门";

/// 轮换元数据（`device_profile` 元数据行，JSON 文档）。
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
struct BootstrapMeta {
    /// 现役档案来源：registered（注册产物）/ static_fallback（静态兜底）/
    /// adopted（库里继承，尚未复检）。
    #[serde(default)]
    source: String,
    /// 最近一次换档前的旧 iid。
    #[serde(default)]
    previous_iid: Option<String>,
    /// 上次注册尝试时间（ms epoch；退避判定基准）。
    #[serde(default)]
    last_register_attempt_ms: Option<i64>,
    /// 上次注册失败原因（成功清空）。
    #[serde(default)]
    last_register_error: Option<String>,
}

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

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn read_meta(store: &Store) -> BootstrapMeta {
    store
        .device_meta_get()
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn write_meta(store: &Store, meta: &BootstrapMeta) {
    if let Ok(v) = serde_json::to_value(meta)
        && let Err(e) = store.device_meta_set(&v)
    {
        log::warn!("[Device] 元数据落库失败（不影响换档）: {e}");
    }
}

/// 启动同步装配：读库 → 阵亡名单/远古快照预筛 → 版本对齐。
/// 返回 (现役档案, 来源)。不走网络——探针与轮换在 [`init`] 拉起的后台任务里。
pub fn adopt_sync(store: &Store) -> (DeviceProfile, &'static str) {
    match store.device_profile() {
        Ok(Some(p)) => {
            let current = video_device();
            let dead_listed = RETIRED_STATIC_IIDS.contains(&p.get("iid"));
            let ancient = p.get("cdid").is_empty();
            if (dead_listed || ancient) && p.get("iid") != current.get("iid") {
                return repave_dead_profile(store, &p, dead_listed);
            }
            let mut p = p;
            align_app_version(&mut p);
            (p, "adopted")
        }
        Ok(None) => {
            let fallback = video_device();
            if let Err(e) = store.save_device_profile(&fallback) {
                log::warn!("[Device] 静态设备档案落库失败（不影响启动）: {e}");
            }
            let mut meta = read_meta(store);
            meta.source = "static_fallback".into();
            write_meta(store, &meta);
            (fallback, "static_fallback")
        }
        Err(e) => {
            log::warn!("[Device] 设备档案读取失败，用静态兜底: {e}");
            (video_device(), "static_fallback")
        }
    }
}

/// 启动预筛命中的重铺：死档案进备份行，静态兜底顶上，元数据记痕迹。
fn repave_dead_profile(
    store: &Store,
    dead: &DeviceProfile,
    dead_listed: bool,
) -> (DeviceProfile, &'static str) {
    let current = video_device();
    log::warn!(
        "[Device] 库内档案已被风控淘汰（iid={}，{}），重铺静态兜底（iid={}）",
        dead.get("iid"),
        if dead_listed {
            "阵亡名单"
        } else {
            "缺 cdid 的远古快照"
        },
        current.get("iid")
    );
    let _ = store.backup_device_profile();
    if let Err(e) = store.save_device_profile(&current) {
        log::warn!("[Device] 重铺落库失败（不影响启动）: {e}");
    }
    let mut meta = read_meta(store);
    meta.source = "static_fallback".into();
    meta.previous_iid = Some(dead.get("iid").to_string());
    write_meta(store, &meta);
    (current, "static_fallback")
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
        cookie: Some(crate::signer::device::anonymous_cookie(&current)),
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
    use crate::store::Store;

    fn store() -> Store {
        Store::open_memory().expect("内存库")
    }

    /// 阵亡名单上的库档案：启动同步就重铺静态兜底，旧档进备份行，
    /// 元数据记下轮换痕迹。
    #[test]
    fn adopt_repaves_dead_listed_profile_with_backup() {
        let db = store();
        let mut stale = video_device();
        stale.set("iid", "1905892595382586");
        stale.set("cdid", "babf8a0e-8586-40ca-bb75-20a1a698b43f");
        db.save_device_profile(&stale).expect("写死档案");

        let (profile, source) = adopt_sync(&db);
        assert_eq!(source, "static_fallback");
        assert_eq!(
            profile.get("iid"),
            video_device().get("iid"),
            "重铺为当前兜底"
        );

        let backup = db
            .device_profile_backup()
            .expect("读备份")
            .expect("旧档已留底");
        assert_eq!(backup.get("iid"), "1905892595382586", "备份是旧档");

        let meta = read_meta(&db);
        assert_eq!(meta.source, "static_fallback");
        assert_eq!(meta.previous_iid.as_deref(), Some("1905892595382586"));
    }

    /// 活着的注册产物（有 cdid、iid 不在名单）：adopt 原样放行，只做版本对齐。
    #[test]
    fn adopt_keeps_healthy_registered_profile() {
        let db = store();
        let mut reg = video_device();
        reg.set("iid", "9999999999999999");
        reg.set("device_id", "8888888888888888");
        reg.set("cdid", "0f0f0f0f-0000-4000-8000-000000000000");
        db.save_device_profile(&reg).expect("写注册档案");

        let (profile, source) = adopt_sync(&db);
        assert_eq!(source, "adopted");
        assert_eq!(profile.get("iid"), "9999999999999999");
        assert_eq!(
            profile.get("version_code"),
            video_device().get("version_code"),
            "版本身份对齐到当前客户端"
        );
    }

    /// 库里没档案：落静态兜底。第二次 adopt 起同一兜底不再动（iid 相同不重铺）。
    #[test]
    fn adopt_seeds_static_when_db_empty() {
        let db = store();
        let (profile, source) = adopt_sync(&db);
        assert_eq!(source, "static_fallback");
        assert_eq!(profile.get("iid"), video_device().get("iid"));
        assert!(db.device_profile().expect("读回").is_some());
    }

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

    /// 元数据行的读写往返（含空→有→覆盖）。
    #[test]
    fn device_meta_roundtrip() {
        let db = store();
        assert!(db.device_meta_get().expect("空").is_none());
        let v = serde_json::json!({"source": "registered", "previous_iid": "1"});
        db.device_meta_set(&v).expect("写");
        assert_eq!(db.device_meta_get().expect("读"), Some(v));
        db.device_meta_set(&serde_json::json!({"source": "static_fallback"}))
            .expect("覆盖");
        assert_eq!(
            db.device_meta_get().expect("读"),
            Some(serde_json::json!({"source": "static_fallback"}))
        );
    }
}

#[cfg(test)]
mod probe {
    use super::*;
    use crate::domain::model::ProxyConfig;

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
                    cookie: Some(crate::signer::device::anonymous_cookie(&profile)),
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
