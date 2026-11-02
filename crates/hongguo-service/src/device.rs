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
//! （注册口径活形态）——播放取流需要服务端「认识」的设备，匿名不可播。

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

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
    /// 上次注册尝试时间（ms epoch；诊断记录，不参与任何判定）。
    #[serde(default)]
    pub last_register_attempt_ms: Option<i64>,
    /// 上次注册失败原因（成功清空；诊断记录，不参与任何判定）。
    #[serde(default)]
    pub last_register_error: Option<String>,
    /// 现役档案的注册代际（`REGISTER_VERSION_CODE_STR`）。缺失或不符 =
    /// 档案出生于旧注册器（bdinstall SDK 代际错配，设备被服务端标记、
    /// 取流不发货），启动时按全新安装语义一次性迁移到当前代际。
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
/// 注册口径活形态钉在构造器里，**不做** `align_app_version`
/// （自报版本与注册指纹必须同源，否则取流被拒）。
pub fn registered_profile(reg: &RegisterResult) -> DeviceProfile {
    let mut p = DeviceProfile::fresh_register_profile(&reg.cdid, &reg.openudid);
    p.set("device_id", &reg.device_id);
    p.set("iid", &reg.install_id);
    p.set_ttreq(Some(reg.ttreq.clone()));
    p
}

/// 注册没有「服务端按量冷却」——那是 2026-11-01 被实流取证推翻的旧猜想
/// （真相是注册代际错配被标记，不是注册次数受限），因此不设任何退避；
/// 失败只区分两类：网络类（断网/超时）等网恢复就地重试，服务端明确
/// 拒绝（形态/代际错）重试无意义立即停。
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
///
/// 运行期换档恢复（2026-11-02 二次修正）：「出生即干净终生不换」的前提
/// 再次被证伪——注册代际对齐解决的是「形态错被标记」，解决不了**发货
/// 资格在注册时抽签**（同设备同 vid 连取结果恒定，换台注册即翻盘；
/// 当日签名富 query 形态实测新注册 0/6 出票）。输家签换新注册即可，
/// 无需对照裁决（hongguo-drama 同款语义：被拒即换新，1 次/分钟限速）。
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
    meta.register_regime = hongguo_signer::REGISTER_VERSION_CODE_STR.into();
    write_meta(&state.store, meta);
    state.replace_device(profile);
}

/// 启动后台生命周期：无设备身份（匿名/全新安装）**立即**注册（实测
/// ~200ms，首批业务请求即可用上注册身份）；有身份的先看注册代际——
/// 旧注册器代际的档案按全新安装语义**一次性**迁移到当前代际（三方
/// 竞品同款语义：数据目录新建时现场注册，此后终生不换）；代际正常的
/// 再打搜索探针，空响应（死设备）也转注册。网络类失败随恢复就地重试
/// （[`register_with_retry`]），不断网不重启也能续上。
pub async fn ensure_registered(state: &AppState) {
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
            hongguo_signer::REGISTER_VERSION_CODE_STR
        );
        // 迁移要么成功（新档出生即当前代际，健康探针不必再做），
        // 要么服务端明确拒绝/网络预算耗尽（同样轮不到探针说话）
        register_with_retry(state, "注册代际迁移").await;
        return;
    }
    let env = state.api_env();
    match probe_alive(&env).await {
        Some(true) => {
            // 搜索通 ≠ 取流通：被标记/抽到输家签的设备读接口全通、唯取流
            // 被拒（2026-11-02 实录），搜索探针之后追加持流探针；未命中
            // 直接换新注册（发货资格注册时抽签，换台即翻盘）。
            match probe_stream(state).await {
                StreamProbe::Alive | StreamProbe::Unknown => {}
                StreamProbe::Marked => {
                    log::warn!(
                        "[Device] 现役档案取流探针未命中（iid={}），换新注册",
                        state.device().get("iid")
                    );
                    register_with_retry(state, "取流探针判死").await;
                }
            }
        }
        Some(false) => {
            log::warn!(
                "[Device] 现役档案探针空响应（iid={}），转注册轮换",
                state.device().get("iid")
            );
            register_with_retry(state, "健康探针判死").await;
        }
        None => log::info!("[Device] 探针不确定（网络/服务端），按兵不动"),
    }
}

/// 是否需要注册代际迁移：代际标记与当前注册器不符（含缺失——任何
/// 在标记机制引入前注册的档案都算旧代际）。
fn needs_regime_migration(meta: &BootstrapMeta) -> bool {
    meta.register_regime != hongguo_signer::REGISTER_VERSION_CODE_STR
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

// ---------------------------------------------------------------- 取流健康守护

/// 连续取流未命中多少次触发换新注册。只数**真实播放路径**上的
/// [`hongguo_common::AppError::StreamMiss`]（预取失败静默，不计）；健康设备
/// 不该连续两次取不到流，被服务端标记的设备则每集必挂——2 次（≈一次
/// 手动重试）即值得换。
const STREAM_MISS_THRESHOLD: u32 = 2;

/// 换新注册的最小间隔（毫秒）：发货资格是注册时抽签，输家签换一台注册
/// 即翻盘，1 次/分钟足够（hongguo-drama 同款续期限速）。
const STREAM_RENEW_MIN_INTERVAL_MS: u64 = 60_000;

static STREAM_MISS_COUNT: AtomicU32 = AtomicU32::new(0);
static STREAM_RENEW_AT_MS: AtomicU64 = AtomicU64::new(0);
static STREAM_RENEW_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// 换新是否该发起：连续失败达阈值，且距上次换新过了最小间隔（首轮放行）。
fn stream_renew_due(fails: u32, last_renew_ms: u64, now: u64) -> bool {
    fails >= STREAM_MISS_THRESHOLD
        && (last_renew_ms == 0 || now.saturating_sub(last_renew_ms) >= STREAM_RENEW_MIN_INTERVAL_MS)
}

/// 取流成功即清零计数（`online::prepare` 每次成功都会调，健康播放不断重置）。
pub fn note_stream_ok() {
    STREAM_MISS_COUNT.store(0, Ordering::Relaxed);
}

/// 取流未命中记账：连续失败达阈值就**换新注册**一台设备顶上——发货资格
/// 是注册时抽签发放的（同设备同 vid 连取结果恒定，2026-11-02 矩阵探针
/// 实测定型），输家签换台注册即翻盘。对照裁决已删：它把 1 分钟能完成的
/// 翻盘拖到 10 分钟以上，故障窗口内用户全程无流可播，而「对照也取不到
/// = 服务端整体拒发」的场景里换新注册同样无害（多打一次注册端点而已）。
///
/// 为什么必须有这道守护：2026-11-02 故障窗口实录——窗口期注册的设备被
/// 服务端标记后**终生**取不到流，用户既播不了也无路可走。限频 + 在途闸
/// 防止把注册端点打狠；预取失败与网络错误不计（前者静默、后者另有重试）。
pub fn note_stream_miss(state: &AppState, vid: &str) {
    let fails = STREAM_MISS_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    let now = now_ms().max(0) as u64;
    if !stream_renew_due(fails, STREAM_RENEW_AT_MS.load(Ordering::Relaxed), now) {
        return;
    }
    if STREAM_RENEW_IN_FLIGHT.swap(true, Ordering::SeqCst) {
        return;
    }
    STREAM_RENEW_AT_MS.store(now, Ordering::Relaxed);
    STREAM_MISS_COUNT.store(0, Ordering::Relaxed);
    let state = state.clone();
    let vid = vid.to_string();
    crate::runtime::spawn(async move {
        log::warn!("[Device] 取流连续 {fails} 次未命中，换新注册设备（vid={vid}）");
        register_and_swap(&state, "取流被拒自动换新").await;
        STREAM_RENEW_IN_FLIGHT.store(false, Ordering::SeqCst);
    });
}

/// 与在线播放同一份选流口径：无 HEVC 机器预取/探测的必须是 h264，否则
/// 探测结论与真实播放翻转（见 `play_service::online::prepare`）。
pub(crate) fn stream_cap() -> hongguo_api::stream_pick::DecoderCapability {
    hongguo_api::stream_pick::DecoderCapability::from_hevc(
        hongguo_media_platform::capability::playback::hevc_playable(),
    )
}

/// 取流探针结论。Alive=现役能取流；Marked=现役被拒；Unknown=网络/服务端
/// 问题分不清归属。
enum StreamProbe {
    Alive,
    Marked,
    Unknown,
}

/// 现役身份直接打轻请求取流（只取流表不打视频字节，廉价）。搜索探针
/// 测不出「读通但取流被标记」的死法——2026-11-02 实录：被标记设备读接口
/// 全通、唯取流 110001/未命中。
async fn probe_stream(state: &AppState) -> StreamProbe {
    let env = state.api_env();
    let Ok(page) = hongguo_api::recommend::fetch_recommend_feed("36", "", 0, &[], &env).await
    else {
        return StreamProbe::Unknown;
    };
    let Some(vid) = page
        .items
        .iter()
        .find(|i| !i.vid.is_empty())
        .map(|i| i.vid.clone())
    else {
        return StreamProbe::Unknown;
    };
    match hongguo_api::play_url::fetch_play_url(&vid, None, stream_cap(), &env).await {
        Ok(_) => StreamProbe::Alive,
        Err(hongguo_common::AppError::StreamMiss(_)) => StreamProbe::Marked,
        Err(_) => StreamProbe::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_store::Store;

    fn store() -> Store {
        Store::open_memory().expect("内存库")
    }

    /// 换新闸门的阈值与限频口径：达阈值才换，首轮放行、间隔内不动、
    /// 过了间隔放行（2026-11-02 故障窗口的守护回归锁，间隔 10min→60s
    /// 随对照裁决删除一并收紧）。
    #[test]
    fn stream_renew_due_respects_threshold_and_interval() {
        const MIN: u64 = STREAM_RENEW_MIN_INTERVAL_MS;
        // 未达阈值：不换
        assert!(!stream_renew_due(STREAM_MISS_THRESHOLD - 1, 0, 1000));
        // 首次达阈值（从未换过）：放行
        assert!(stream_renew_due(STREAM_MISS_THRESHOLD, 0, 1000));
        // 刚换过（间隔内）再连败：限频拦下
        assert!(!stream_renew_due(
            STREAM_MISS_THRESHOLD,
            1000,
            1000 + MIN - 1
        ));
        // 过了最小间隔：放行
        assert!(stream_renew_due(STREAM_MISS_THRESHOLD, 1000, 1000 + MIN));
        // 远超阈值同样受间隔约束
        assert!(!stream_renew_due(
            STREAM_MISS_THRESHOLD * 10,
            1000,
            1000 + MIN - 1
        ));
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

    /// 注册产物档案启动复检：钉注册口径不做版本对齐（自报版本与注册
    /// 指纹不同源时取流被拒）。
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

    /// 被 align 升到当前客户端口径的注册档案（版本键偏离注册口径）：
    /// 启动复检从档案自身身份重建为纯注册口径并落库。
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

    /// 注册代际迁移判定：标记缺失（标记机制前的所有存量档案）或代际
    /// 不符 → 迁移；当前代际 → 不迁。
    #[test]
    fn regime_migration_only_for_mismatched_marker() {
        let mut meta = BootstrapMeta::default();
        assert!(
            needs_regime_migration(&meta),
            "无标记 = 旧注册器产物，需要迁移"
        );
        meta.register_regime = "00000".into();
        assert!(needs_regime_migration(&meta), "旧代际标记需要迁移");
        meta.register_regime = hongguo_signer::REGISTER_VERSION_CODE_STR.into();
        assert!(
            !needs_regime_migration(&meta),
            "当前代际档案不迁（终生不换设备身份）"
        );
    }
}
