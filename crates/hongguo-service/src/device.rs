//! 设备档案的启动同步（adopt）：读库 → 阵亡名单/远古快照预筛 → 版本对齐。
//!
//! 从壳的 bootstrap 迁入（双壳共用：Tauri 与 GPUI 的启动装配走同一段设备
//! 引导，「操作一致」的第一层）。纯 store + signer，不走网络——健康探针与
//! 轮换是异步生命周期，留在各壳的 bootstrap（Tauri 侧挂 async_runtime；
//! GPUI 侧待其生命周期装配时接入同一套）。
//!
//! 2026-10-27 起注册链路恢复（TT-Encrypt V5 格式修正后真机注册实测通过，
//! 见 hongguo-api::register 模块文档）：匿名档案仍是无身份时的启动兜底，
//! [`ensure_registered`] 在壳启动后台把匿名/死档案自动换档为注册产物
//! （73932 活形态）——播放取流需要服务端「认识」的设备，匿名不可播。

use hongguo_api::client::ApiEnv;
use hongguo_api::register::{RegisterResult, register_device};
use hongguo_common::utils::time::now_ms;
use hongguo_signer::device::{DeviceProfile, align_app_version, anonymous_profile, fresh_cdid};
use hongguo_store::Store;

use crate::app_state::AppState;
use crate::events::ServiceEvent;

/// 已被淘汰、必须从库里重铺的档案 install_id（2026-10-26 起含**全部静态
/// 兜底档案**——共享身份是风控暴露面，一并退役；匿名档案取代之）。
/// **今后每次淘汰档案，旧 iid 必须追加进名单**——健康探针只兜「未来的死」，
/// 名单兜「已知的死」，缺补会让老用户再次集体阵亡（2026-10-09 搜索全挂
/// 事故的教训）。
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

/// 启动同步装配：读库 → 阵亡名单/远古快照预筛 → 版本对齐。
/// 返回 (现役档案, 来源)。不走网络——探针与轮换在壳的 bootstrap 后台任务里。
pub fn adopt_sync(store: &Store) -> (DeviceProfile, &'static str) {
    match store.device_profile() {
        Ok(Some(p)) => {
            let dead_listed = RETIRED_STATIC_IIDS.contains(&p.get("iid"));
            let ancient = p.get("cdid").is_empty();
            if dead_listed || ancient {
                return repave_anonymous(store, &p, dead_listed);
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

/// 注册产物 → 现役档案（2026-10-27 取流矩阵实证的 73932 活形态）。
///
/// 字段集与顺序必须与注册请求同源（指纹一致性 + 签名 query 顺序敏感）：
/// 版本身份钉 7.3.9.32、UA 尾段为 73932 时代 Cronet 形态，**不做**
/// `align_app_version`（升 74232 与注册指纹不同源，取流实测即挂）。
pub fn registered_profile(reg: &RegisterResult) -> DeviceProfile {
    let mut p = DeviceProfile::from_pairs(
        &[
            ("ac", "wifi"),
            ("aid", "8662"),
            ("app_name", "novelread"),
            ("channel", "xiaomi_8662_64"),
            ("compliance_status", "0"),
            ("device_brand", "xiaomi"),
            ("device_platform", "android"),
            ("device_type", "23127PN0CC"),
            ("dpi", "460"),
            ("dragon_device_type", "phone"),
            ("host_abi", "arm64-v8a"),
            ("is_android_pad_screen", "0"),
            ("language", "zh"),
            ("manifest_version_code", "73932"),
            ("os", "android"),
            ("os_api", "34"),
            ("os_version", "14"),
            ("player_so_load", "1"),
            ("pv_player", "73932"),
            ("resolution", "1200*2670"),
            ("ssmix", "a"),
            ("update_version_code", "73932"),
            ("version_code", "73932"),
            ("version_name", "7.3.9.32"),
        ],
        "com.phoenix.read/73932 (Linux; U; Android 14; zh_CN; Xiaomi 14; \
         Build/UKQ1.230804.001; Cronet/TTNetVersion:8d40f833 QuicVersion:462f352c 2026-08-31)",
    );
    p.set("cdid", &reg.cdid);
    p.set("openudid", &reg.openudid);
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

    /// 活着的注册产物（有 cdid、iid 不在名单）：adopt 原样放行，只做版本对齐。
    #[test]
    fn adopt_keeps_healthy_registered_profile() {
        let db = store();
        let mut reg = anonymous_profile("0f0f0f0f-0000-4000-8000-000000000000");
        reg.set("iid", "9999999999999999");
        reg.set("device_id", "8888888888888888");
        db.save_device_profile(&reg).expect("写注册档案");

        let (profile, source) = adopt_sync(&db);
        assert_eq!(source, "adopted");
        assert_eq!(profile.get("iid"), "9999999999999999");
        assert_eq!(
            profile.get("version_code"),
            hongguo_signer::APP_VERSION_CODE_STR,
            "版本身份对齐到当前客户端"
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
