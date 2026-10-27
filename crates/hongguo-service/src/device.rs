//! 设备档案的启动同步（adopt）：读库 → 阵亡名单/远古快照预筛 → 版本对齐。
//!
//! 从壳的 bootstrap 迁入（双壳共用：Tauri 与 GPUI 的启动装配走同一段设备
//! 引导，「操作一致」的第一层）。纯 store + signer，不走网络——健康探针与
//! 轮换是异步生命周期，留在各壳的 bootstrap（Tauri 侧挂 async_runtime；
//! GPUI 侧待其生命周期装配时接入同一套）。
//!
//! 注册链路走 TT-Encrypt V5 格式（见 hongguo-api::register 模块文档）：
//! 匿名档案仍是无身份时的启动兜底，
//! [`ensure_registered`] 在壳启动后台把匿名/死档案自动换档为注册产物
//! （74232 活形态）——播放取流需要服务端「认识」的设备，匿名不可播。

use hongguo_api::client::ApiEnv;
use hongguo_api::register::{RegisterResult, register_device};
use hongguo_common::utils::time::now_ms;
use hongguo_signer::device::{DeviceProfile, align_app_version, anonymous_profile, fresh_cdid};
use hongguo_store::Store;

use crate::app_state::AppState;
use crate::events::ServiceEvent;

/// 设备注册换档的前端事件名（`ServiceEvent::DeviceRegistered` 对外名字，
/// 壳转发与前端 `EVENTS.deviceRegistered` 共用；载荷 = 新 iid）。
pub const DEVICE_REGISTERED: &str = "device-registered";

/// 已被淘汰、必须从库里重铺的档案 install_id（含**全部静态
/// 兜底档案**——共享身份是风控暴露面，一并退役；匿名档案取代之）。
/// **今后每次淘汰档案，旧 iid 必须追加进名单**——健康探针只兜「未来的死」，
/// 名单兜「已知的死」，缺补会让老用户再次集体阵亡。
pub const RETIRED_STATIC_IIDS: &[&str] = &["1905892595382586", "2715158266282762"];

/// 轮换元数据（`device_profile` 元数据行，JSON 文档）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct BootstrapMeta {
    /// 现役档案来源：registered（注册产物）/ anonymous（匿名游客档案）/
    /// adopted（库里继承，尚未复检）。
    #[serde(default)]
    pub source: String,
    /// 最近一次换档前的旧 iid。
    #[serde(default)]
    pub previous_iid: Option<String>,
    /// 上次注册尝试时间（ms epoch；退避判定基准）。
    #[serde(default)]
    pub last_register_attempt_ms: Option<i64>,
    /// 上次注册失败原因（成功清空）。
    #[serde(default)]
    pub last_register_error: Option<String>,
}

pub fn read_meta(store: &Store) -> BootstrapMeta {
    store
        .device_meta_get()
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

pub fn write_meta(store: &Store, meta: &BootstrapMeta) {
    if let Ok(v) = serde_json::to_value(meta)
        && let Err(e) = store.device_meta_set(&v)
    {
        log::warn!("[Device] 元数据落库失败（不影响换档）: {e}");
    }
}

/// 启动同步装配：读库 → 阵亡名单/远古快照预筛 → 注册产物复检/版本对齐。
/// 返回 (现役档案, 来源)。不走网络——探针与轮换在壳的 bootstrap 后台任务里。
pub fn adopt_sync(store: &Store) -> (DeviceProfile, &'static str) {
    match store.device_profile() {
        Ok(Some(p)) => {
            let dead_listed = RETIRED_STATIC_IIDS.contains(&p.get("iid"));
            let ancient = p.get("cdid").is_empty();
            if dead_listed || ancient {
                return repave_anonymous(store, &p, dead_listed);
            }
            // 注册产物钉注册口径，不参与版本对齐——自报版本与注册指纹必须
            // 同源，否则取流被拒；来源以轮换元数据为准（注册换档写入）
            if read_meta(store).source == "registered" {
                return (heal_registered_profile(store, p), "adopted");
            }
            let mut p = p;
            align_app_version(&mut p);
            (p, "adopted")
        }
        Ok(None) => (seed_anonymous(store), "anonymous"),
        Err(e) => {
            log::warn!("[Device] 设备档案读取失败，用匿名档案: {e}");
            (seed_anonymous(store), "anonymous")
        }
    }
}

/// 注册产物的启动复检：版本键是注册口径就原样放行；被旧版 adopt 的
/// `align_app_version` 升到当前客户端版本的（与注册指纹不同源，取流即挂），
/// 从档案自身身份重建为纯注册口径并落库。
fn heal_registered_profile(store: &Store, p: DeviceProfile) -> DeviceProfile {
    if p.get("version_code") == hongguo_signer::REGISTER_VERSION_CODE_STR {
        return p;
    }
    let healed = registered_profile(&RegisterResult {
        device_id: p.get("device_id").into(),
        install_id: p.get("iid").into(),
        cdid: p.get("cdid").into(),
        openudid: p.get("openudid").into(),
        ttreq: p.ttreq().unwrap_or_default().into(),
        ..Default::default()
    });
    log::warn!(
        "[Device] 注册档案版本键曾被升到 {}（与注册指纹不同源），重建为注册口径 {}",
        p.get("version_code"),
        healed.get("version_code")
    );
    if let Err(e) = store.save_device_profile(&healed) {
        log::warn!("[Device] 重建档案落库失败（本次会话仍生效）: {e}");
    }
    healed
}

/// 生成并落库匿名档案（cdid 每次安装生成一份，此后随库稳定）。
fn seed_anonymous(store: &Store) -> DeviceProfile {
    let profile = anonymous_profile(&fresh_cdid());
    if let Err(e) = store.save_device_profile(&profile) {
        log::warn!("[Device] 匿名档案落库失败（不影响启动）: {e}");
    }
    profile
}

/// 预筛命中的重铺：死档案进备份行，匿名档案顶上，元数据记痕迹。
fn repave_anonymous(
    store: &Store,
    dead: &DeviceProfile,
    dead_listed: bool,
) -> (DeviceProfile, &'static str) {
    log::warn!(
        "[Device] 库内档案已淘汰（iid={}，{}），重铺匿名档案",
        dead.get("iid"),
        if dead_listed {
            "阵亡名单"
        } else {
            "缺 cdid 的远古快照"
        }
    );
    let _ = store.backup_device_profile();
    let profile = seed_anonymous(store);
    let mut meta = read_meta(store);
    meta.source = "anonymous".into();
    meta.previous_iid = Some(dead.get("iid").to_string());
    write_meta(store, &meta);
    (profile, "anonymous")
}

/// 注册产物 → 现役档案：与注册请求共用
/// [`DeviceProfile::fresh_register_profile`] 构造（指纹字段集、顺序与 UA
/// 天然同源，编译期保证），只补注册响应下发的设备身份。
///
/// 注册口径（74232 活形态）钉在构造器里，**不做** `align_app_version`
/// （自报版本与注册指纹必须同源，否则取流被拒）。
pub fn registered_profile(reg: &RegisterResult) -> DeviceProfile {
    let mut p = DeviceProfile::fresh_register_profile(&reg.cdid, &reg.openudid);
    p.set("device_id", &reg.device_id);
    p.set("iid", &reg.install_id);
    p.set_ttreq(Some(reg.ttreq.clone()));
    p
}

/// 注册退避窗口：失败后 24h 内不重试（服务端按量风控，反复试加重冷却）。
const REGISTER_BACKOFF_MS: i64 = 24 * 3600 * 1000;

/// 注册换档结论（壳的设备状态命令用）。
#[derive(Debug, Clone)]
pub struct Registration {
    pub ok: bool,
    pub iid: String,
    pub device_id: String,
    /// skipped=退避期内未尝试；error=注册失败（错误在 meta 里）。
    pub kind: &'static str,
    pub error: Option<String>,
}

/// 注册新设备并换档：备份旧档 → 新档落库+热替换 → 元数据记痕迹。
/// 失败进 [`REGISTER_BACKOFF_MS`] 退避（`force` 绕过，手动重试用）。
pub async fn register_and_swap(state: &AppState, force: bool, reason: &str) -> Registration {
    let mut meta = read_meta(&state.store);
    if !force
        && let Some(at) = meta.last_register_attempt_ms
        && now_ms() - at < REGISTER_BACKOFF_MS
        && meta.last_register_error.is_some()
    {
        log::info!("[Device] 注册退避期内（{reason}），跳过");
        return Registration {
            ok: false,
            iid: state.device().get("iid").to_string(),
            device_id: String::new(),
            kind: "skipped",
            error: meta.last_register_error.clone(),
        };
    }
    meta.last_register_attempt_ms = Some(now_ms());
    write_meta(&state.store, &meta);

    let reg_env = ApiEnv {
        proxy: state.settings().proxy,
        device: anonymous_profile(&fresh_cdid()),
        cookie: None,
        x_tt_token: None,
    };
    match register_device(&reg_env).await {
        Ok(r) => {
            log::info!(
                "[Device] 注册成功（{reason}）device_id={} iid={}",
                r.device_id,
                r.install_id
            );
            let profile = registered_profile(&r);
            swap_profile(state, &mut meta, profile, "registered");
            // 换档前发出去的匿名请求产物（顶栏塌缩表等）全部作废
            hongguo_api::recommend::clear_feed_tabs_cache();
            state
                .events()
                .publish(ServiceEvent::DeviceRegistered(r.install_id.clone()));
            meta.last_register_error = None;
            write_meta(&state.store, &meta);
            Registration {
                ok: true,
                iid: r.install_id,
                device_id: r.device_id,
                kind: "registered",
                error: None,
            }
        }
        Err(e) => {
            log::warn!("[Device] 注册失败（{reason}）: {e}");
            meta.last_register_error = Some(e.to_string());
            write_meta(&state.store, &meta);
            Registration {
                ok: false,
                iid: state.device().get("iid").to_string(),
                device_id: String::new(),
                kind: "error",
                error: Some(e.to_string()),
            }
        }
    }
}

/// 换档三连：旧档进备份行 → 新档落库+热替换 → 元数据更新来源。
fn swap_profile(state: &AppState, meta: &mut BootstrapMeta, profile: DeviceProfile, source: &str) {
    let old_iid = state.device().get("iid").to_string();
    let new_iid = profile.get("iid").to_string();
    if old_iid != new_iid {
        if let Err(e) = state.store.backup_device_profile() {
            log::warn!("[Device] 旧档备份失败（继续换档）: {e}");
        }
        if let Err(e) = state.store.save_device_profile(&profile) {
            log::warn!("[Device] 新档落库失败（本次会话仍生效）: {e}");
        }
        log::info!("[Device] 换档 {old_iid} → {new_iid}（{source}）");
    }
    meta.source = source.to_string();
    meta.previous_iid = Some(old_iid).filter(|i| i != &new_iid);
    write_meta(&state.store, meta);
    state.replace_device(profile);
}

/// 启动后台生命周期：无设备身份（匿名/全新安装）**立即**注册（实测
/// ~200ms，首批业务请求即可用上注册身份）；有身份的打一次搜索探针，
/// 空响应（死设备）也转注册。注册失败维持现状（匿名可读不可播；退避
/// 期内不重试）。
pub async fn ensure_registered(state: &AppState) {
    if state.device().get("iid").is_empty() {
        log::info!("[Device] 现役无设备身份（匿名），启动注册");
        register_and_swap(state, false, "启动自动注册").await;
        return;
    }
    let env = state.api_env();
    match probe_alive(&env).await {
        Some(true) => {}
        Some(false) => {
            log::warn!(
                "[Device] 现役档案探针空响应（iid={}），转注册轮换",
                state.device().get("iid")
            );
            register_and_swap(state, false, "健康探针判死").await;
        }
        None => log::info!("[Device] 探针不确定（网络/服务端），按兵不动"),
    }
}

/// 健康探针（找剧搜索是风控最严的一支）：Some(true)=活，Some(false)=
/// 空响应（死设备信号），None=网络/服务端问题分不清归属。
async fn probe_alive(env: &ApiEnv) -> Option<bool> {
    match hongguo_api::search::search_series("热门", 0, "", env).await {
        Ok(_) => Some(true),
        Err(hongguo_common::AppError::EmptyResponse(_)) => Some(false),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_store::Store;

    fn store() -> Store {
        Store::open_memory().expect("内存库")
    }

    /// 测试注册产物（身份固定，档案字段断言可对照）。
    fn reg_result() -> RegisterResult {
        RegisterResult {
            device_id: "8888888888888888".into(),
            install_id: "9999999999999999".into(),
            ttreq: "ttreq-ticket".into(),
            cdid: "cdid-uuid".into(),
            openudid: "openudid-hex".into(),
            ..Default::default()
        }
    }

    /// 阵亡名单上的库档案：启动同步就重铺静态兜底，旧档进备份行，
    /// 元数据记下轮换痕迹。
    #[test]
    fn adopt_repaves_dead_listed_profile_with_backup() {
        let db = store();
        let mut stale = anonymous_profile("babf8a0e-8586-40ca-bb75-20a1a698b43f");
        stale.set("iid", "1905892595382586");
        db.save_device_profile(&stale).expect("写死档案");

        let (profile, source) = adopt_sync(&db);
        assert_eq!(source, "anonymous");
        // 重铺为匿名档案：无设备身份
        assert_eq!(profile.get("iid"), "");

        let backup = db
            .device_profile_backup()
            .expect("读备份")
            .expect("旧档已留底");
        assert_eq!(backup.get("iid"), "1905892595382586", "备份是旧档");

        let meta = read_meta(&db);
        assert_eq!(meta.source, "anonymous");
        assert_eq!(meta.previous_iid.as_deref(), Some("1905892595382586"));
    }

    /// 旧静态兜底档案（共享身份，已整体退役）：adopt 也重铺匿名。
    #[test]
    fn adopt_repaves_legacy_baked_static_profile() {
        let db = store();
        let mut baked = anonymous_profile("babf8a0e-8586-40ca-bb75-20a1a698b43f");
        baked.set("iid", "2715158266282762");
        baked.set("device_id", "2715158266032906");
        db.save_device_profile(&baked).expect("写旧静态档案");

        let (profile, source) = adopt_sync(&db);
        assert_eq!(source, "anonymous");
        assert_eq!(profile.get("iid"), "");
        assert_eq!(
            read_meta(&db).previous_iid.as_deref(),
            Some("2715158266282762")
        );
    }

    /// 有身份但轮换元数据缺 registered 标记的档案（旧版本升级带上来的
    /// 遗产权档案）：adopt 按匿名口径做版本对齐。
    #[test]
    fn adopt_aligns_profile_without_registered_meta() {
        let db = store();
        let mut reg = anonymous_profile("0f0f0f0f-0000-4000-8000-000000000000");
        reg.set("iid", "9999999999999999");
        reg.set("device_id", "8888888888888888");
        db.save_device_profile(&reg).expect("写档案");

        let (profile, source) = adopt_sync(&db);
        assert_eq!(source, "adopted");
        assert_eq!(profile.get("iid"), "9999999999999999");
        assert_eq!(
            profile.get("version_code"),
            hongguo_signer::APP_VERSION_CODE_STR,
            "版本身份对齐到当前客户端"
        );
    }

    /// 注册产物档案启动复检：钉注册口径不做版本对齐（升 74232 与注册
    /// 指纹不同源，取流实测即挂）。
    #[test]
    fn adopt_keeps_registered_profile_on_register_regime() {
        let db = store();
        db.save_device_profile(&registered_profile(&reg_result()))
            .expect("写注册档案");
        let mut meta = read_meta(&db);
        meta.source = "registered".into();
        write_meta(&db, &meta);

        let (profile, source) = adopt_sync(&db);
        assert_eq!(source, "adopted");
        assert_eq!(
            profile.get("version_code"),
            hongguo_signer::REGISTER_VERSION_CODE_STR,
            "注册口径不被 align 升走"
        );
        assert_eq!(profile.user_agent(), hongguo_signer::device::register_ua());
        assert_eq!(profile.ttreq(), Some("ttreq-ticket"));
    }

    /// 历史上被 align 升坏的注册档案（版本键 74232）：启动复检从档案
    /// 自身身份重建为纯注册口径并落库。
    #[test]
    fn adopt_heals_version_bumped_registered_profile() {
        let db = store();
        let mut damaged = registered_profile(&reg_result());
        hongguo_signer::device::align_app_version(&mut damaged);
        db.save_device_profile(&damaged).expect("写受损档案");
        let mut meta = read_meta(&db);
        meta.source = "registered".into();
        write_meta(&db, &meta);

        let (profile, _) = adopt_sync(&db);
        assert_eq!(
            profile.get("version_code"),
            hongguo_signer::REGISTER_VERSION_CODE_STR,
            "受损注册档案重建回注册口径"
        );
        assert_eq!(profile.get("device_id"), "8888888888888888");
        assert_eq!(profile.get("iid"), "9999999999999999");
        assert_eq!(profile.ttreq(), Some("ttreq-ticket"));
        let stored = db.device_profile().expect("读回").expect("已落库");
        assert_eq!(
            stored.get("version_code"),
            hongguo_signer::REGISTER_VERSION_CODE_STR
        );
    }

    /// 库里没档案：生成并落库匿名档案（cdid 随库稳定）。
    #[test]
    fn adopt_seeds_anonymous_when_db_empty() {
        let db = store();
        let (profile, source) = adopt_sync(&db);
        assert_eq!(source, "anonymous");
        assert!(!profile.get("cdid").is_empty(), "匿名档案必须带 cdid");
        assert_eq!(profile.get("iid"), "");
        let stored = db.device_profile().expect("读回").expect("已落库");
        assert_eq!(
            stored.get("cdid"),
            profile.get("cdid"),
            "落库的 cdid 与现役一致"
        );
    }

    /// 注册产物档案与注册请求共用同一构造：版本身份钉注册口径、
    /// UA 为注册形态，身份键来自注册响应且按「指纹在前、身份在后」的
    /// 顺序落位（签名 query 顺序敏感）。
    #[test]
    fn registered_profile_shares_register_fingerprint() {
        let reg = RegisterResult {
            device_id: "8888888888888888".into(),
            install_id: "9999999999999999".into(),
            ttreq: "ttreq-ticket".into(),
            cdid: "cdid-uuid".into(),
            openudid: "openudid-hex".into(),
            ..Default::default()
        };
        let p = registered_profile(&reg);
        for k in [
            "version_code",
            "update_version_code",
            "manifest_version_code",
            "pv_player",
        ] {
            assert_eq!(p.get(k), hongguo_signer::REGISTER_VERSION_CODE_STR);
        }
        assert_eq!(p.get("version_name"), hongguo_signer::REGISTER_VERSION_NAME);
        assert_eq!(p.user_agent(), hongguo_signer::device::register_ua());
        assert_eq!(p.get("device_id"), "8888888888888888");
        assert_eq!(p.get("iid"), "9999999999999999");
        let all: Vec<&str> = p.iter().map(|(k, _)| k).collect();
        assert_eq!(
            &all[all.len() - 4..],
            &["cdid", "openudid", "device_id", "iid"]
        );
    }

    /// 元数据行的读写往返（含空→有→覆盖）。
    #[test]
    fn device_meta_roundtrip() {
        let db = store();
        assert!(db.device_meta_get().expect("空").is_none());
        let v = serde_json::json!({"source": "registered", "previous_iid": "1"});
        db.device_meta_set(&v).expect("写");
        assert_eq!(db.device_meta_get().expect("读"), Some(v));
        db.device_meta_set(&serde_json::json!({"source": "anonymous"}))
            .expect("覆盖");
        assert_eq!(
            db.device_meta_get().expect("读"),
            Some(serde_json::json!({"source": "anonymous"}))
        );
    }
}
