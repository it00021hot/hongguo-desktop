//! 设备档案的启动同步（adopt）：读库 → 阵亡名单/远古快照预筛 → 版本对齐；双壳共用。
//! 纯 store + signer，不走网络——健康探针与轮换是异步生命周期，留在各壳的 bootstrap。
//! 注册链路走 TT-Encrypt V5（见 hongguo-api::register）：[`ensure_registered`] 把匿名/死档案
//! 自动换档为注册产物——播放取流需要服务端「认识」的设备，匿名不可播。

use std::sync::atomic::{AtomicU32, Ordering};

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

/// 冷启动预热完成前挡住起播。壳在拉起 [`ensure_registered`] 之前调用。
pub use crate::cold_start::arm as arm_cold_start;

/// 已被淘汰、必须从库里重铺的档案 install_id（含全部静态兜底档案——共享身份是风控暴露面）。
/// 今后每次淘汰档案，旧 iid 必须追加进名单：名单兜「已知的死」，缺补会让老用户再次集体阵亡。
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
    /// 上次注册尝试时间（ms epoch）。空响应换档用它做冷却。
    #[serde(default)]
    pub last_register_attempt_ms: Option<i64>,
    /// 现役档案是否已经拿到过非空码流。没有发货记录的档案在冷却内仍允许再换一次。
    #[serde(default)]
    pub play_shipped: bool,
    /// 上次注册失败原因（成功清空；诊断记录，不参与任何判定）。
    #[serde(default)]
    pub last_register_error: Option<String>,
    /// 现役档案的注册代际（[`hongguo_api::register::REGISTER_REGIME`]）。
    /// 缺失或不符 = 档案出生于旧注册器（被服务端标记、取流不发货），
    /// 启动时按全新安装语义一次性迁移到当前代际。
    #[serde(default)]
    pub register_regime: String,
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
            // 注册产物钉注册口径，不参与版本对齐——自报版本与注册指纹必须同源，否则取流被拒。
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

/// 注册产物的启动复检：版本与渠道都符合当前注册口径才原样放行；否则（版本被旧版
/// `align_app_version` 升走、或渠道仍是存量旧值）从档案自身身份重建为当前注册口径并落库。
fn heal_registered_profile(store: &Store, p: DeviceProfile) -> DeviceProfile {
    if p.get("version_code") == hongguo_signer::REGISTER_VERSION_CODE_STR
        && p.get("channel") == hongguo_signer::device::BUILD_CHANNEL
    {
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
        "[Device] 注册档案身份键与当前口径不符（version={} channel={}），重建为 version={} channel={}",
        p.get("version_code"),
        p.get("channel"),
        healed.get("version_code"),
        healed.get("channel")
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

/// 注册产物 → 现役档案：与注册请求共用 [`DeviceProfile::fresh_register_profile`] 构造，
/// 只补注册响应下发的设备身份。注册口径钉在构造器里，**不做** `align_app_version`
/// （自报版本与注册指纹必须同源，否则取流被拒）。
pub fn registered_profile(reg: &RegisterResult) -> DeviceProfile {
    let mut p = DeviceProfile::fresh_register_profile(&reg.cdid, &reg.openudid);
    p.set("device_id", &reg.device_id);
    p.set("iid", &reg.install_id);
    p.set_ttreq(Some(reg.ttreq.clone()));
    p
}

/// 注册重试节奏：失败只区分网络类（断网/超时，等网恢复就地重试）与服务端明确拒绝
/// （形态/代际错，重试无意义立即停）。
const REGISTER_RETRY_MS: u64 = 60_000;
const REGISTER_RETRY_MAX: usize = 30;

/// 注册（带网络恢复重试）：成功返回 true。网络类失败按节奏重试到成功
/// 或预算耗尽；其他失败立即返回。
async fn register_with_retry(state: &AppState, reason: &str) -> bool {
    for attempt in 0..REGISTER_RETRY_MAX {
        let reg = register_and_swap(state, reason).await;
        if reg.ok {
            return true;
        }
        if !reg.network {
            return false;
        }
        log::info!(
            "[Device] {reason}：网络不可用，{}s 后重试（{}/{}）",
            REGISTER_RETRY_MS / 1000,
            attempt + 1,
            REGISTER_RETRY_MAX
        );
        tokio::time::sleep(std::time::Duration::from_millis(REGISTER_RETRY_MS)).await;
    }
    log::warn!("[Device] {reason}：网络重试预算耗尽（{REGISTER_RETRY_MAX} 次），下次启动再试");
    false
}

/// 注册换档结论（壳的设备状态命令用）。
#[derive(Debug, Clone)]
pub struct Registration {
    pub ok: bool,
    pub iid: String,
    pub device_id: String,
    /// error=注册失败（错误在 meta 里）。
    pub kind: &'static str,
    pub error: Option<String>,
    /// 失败是否网络类（连接层断网/超时）：网络类等网恢复重试有意义，
    /// 服务端明确拒绝没有。
    pub network: bool,
}

/// 注册新设备并换档：备份旧档 → 新档落库+热替换 → 元数据记痕迹。
pub async fn register_and_swap(state: &AppState, reason: &str) -> Registration {
    let mut meta = read_meta(&state.store);
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
            // 换档前发出去的匿名请求产物（顶栏塌缩表、批量预取流）全部作废
            hongguo_api::recommend::clear_feed_tabs_cache();
            hongguo_api::play_url::clear_batch_cache();
            // 冷启动行为预热必须等完再放行：官方冷启动序列完成后才有发货资格；
            // 异步 fire-and-forget 会让起播抢在预热前打空响应。
            let env = state.api_env();
            let ok = hongguo_api::warmup::run(&env).await;
            log::info!("[Device] 冷启动行为预热完成（{ok} 条）");
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
                network: false,
            }
        }
        Err(e) => {
            log::warn!("[Device] 注册失败（{reason}）: {e}");
            let network = matches!(e, hongguo_common::AppError::Network(_));
            meta.last_register_error = Some(e.to_string());
            write_meta(&state.store, &meta);
            Registration {
                ok: false,
                iid: state.device().get("iid").to_string(),
                device_id: String::new(),
                kind: "error",
                error: Some(e.to_string()),
                network,
            }
        }
    }
}

/// 换档三连：旧档进备份行 → 新档落库+热替换 → 元数据更新来源。
/// 注册代际对齐解决不了「发货资格在注册时抽签」——输家签换新注册即可（1 次/分钟限速）。
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
    meta.register_regime = hongguo_api::register::REGISTER_REGIME.into();
    meta.play_shipped = false;
    write_meta(&state.store, meta);
    state.replace_device(profile);
}

/// 启动后台生命周期：无设备身份（匿名/全新安装）**立即**注册；有身份的先看注册代际，
/// 旧代际档案按全新安装语义**一次性**迁移到当前代际；代际正常的再打搜索探针，
/// 空响应（死设备）也转注册。网络类失败随恢复就地重试（`register_with_retry`）。
pub async fn ensure_registered(state: &AppState) {
    let _release_playback = crate::cold_start::ReleaseOnDrop;
    if state.device().get("iid").is_empty() {
        log::info!("[Device] 现役无设备身份（匿名），启动注册");
        register_with_retry(state, "启动自动注册").await;
        return;
    }
    let meta = read_meta(&state.store);
    if needs_regime_migration(&meta) {
        log::warn!(
            "[Device] 库内档案出生于旧注册代际（iid={}，regime={:?}），一次性迁移到 {} 代际",
            state.device().get("iid"),
            meta.register_regime,
            hongguo_api::register::REGISTER_REGIME
        );
        // 迁移成功即新档出生当前代际（探针不必再做）；拒绝/预算耗尽同样轮不到探针说话
        register_with_retry(state, "注册代际迁移").await;
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
            // 成功时 register_and_swap 已同步跑完预热；失败则旧档已死，预热无意义
            register_with_retry(state, "健康探针判死").await;
            return;
        }
        None => log::info!("[Device] 探针不确定（网络/服务端），按兵不动"),
    }
    // 冷启动行为预热：每个进程一次，服务端按行为状态发放深集授权。
    // 必须等完再放行起播，不能后台 fire-and-forget。
    let env = state.api_env();
    let ok = hongguo_api::warmup::run(&env).await;
    log::info!("[Device] 冷启动行为预热完成（{ok} 条）");
}

/// 是否需要注册代际迁移：代际标记与当前注册器不符（含缺失——任何
/// 在标记机制引入前注册的档案都算旧代际）。
fn needs_regime_migration(meta: &BootstrapMeta) -> bool {
    meta.register_regime != hongguo_api::register::REGISTER_REGIME
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

static STREAM_MISS_COUNT: AtomicU32 = AtomicU32::new(0);

/// 取流成功：清零 miss 计数，并记下这台设备已经发过货。
pub fn note_stream_ok(state: &AppState) {
    STREAM_MISS_COUNT.store(0, Ordering::Relaxed);
    let mut meta = read_meta(&state.store);
    if meta.play_shipped {
        return;
    }
    meta.play_shipped = true;
    write_meta(&state.store, &meta);
}

/// 取流未命中记账（有响应体但挑不出可播流）。不换设备——与空响应
/// （[`recover_after_empty_stream`]）分开。
pub fn note_stream_miss(_state: &AppState, vid: &str) {
    let fails = STREAM_MISS_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    log::warn!("[Device] 取流未命中 vid={vid} 连续={fails}（有响应无可播档）");
}

/// 空响应换档冷却：同 IP 高频注册会风控。已经发过货的档案按这个间隔最多换一次。
const EMPTY_RECOVER_COOLDOWN_MS: i64 = 10 * 60 * 1000;
/// 还没发过货的档案在冷却里最多再换这么多次（注册抽签的输家）。
const UNSHIPPED_ROLLS_IN_COOLDOWN: u32 = 2;

static UNSHIPPED_ROLLS: AtomicU32 = AtomicU32::new(0);

/// 冷却内是否还允许换档。已发货的档案遵守冷却；没发过货的档案还有有限次数。
fn allow_empty_reregister(in_cooldown: bool, play_shipped: bool, unshipped_rolls: u32) -> bool {
    if !in_cooldown {
        return true;
    }
    !play_shipped && unshipped_rolls < UNSHIPPED_ROLLS_IN_COOLDOWN
}

/// 取流全程空响应时恢复：没发过货的档案即使还在冷却内也再换一次；
/// 已经发过货的档案冷却内只补预热。换档本身会先跑完冷启动预热。
pub async fn recover_after_empty_stream(state: &AppState, vid: &str) -> bool {
    let now = now_ms();
    let meta = read_meta(&state.store);
    let in_cooldown = meta
        .last_register_attempt_ms
        .is_some_and(|prev| now.saturating_sub(prev) < EMPTY_RECOVER_COOLDOWN_MS);
    let rolls = UNSHIPPED_ROLLS.load(Ordering::Relaxed);
    if allow_empty_reregister(in_cooldown, meta.play_shipped, rolls) {
        if in_cooldown {
            UNSHIPPED_ROLLS.fetch_add(1, Ordering::Relaxed);
            log::warn!("[Device] 取流空响应 vid={vid}，现役档案尚未发货，冷却内继续换档");
        } else {
            log::warn!("[Device] 取流空响应 vid={vid}，匿名换档恢复身份");
        }
        if !register_with_retry(state, "取流空响应恢复").await {
            return false;
        }
    } else {
        log::warn!(
            "[Device] 取流空响应 vid={vid}，距上次注册 {}s，冷却中只补预热",
            meta.last_register_attempt_ms
                .map(|prev| (now - prev) / 1000)
                .unwrap_or(0)
        );
    }
    let env = state.api_env();
    let n = hongguo_api::warmup::run(&env).await;
    log::info!("[Device] 空响应恢复预热完成（{n} 条）");
    true
}

/// 与在线播放同一份选流口径：HEVC + ByteVC2 探测结论一并传入，
/// 否则选流与真实可播能力翻转（见 `play_service::online::prepare`）。
pub(crate) fn stream_cap() -> hongguo_api::stream_pick::DecoderCapability {
    hongguo_api::stream_pick::DecoderCapability::from_probes(
        hongguo_media_platform::capability::playback::hevc_playable(),
        hongguo_media_platform::capability::bytevc2_decode_available(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_store::Store;

    fn store() -> Store {
        Store::open_memory().expect("内存库")
    }

    #[test]
    fn note_stream_ok_clears_miss_count() {
        STREAM_MISS_COUNT.store(3, Ordering::Relaxed);
        let state = std::sync::Arc::new(crate::app_state::AppStateInner::with_db(store()));
        note_stream_ok(&state);
        assert_eq!(STREAM_MISS_COUNT.load(Ordering::Relaxed), 0);
        assert!(read_meta(&state.store).play_shipped);
    }

    #[test]
    fn unshipped_device_may_reregister_inside_cooldown() {
        assert!(allow_empty_reregister(false, false, 0));
        assert!(allow_empty_reregister(true, false, 0));
        assert!(allow_empty_reregister(true, false, 1));
        assert!(!allow_empty_reregister(
            true,
            false,
            UNSHIPPED_ROLLS_IN_COOLDOWN
        ));
        assert!(!allow_empty_reregister(true, true, 0));
        assert!(allow_empty_reregister(false, true, 9));
    }

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

    #[test]
    fn adopt_repaves_dead_listed_profile_with_backup() {
        let db = store();
        let mut stale = anonymous_profile("babf8a0e-8586-40ca-bb75-20a1a698b43f");
        stale.set("iid", "1905892595382586");
        db.save_device_profile(&stale).expect("写死档案");

        let (profile, source) = adopt_sync(&db);
        assert_eq!(source, "anonymous");
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
        assert_eq!(
            profile.user_agent(),
            hongguo_signer::device::fingerprint_for_cdid("cdid-uuid").register_ua()
        );
        assert_eq!(profile.ttreq(), Some("ttreq-ticket"));
    }

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
        assert_eq!(
            p.user_agent(),
            hongguo_signer::device::fingerprint_for_cdid("cdid-uuid").register_ua()
        );
        assert_eq!(p.get("device_id"), "8888888888888888");
        assert_eq!(p.get("iid"), "9999999999999999");
        let all: Vec<&str> = p.iter().map(|(k, _)| k).collect();
        assert_eq!(
            &all[all.len() - 4..],
            &["cdid", "openudid", "device_id", "iid"]
        );
    }

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

    #[test]
    fn regime_migration_only_for_mismatched_marker() {
        let mut meta = BootstrapMeta::default();
        assert!(
            needs_regime_migration(&meta),
            "无标记 = 旧注册器产物，需要迁移"
        );
        meta.register_regime = "00000".into();
        assert!(needs_regime_migration(&meta), "旧代际标记需要迁移");
        // 仅 version_code、无 SDK 指纹的旧标记也要迁（74332 ≠ 74332:4.3.2-rc.44）
        meta.register_regime = hongguo_signer::REGISTER_VERSION_CODE_STR.into();
        assert!(
            needs_regime_migration(&meta),
            "缺 SDK 指纹的旧代际标记需要迁移"
        );
        meta.register_regime = hongguo_api::register::REGISTER_REGIME.into();
        assert!(
            !needs_regime_migration(&meta),
            "当前代际档案不迁（终生不换设备身份）"
        );
    }
}
