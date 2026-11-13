//! 双壳共用的设备档案装载与注册。已有身份本地对齐当前版本，缺失身份才注册。

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Instant;

use hongguo_api::client::ApiEnv;
use hongguo_api::register::{
    RegisterAttemptError, RegisterBlock, RegisterMaterial, RegisterResult, load_reuse_profile,
    register_retrying, reuse_json_path,
};
use hongguo_common::utils::time::now_ms;
use hongguo_signer::device::{DeviceProfile, align_app_version, anonymous_profile, fresh_cdid};
use hongguo_store::Store;

use crate::app_state::AppState;
use crate::events::ServiceEvent;

/// 设备注册换档的前端事件名（`ServiceEvent::DeviceRegistered` 对外名字，
/// 壳转发与前端 `EVENTS.deviceRegistered` 共用；载荷 = 新 iid）。
pub const DEVICE_REGISTERED: &str = "device-registered";

/// 启动注册完成前挡住在线取流。壳在拉起 [`ensure_registered`] 之前调用。
pub use crate::cold_start::arm as arm_cold_start;

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
    /// 上次注册尝试时间（ms epoch）。
    #[serde(default)]
    pub last_register_attempt_ms: Option<i64>,
    /// 注册限制的绝对截止时间（毫秒）。到点之前重启也不注册。
    #[serde(default)]
    pub register_block_until_ms: Option<i64>,
    /// 拒绝响应没有可解析的 Retry-After。不自动恢复，重启也不清掉。
    #[serde(default)]
    pub register_block_manual: bool,
    /// 限制记录的单调序号。主库与旁文件冲突时序号大的生效，相同则主库优先。
    #[serde(default)]
    pub register_block_seq: u64,
    /// 读到了限制位置但内容不可用。只在内存里置位，解析失败不能把它当成 false。
    #[serde(default)]
    pub register_block_unknown: bool,
    /// 上次注册失败原因（成功清空；诊断记录，不参与任何判定）。
    #[serde(default)]
    pub last_register_error: Option<String>,
    /// 注册时的客户端与 SDK 版本，供诊断读取。
    #[serde(default)]
    pub register_regime: String,
}

static BLOCK_IO: std::sync::Mutex<()> = std::sync::Mutex::new(());
static FAIL_SIDECAR_WRITES: AtomicU32 = AtomicU32::new(0);
static FAIL_SIDECAR_DELETES: AtomicU32 = AtomicU32::new(0);
static FAIL_JOURNAL_RENAMES: AtomicU32 = AtomicU32::new(0);

enum BlockSource {
    Absent,
    Present(BootstrapMeta),
    Unreadable,
}

pub fn read_meta(store: &Store) -> BootstrapMeta {
    let _guard = BLOCK_IO.lock().unwrap_or_else(|e| e.into_inner());
    load_meta_locked(store)
}

fn load_meta_locked(store: &Store) -> BootstrapMeta {
    let db = load_db_block(store);
    let side = load_sidecar_block(store);
    let mut meta = merge_block_sources(db, side);
    let no_record = matches!(
        (load_db_block(store), load_sidecar_block(store)),
        (BlockSource::Absent, BlockSource::Absent)
    );
    if no_record
        || matches!(marker_state(store), BlockSource::Unreadable)
        || matches!(journal_state(store), BlockSource::Unreadable)
        || matches!(load_db_block(store), BlockSource::Unreadable)
        || matches!(load_sidecar_block(store), BlockSource::Unreadable)
    {
        // 没有可读的持久记录，或记录损坏：无自动注册资格。
        meta.register_block_unknown = true;
    }
    meta
}

fn load_db_block(store: &Store) -> BlockSource {
    match store.device_meta_get() {
        Err(_) => BlockSource::Unreadable,
        Ok(None) => BlockSource::Absent,
        Ok(Some(value)) => match serde_json::from_value::<BootstrapMeta>(value) {
            Ok(meta) => BlockSource::Present(meta),
            Err(_) => BlockSource::Unreadable,
        },
    }
}

fn load_sidecar_block(store: &Store) -> BlockSource {
    let Some(path) = block_sidecar(store) else {
        return BlockSource::Absent;
    };
    match hongguo_store::paths::qa_read_to_string(&path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => BlockSource::Absent,
        Err(_) => BlockSource::Unreadable,
        Ok(raw) => match serde_json::from_str::<BootstrapMeta>(&raw) {
            Ok(meta) => BlockSource::Present(meta),
            Err(_) => BlockSource::Unreadable,
        },
    }
}

fn merge_block_sources(db: BlockSource, side: BlockSource) -> BootstrapMeta {
    if matches!(db, BlockSource::Unreadable) || matches!(side, BlockSource::Unreadable) {
        let mut meta = match (db, side) {
            (BlockSource::Present(meta), _) | (_, BlockSource::Present(meta)) => meta,
            _ => BootstrapMeta::default(),
        };
        meta.register_block_unknown = true;
        return meta;
    }
    match (db, side) {
        (BlockSource::Present(db), BlockSource::Present(side)) => {
            if side.register_block_seq > db.register_block_seq {
                side
            } else {
                db
            }
        }
        (BlockSource::Present(meta), BlockSource::Absent)
        | (BlockSource::Absent, BlockSource::Present(meta)) => meta,
        _ => BootstrapMeta::default(),
    }
}

fn block_sidecar(store: &Store) -> Option<std::path::PathBuf> {
    let path = store.db_path();
    if path == std::path::Path::new(":memory:") {
        return None;
    }
    let mut name = path.as_os_str().to_os_string();
    name.push(".register-block");
    Some(std::path::PathBuf::from(name))
}

fn write_block_sidecar(store: &Store, meta: &BootstrapMeta) -> hongguo_common::AppResult<()> {
    if FAIL_SIDECAR_WRITES.load(Ordering::Relaxed) > 0 {
        FAIL_SIDECAR_WRITES.fetch_sub(1, Ordering::Relaxed);
        return Err(hongguo_common::AppError::Io("故障注入：旁文件写入".into()));
    }
    let path = block_sidecar(store).ok_or_else(|| {
        hongguo_common::AppError::StoreCorrupt("内存库没有可落盘的限制旁路".into())
    })?;
    let raw = serde_json::to_string(meta)
        .map_err(|e| hongguo_common::AppError::StoreCorrupt(e.to_string()))?;
    let tmp = std::path::PathBuf::from(format!("{}.tmp", path.display()));
    hongguo_store::paths::qa_write(&tmp, raw.as_bytes())
        .map_err(|e| hongguo_common::AppError::Io(e.to_string()))?;
    hongguo_store::paths::qa_rename(&tmp, &path).map_err(|e| {
        let _ = hongguo_store::paths::qa_remove_file(&tmp);
        hongguo_common::AppError::Io(e.to_string())
    })
}

fn unknown_marker(store: &Store) -> Option<std::path::PathBuf> {
    block_sidecar(store).map(|path| std::path::PathBuf::from(format!("{}.unknown", path.display())))
}

fn marker_state(store: &Store) -> BlockSource {
    let Some(path) = unknown_marker(store) else {
        return BlockSource::Absent;
    };
    let Ok(path) = hongguo_store::paths::prepare_qa_io(&path) else {
        return BlockSource::Unreadable;
    };
    match std::fs::symlink_metadata(&path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => BlockSource::Absent,
        Err(_) => BlockSource::Unreadable,
        Ok(meta) if meta.file_type().is_symlink() => BlockSource::Unreadable,
        Ok(_) => BlockSource::Unreadable,
    }
}

fn write_unknown_marker(store: &Store) -> hongguo_common::AppResult<()> {
    let path = unknown_marker(store).ok_or_else(|| {
        hongguo_common::AppError::StoreCorrupt("内存库没有可落盘的未知限制标记".into())
    })?;
    let tmp = std::path::PathBuf::from(format!("{}.tmp", path.display()));
    hongguo_store::paths::qa_write(&tmp, b"unknown\n")
        .map_err(|e| hongguo_common::AppError::Io(e.to_string()))?;
    hongguo_store::paths::qa_rename(&tmp, &path).map_err(|e| {
        let _ = hongguo_store::paths::qa_remove_file(&tmp);
        hongguo_common::AppError::Io(e.to_string())
    })
}

fn journal_path(store: &Store) -> Option<std::path::PathBuf> {
    block_sidecar(store).map(|path| std::path::PathBuf::from(format!("{}.journal", path.display())))
}

fn journal_state(store: &Store) -> BlockSource {
    let Some(path) = journal_path(store) else {
        return BlockSource::Absent;
    };
    match hongguo_store::paths::qa_read_to_string(&path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => BlockSource::Absent,
        Err(_) => BlockSource::Unreadable,
        Ok(raw) => match serde_json::from_str::<BootstrapMeta>(&raw) {
            Ok(meta) => {
                let _ = meta;
                BlockSource::Unreadable
            }
            Err(_) => BlockSource::Unreadable,
        },
    }
}

fn write_journal(store: &Store, meta: &BootstrapMeta) -> hongguo_common::AppResult<()> {
    let path = journal_path(store).ok_or_else(|| {
        hongguo_common::AppError::StoreCorrupt("内存库没有可落盘的限制日志".into())
    })?;
    let raw = serde_json::to_string(meta)
        .map_err(|e| hongguo_common::AppError::StoreCorrupt(e.to_string()))?;
    let tmp = std::path::PathBuf::from(format!("{}.tmp", path.display()));
    hongguo_store::paths::qa_write(&tmp, raw.as_bytes())
        .map_err(|e| hongguo_common::AppError::Io(e.to_string()))?;
    if FAIL_JOURNAL_RENAMES.load(Ordering::Relaxed) > 0 {
        FAIL_JOURNAL_RENAMES.fetch_sub(1, Ordering::Relaxed);
        let _ = hongguo_store::paths::qa_remove_file(&tmp);
        return Err(hongguo_common::AppError::Io(
            "故障注入：限制日志改名".into(),
        ));
    }
    hongguo_store::paths::qa_rename(&tmp, &path).map_err(|e| {
        let _ = hongguo_store::paths::qa_remove_file(&tmp);
        hongguo_common::AppError::Io(e.to_string())
    })
}

fn remove_journal(store: &Store) -> hongguo_common::AppResult<()> {
    let Some(path) = journal_path(store) else {
        return Ok(());
    };
    match hongguo_store::paths::qa_remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(hongguo_common::AppError::Io(err.to_string())),
    }
}

fn remove_unknown_marker(store: &Store) -> hongguo_common::AppResult<()> {
    let Some(path) = unknown_marker(store) else {
        return Ok(());
    };
    match hongguo_store::paths::qa_remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(hongguo_common::AppError::Io(err.to_string())),
    }
}

fn remove_block_sidecar(store: &Store) -> hongguo_common::AppResult<()> {
    if FAIL_SIDECAR_DELETES.load(Ordering::Relaxed) > 0 {
        FAIL_SIDECAR_DELETES.fetch_sub(1, Ordering::Relaxed);
        return Err(hongguo_common::AppError::Io("故障注入：旁文件删除".into()));
    }
    let Some(path) = block_sidecar(store) else {
        return Ok(());
    };
    match hongguo_store::paths::qa_remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(hongguo_common::AppError::Io(err.to_string())),
    }
}

fn next_block_seq(store: &Store) -> u64 {
    let db = match load_db_block(store) {
        BlockSource::Present(meta) => meta.register_block_seq,
        _ => 0,
    };
    let side = match load_sidecar_block(store) {
        BlockSource::Present(meta) => meta.register_block_seq,
        _ => 0,
    };
    db.max(side).saturating_add(1)
}

pub fn write_meta(store: &Store, meta: &BootstrapMeta) {
    if let Err(e) = write_meta_strict(store, meta) {
        log::warn!("[Device] 元数据落库失败（不影响换档）: {e}");
    }
}

fn write_meta_strict(store: &Store, meta: &BootstrapMeta) -> hongguo_common::AppResult<()> {
    let v = serde_json::to_value(meta)
        .map_err(|e| hongguo_common::AppError::StoreCorrupt(e.to_string()))?;
    store.device_meta_set(&v)
}

/// 只初始化新设备库或可识别的匿名档案，保留既有注册限制。
pub fn open_application_store(path: &std::path::Path) -> hongguo_common::AppResult<Store> {
    let first_install = is_new_device_db(path);
    let store = Store::open_with_lock_retry(path)?;
    if hongguo_common::offline_qa::enabled() || reuse_json_path().is_some() {
        return Ok(store);
    }
    let _guard = BLOCK_IO.lock().unwrap_or_else(|e| e.into_inner());
    if matches!(load_db_block(&store), BlockSource::Absent)
        && matches!(load_sidecar_block(&store), BlockSource::Absent)
        && matches!(marker_state(&store), BlockSource::Absent)
        && matches!(journal_state(&store), BlockSource::Absent)
    {
        let anonymous = store
            .device_profile()
            .ok()
            .flatten()
            .filter(is_known_anonymous);
        if first_install || anonymous.is_some() {
            store.initialize_device_meta(serde_json::json!({"source": "anonymous"}), anonymous)?;
        }
    }
    Ok(store)
}

fn is_known_anonymous(profile: &DeviceProfile) -> bool {
    let cdid = profile.get("cdid");
    if !is_generated_cdid(cdid) {
        return false;
    }
    if *profile == anonymous_profile(cdid) {
        return true;
    }
    let version = profile.get("version_code");
    let channel = profile.get("channel");
    let supported = match (version, profile.get("version_name"), channel) {
        ("73932", "7.3.9.32", "seo_laxin_pc_android")
        | ("74232", "7.4.2.32", "seo_laxin_pc_pinzhuan_android") => true,
        ("74232", "7.4.2.32", channel) => channel.strip_suffix("_8662_64").is_some_and(|brand| {
            brand == profile.get("device_brand")
                && matches!(
                    brand,
                    "xiaomi" | "redmi" | "iqoo" | "vivo" | "oppo" | "oneplus" | "honor" | "samsung"
                )
        }),
        _ => false,
    };
    if !supported {
        return false;
    }
    profile.get("aid") == hongguo_signer::device::APP_ID
        && profile.get("app_name") == "novelread"
        && profile
            .user_agent()
            .starts_with(&format!("com.phoenix.read/{version} "))
        && ["manifest_version_code", "update_version_code", "pv_player"]
            .iter()
            .all(|field| profile.get(field) == version)
        && profile.ttreq().is_none()
        && profile
            .iter()
            .all(|(field, _)| !matches!(field, "iid" | "device_id" | "openudid"))
}

fn is_generated_cdid(cdid: &str) -> bool {
    let bytes = cdid.as_bytes();
    bytes.len() == 36
        && bytes[14] == b'4'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        && bytes.iter().enumerate().all(|(index, &byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
        })
}

fn is_new_device_db(path: &std::path::Path) -> bool {
    path != std::path::Path::new(":memory:")
        && ["", "-wal", "-shm", "-journal"].iter().all(|suffix| {
            let mut name = path.as_os_str().to_os_string();
            name.push(suffix);
            std::fs::symlink_metadata(name)
                .is_err_and(|err| err.kind() == std::io::ErrorKind::NotFound)
        })
}

/// 本地装载档案并对齐当前版本，已有设备身份保持稳定。
pub fn adopt_sync(store: &Store) -> (DeviceProfile, &'static str) {
    match store.device_profile() {
        Ok(Some(p)) => {
            if hongguo_api::register::has_registered_ids(&p) {
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
    let klink_egdi = p.get("klink_egdi");
    let healed = registered_profile(&RegisterResult {
        device_id: p.get("device_id").into(),
        install_id: p.get("iid").into(),
        cdid: p.get("cdid").into(),
        openudid: p.get("openudid").into(),
        ttreq: p.ttreq().unwrap_or_default().into(),
        klink_egdi: (!klink_egdi.is_empty()).then(|| klink_egdi.into()),
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

/// 注册产物 → 现役档案：与注册请求共用 [`DeviceProfile::fresh_register_profile`] 构造，
/// 只补注册响应下发的设备身份。注册口径钉在构造器里，**不做** `align_app_version`
/// （自报版本与注册指纹必须同源，否则取流被拒）。
pub fn registered_profile(reg: &RegisterResult) -> DeviceProfile {
    let mut p = DeviceProfile::fresh_register_profile(&reg.cdid, &reg.openudid);
    p.set("device_id", &reg.device_id);
    p.set("iid", &reg.install_id);
    p.set_ttreq(Some(reg.ttreq.clone()));
    if let Some(klink_egdi) = reg.klink_egdi.as_deref().filter(|value| !value.is_empty()) {
        p.set("klink_egdi", klink_egdi);
    }
    p
}

/// 瞬态失败的重试次数与间隔。同一次尝试复用同一份身份材料。
const REGISTER_RETRY_MS: u64 = 60_000;
const REGISTER_RETRY_MAX: usize = 30;

/// 元数据没写成时，本进程仍拒绝注册。重启后只认已经落库的限制。
static REGISTER_FORCE_BLOCK: AtomicBool = AtomicBool::new(false);

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

/// 注册新设备并换档。库提交成功后才替换内存、才发成功事件。
pub async fn register_and_swap(state: &AppState, reason: &str) -> Registration {
    register_and_swap_at(
        state,
        reason,
        hongguo_api::register::REGISTER_ORIGIN,
        std::time::Duration::from_millis(REGISTER_RETRY_MS),
    )
    .await
}

async fn register_and_swap_at(
    state: &AppState,
    reason: &str,
    origin: &str,
    pause: std::time::Duration,
) -> Registration {
    if hongguo_common::offline_qa::enabled() {
        return refused(state, "离线 QA 不发起注册");
    }
    if let Some(refusal) = reuse_refusal(state) {
        return refusal;
    }
    let mut meta = read_meta(&state.store);
    if registration_blocked(&meta, now_ms()) {
        return refused(state, "无可信持久状态或限制仍有效，不自动注册");
    }
    meta.last_register_attempt_ms = Some(now_ms());
    if let Err(e) = write_meta_strict(&state.store, &meta) {
        return refused(state, &e.to_string());
    }
    let reg_env = ApiEnv {
        proxy: state.settings().proxy,
        device: anonymous_profile(&fresh_cdid()),
        cookie: None,
        x_tt_token: None,
    };
    let material = RegisterMaterial::mint();
    let started = Instant::now();
    match register_retrying(&reg_env, &material, origin, REGISTER_RETRY_MAX, pause).await {
        Ok(r) => {
            log::info!(
                "[Device] 服务端返回了注册响应（{reason}） elapsedMs={}",
                started.elapsed().as_millis()
            );
            activate_registration(state, &mut meta, &r, reason)
        }
        Err(RegisterAttemptError::Blocked(block)) => {
            if let Err(e) = persist_block(state, &mut meta, block) {
                REGISTER_FORCE_BLOCK.store(true, Ordering::Relaxed);
                return refused(state, &format!("注册限制没有落盘，停止自动注册: {e}"));
            }
            refused(state, "注册被拒绝或限流")
        }
        Err(RegisterAttemptError::ReuseOnly) => {
            reuse_refusal(state).unwrap_or_else(|| refused(state, "只复用模式禁止注册"))
        }
        Err(err) => {
            let app_err = hongguo_common::AppError::from(err);
            let network = matches!(app_err, hongguo_common::AppError::Network(_));
            meta.last_register_error = Some(app_err.to_string());
            let _ = write_meta_strict(&state.store, &meta);
            Registration {
                ok: false,
                iid: state.device().get("iid").to_string(),
                device_id: String::new(),
                kind: "error",
                error: Some(app_err.to_string()),
                network,
            }
        }
    }
}

/// 本地激活：事务提交并替换内存之后才记成功、才发事件。
fn activate_registration(
    state: &AppState,
    meta: &mut BootstrapMeta,
    reg: &RegisterResult,
    reason: &str,
) -> Registration {
    let _guard = BLOCK_IO.lock().unwrap_or_else(|e| e.into_inner());
    let profile = registered_profile(reg);
    meta.last_register_error = None;
    meta.register_block_manual = false;
    meta.register_block_until_ms = None;
    meta.register_block_unknown = false;
    meta.register_block_seq = next_block_seq(&state.store);
    if let Err(e) = swap_profile(state, meta, profile, "registered") {
        return refused(state, &e.to_string());
    }
    if remove_block_sidecar(&state.store).is_ok() && remove_unknown_marker(&state.store).is_ok() {
        REGISTER_FORCE_BLOCK.store(false, Ordering::Relaxed);
    }
    hongguo_api::recommend::clear_feed_tabs_cache();
    hongguo_api::play_url::clear_batch_cache();
    log::info!("[Device] 本地档案已激活（{reason}）");
    state
        .events()
        .publish(ServiceEvent::DeviceRegistered(reg.install_id.clone()));
    Registration {
        ok: true,
        iid: reg.install_id.clone(),
        device_id: reg.device_id.clone(),
        kind: "registered",
        error: None,
        network: false,
    }
}

fn refused(state: &AppState, message: &str) -> Registration {
    log::warn!("[Device] {message}");
    Registration {
        ok: false,
        iid: state.device().get("iid").to_string(),
        device_id: String::new(),
        kind: "error",
        error: Some(message.to_string()),
        network: false,
    }
}

fn reuse_refusal(state: &AppState) -> Option<Registration> {
    let path = reuse_json_path()?;
    match load_reuse_profile(&path) {
        Ok(profile) => {
            if state.device().get("iid") != profile.get("iid")
                && let Err(e) = install_reuse(state, &profile)
            {
                return Some(refused(state, &e.to_string()));
            }
            Some(refused(state, "只复用模式禁止注册"))
        }
        Err(e) => Some(refused(state, &e.to_string())),
    }
}

fn install_reuse(state: &AppState, profile: &DeviceProfile) -> hongguo_common::AppResult<()> {
    let mut meta = read_meta(&state.store);
    swap_profile(state, &mut meta, profile.clone(), "reuse")
}

/// 限制窗口必须留下重启后仍能读到的记录。主库失败时写旁文件；两边都失败再写未知标记。
fn persist_block(
    state: &AppState,
    meta: &mut BootstrapMeta,
    block: RegisterBlock,
) -> hongguo_common::AppResult<()> {
    let _guard = BLOCK_IO.lock().unwrap_or_else(|e| e.into_inner());
    apply_register_block(meta, block);
    meta.register_block_seq = next_block_seq(&state.store);
    meta.register_block_unknown = false;
    match write_meta_strict(&state.store, meta) {
        Ok(()) => {
            let _ = remove_block_sidecar(&state.store);
            let _ = remove_unknown_marker(&state.store);
            Ok(())
        }
        Err(db_err) => match write_block_sidecar(&state.store, meta) {
            Ok(()) => Ok(()),
            Err(side_err) => match write_unknown_marker(&state.store) {
                Ok(()) => {
                    REGISTER_FORCE_BLOCK.store(true, Ordering::Relaxed);
                    Ok(())
                }
                Err(marker_err) => {
                    REGISTER_FORCE_BLOCK.store(true, Ordering::Relaxed);
                    Err(hongguo_common::AppError::StoreCorrupt(format!(
                        "{db_err}；旁路也失败: {side_err}；未知标记也失败: {marker_err}"
                    )))
                }
            },
        },
    }
}

/// 显式解除限制。任何一步失败都留下仍能阻断的记录；启动流程不会调用这里。
pub fn clear_register_block(store: &Store) -> hongguo_common::AppResult<()> {
    let _guard = BLOCK_IO.lock().unwrap_or_else(|e| e.into_inner());
    let saved_side =
        block_sidecar(store).and_then(|path| hongguo_store::paths::qa_read(&path).ok());
    let saved_marker =
        unknown_marker(store).and_then(|path| hongguo_store::paths::qa_read(&path).ok());
    let mut blocking = load_meta_locked(store);
    blocking.register_block_unknown = true;
    write_journal(store, &blocking)?;
    remove_block_sidecar(store)?;
    if let Err(err) = remove_unknown_marker(store) {
        restore_bytes(block_sidecar(store), saved_side.as_deref())?;
        return Err(err);
    }
    let mut cleared = match load_db_block(store) {
        BlockSource::Present(meta) => meta,
        BlockSource::Absent => BootstrapMeta::default(),
        BlockSource::Unreadable => {
            restore_bytes(block_sidecar(store), saved_side.as_deref())?;
            restore_bytes(unknown_marker(store), saved_marker.as_deref())?;
            return Err(hongguo_common::AppError::StoreCorrupt(
                "主库限制不可读，不能解除".into(),
            ));
        }
    };
    cleared.register_block_manual = false;
    cleared.register_block_until_ms = None;
    cleared.register_block_unknown = false;
    cleared.register_block_seq = next_block_seq(store);
    if let Err(err) = write_meta_strict(store, &cleared) {
        restore_bytes(block_sidecar(store), saved_side.as_deref())?;
        restore_bytes(unknown_marker(store), saved_marker.as_deref())?;
        return Err(err);
    }
    remove_journal(store)?;
    REGISTER_FORCE_BLOCK.store(false, Ordering::Relaxed);
    Ok(())
}

fn restore_bytes(
    path: Option<std::path::PathBuf>,
    bytes: Option<&[u8]>,
) -> hongguo_common::AppResult<()> {
    let (Some(path), Some(bytes)) = (path, bytes) else {
        return Ok(());
    };
    let tmp = std::path::PathBuf::from(format!("{}.restore.tmp", path.display()));
    hongguo_store::paths::qa_write(&tmp, bytes)
        .map_err(|e| hongguo_common::AppError::Io(e.to_string()))?;
    hongguo_store::paths::qa_rename(&tmp, &path).map_err(|e| {
        let _ = hongguo_store::paths::qa_remove_file(&tmp);
        hongguo_common::AppError::Io(e.to_string())
    })
}

pub fn registration_blocked(meta: &BootstrapMeta, now_ms: i64) -> bool {
    if REGISTER_FORCE_BLOCK.load(Ordering::Relaxed)
        || meta.register_block_unknown
        || meta.register_block_manual
    {
        return true;
    }
    meta.register_block_until_ms
        .is_some_and(|until| now_ms < until)
}

pub fn apply_register_block(meta: &mut BootstrapMeta, block: RegisterBlock) {
    match block {
        RegisterBlock::Until { until_ms } => {
            meta.register_block_until_ms = Some(until_ms);
            meta.register_block_manual = false;
        }
        RegisterBlock::Manual => {
            meta.register_block_manual = true;
            meta.register_block_until_ms = None;
        }
    }
}

/// 备份、现役、元数据同一事务。提交成功后才替换内存。
fn swap_profile(
    state: &AppState,
    meta: &mut BootstrapMeta,
    profile: DeviceProfile,
    source: &str,
) -> hongguo_common::AppResult<()> {
    let old_iid = state.device().get("iid").to_string();
    let new_iid = profile.get("iid").to_string();
    meta.source = source.to_string();
    meta.previous_iid = Some(old_iid).filter(|i| i != &new_iid);
    meta.register_regime = hongguo_api::register::REGISTER_REGIME.into();
    let profile_json = serde_json::to_string(&profile)
        .map_err(|e| hongguo_common::AppError::StoreCorrupt(e.to_string()))?;
    let meta_json = serde_json::to_string(&*meta)
        .map_err(|e| hongguo_common::AppError::StoreCorrupt(e.to_string()))?;
    state.store.commit_device_swap(&profile_json, &meta_json)?;
    state.replace_device(profile);
    log::info!("[Device] 换档完成（{source}）");
    Ok(())
}

/// 补齐缺失的设备身份，已有注册档案直接复用。
pub async fn ensure_registered(state: &AppState) {
    let _release_playback = crate::cold_start::ReleaseOnDrop;
    if reuse_json_path().is_some() {
        let _ = reuse_refusal(state);
        return;
    }
    let meta = read_meta(&state.store);
    if registration_blocked(&meta, now_ms()) {
        // G1：离线 QA 下 registration_blocked 早退也要打出清单可检索句，
        // 不改变不注册语义，仅补日志对齐 ZERO-EXTERNAL-CHECKLIST。
        if hongguo_common::offline_qa::enabled() {
            log::info!("[Device] 离线 QA：不注册、不打健康探针、不做行为预热");
        }
        log::warn!("[Device] 无可信持久状态或限制仍有效，启动不自动注册");
        return;
    }
    if hongguo_common::offline_qa::enabled() {
        log::info!("[Device] 离线 QA：不注册、不打健康探针、不做行为预热");
        return;
    }
    if !hongguo_api::register::has_registered_ids(&state.device()) {
        log::info!("[Device] 缺少有效注册身份，启动自动注册");
        register_and_swap(state, "启动自动注册").await;
        return;
    }
    log::info!("[Device] 启动复用已注册档案");
}

/// 预取与在线播放共用内置解码约束。
pub(crate) fn stream_cap() -> hongguo_api::stream_pick::DecoderCapability {
    crate::capability_service::playback_cap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_store::Store;

    fn store() -> Store {
        Store::open_memory().expect("内存库")
    }

    #[test]
    fn register_block_survives_reopen_and_manual_has_no_expiry() {
        let dir = std::env::temp_dir().join(format!("hg-block-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("device.db");
        let db = Store::open(&path).expect("临时库");
        let mut meta = BootstrapMeta::default();
        apply_register_block(
            &mut meta,
            RegisterBlock::Until {
                until_ms: now_ms() + 60_000,
            },
        );
        write_meta_strict(&db, &meta).expect("写入限制");
        drop(db);
        let reopened = Store::open(&path).expect("重启后的临时库");
        assert!(registration_blocked(&read_meta(&reopened), now_ms()));
        let mut manual = BootstrapMeta::default();
        apply_register_block(&mut manual, RegisterBlock::Manual);
        write_meta_strict(&reopened, &manual).expect("写入无头限制");
        drop(reopened);
        let again = Store::open(&path).expect("再次打开");
        let loaded = read_meta(&again);
        assert!(loaded.register_block_manual);
        assert!(loaded.register_block_until_ms.is_none());
        assert!(registration_blocked(&loaded, now_ms() + 86_400_000));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn swap_fault_keeps_old_profile_and_skips_success() {
        let dir = std::env::temp_dir().join(format!("hg-swap-fault-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Store::open(dir.join("device.db")).expect("临时库");
        let old = registered_profile(&reg_result());
        db.save_device_profile(&old).expect("旧档");
        let state = std::sync::Arc::new(crate::app_state::AppStateInner::with_device(
            db.clone(),
            old.clone(),
        ));
        let mut rx = state.events().subscribe();
        let mut meta = read_meta(&db);
        let profile_json = serde_json::to_string(&registered_profile(&RegisterResult {
            install_id: "111".into(),
            device_id: "222".into(),
            ..reg_result()
        }))
        .unwrap();
        let meta_json = serde_json::to_string(&meta).unwrap();
        let err = db
            .commit_device_swap_fault(&profile_json, &meta_json, 2)
            .expect_err("注入故障");
        assert!(err.to_string().contains("故障注入"));
        let kept = db.device_profile().expect("读").expect("旧档还在");
        assert_eq!(kept.get("iid"), old.get("iid"));
        assert_eq!(state.device().get("iid"), old.get("iid"));
        assert!(rx.try_recv().is_err(), "失败不发成功事件");
        let mut newer = old.clone();
        newer.set("iid", "1234567890123456");
        let generation = state.device_generation();
        swap_profile(&state, &mut meta, newer, "registered").expect("成功提交才换内存");
        assert_eq!(state.device().get("iid"), "1234567890123456");
        assert!(state.device_generation() > generation);
        assert_eq!(
            db.device_profile_backup().unwrap().unwrap().get("iid"),
            old.get("iid")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn reuse_mode_and_rejection_do_not_register() {
        hongguo_api::register::reset_register_http_sends();
        let db = store();
        let old = registered_profile(&reg_result());
        db.save_device_profile(&old).unwrap();
        let state = std::sync::Arc::new(crate::app_state::AppStateInner::with_device(db, old));
        let missing = std::env::temp_dir().join(format!("hg-reuse-svc-{}", std::process::id()));
        let prev = std::env::var_os("HONGGUO_REUSE_DEVICE_JSON");
        unsafe { std::env::set_var("HONGGUO_REUSE_DEVICE_JSON", &missing) };
        let got = register_and_swap(&state, "只复用缺文件").await;
        assert!(!got.ok);
        assert_eq!(hongguo_api::register::register_http_sends(), 0);
        assert_eq!(state.device().get("iid"), "9999999999999999");
        unsafe {
            match prev {
                Some(v) => std::env::set_var("HONGGUO_REUSE_DEVICE_JSON", v),
                None => std::env::remove_var("HONGGUO_REUSE_DEVICE_JSON"),
            }
        }
        let _ = std::fs::remove_file(&missing);
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

    #[tokio::test]
    async fn startup_registers_once_then_reuses_the_activated_profile() {
        use hongguo_common::model::settings::ProxyMode;
        use hongguo_common::model::{ProxyConfig, Settings};
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

        let db = store();
        write_meta_strict(&db, &BootstrapMeta::default()).unwrap();
        let state = std::sync::Arc::new(crate::app_state::AppStateInner::with_db(db));
        let mut events = state.events().subscribe();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        state.replace_settings(Settings {
            proxy: ProxyConfig {
                mode: ProxyMode::Manual,
                url: origin.clone(),
            },
            ..Settings::default()
        });

        let expected = reg_result();
        let proxy = async {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = BufReader::new(socket);
            let mut request = String::new();
            while !request.ends_with("\r\n\r\n") {
                assert_ne!(socket.read_line(&mut request).await.unwrap(), 0);
            }
            let target = request.lines().next().unwrap();
            assert!(target.starts_with("POST "));
            assert!(target.contains("/service/2/device_register/"));
            let content_length: usize = request
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .unwrap()
                .parse()
                .unwrap();
            socket
                .read_exact(&mut vec![0; content_length])
                .await
                .unwrap();
            let body = serde_json::json!({"device_id": expected.device_id, "install_id": expected.install_id}).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\nSet-Cookie: ttreq={}\r\n\r\n{body}",
                body.len(),
                expected.ttreq,
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            drop(socket);
            listener.accept().await.unwrap();
        };
        let registration =
            register_and_swap_at(&state, "测试启动注册", &origin, std::time::Duration::ZERO);
        let registered = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::select! {
                result = async {
                    let registered = registration.await;
                    assert!(registered.ok, "{:?}", registered.error);
                    state.store.device_meta_set(&serde_json::json!({
                        "source": "registered",
                        "register_regime": "73932"
                    })).unwrap();
                    ensure_registered(&state).await;
                    registered
                } => result,
                () = proxy => panic!("设备注册完成前发起了额外网络请求"),
            }
        })
        .await
        .expect("注册应在收到响应后完成");

        assert_eq!(registered.iid, expected.install_id);
        assert_eq!(state.device_generation(), 1);
        assert_eq!(state.device().get("iid"), expected.install_id);
        assert_eq!(
            state.store.device_profile().unwrap().unwrap().get("iid"),
            expected.install_id
        );
        assert!(
            matches!(events.try_recv(), Ok(ServiceEvent::DeviceRegistered(iid)) if iid == expected.install_id)
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
        let mut damaged = registered_profile(&RegisterResult {
            klink_egdi: Some("test-egdi".into()),
            ..reg_result()
        });
        damaged.set("version_code", "1");
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
        assert_eq!(profile.get("klink_egdi"), "test-egdi");
        let stored = db.device_profile().expect("读回").expect("已落库");
        assert_eq!(
            stored.get("version_code"),
            hongguo_signer::REGISTER_VERSION_CODE_STR
        );
        assert_eq!(stored.get("klink_egdi"), "test-egdi");
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
    fn new_install_initializes_registration_but_missing_metadata_in_old_store_stays_blocked() {
        let root = std::env::temp_dir().join(format!("hg-new-install-{}", fresh_cdid()));
        let first_path = root.join("first").join("hongguo.db");
        std::fs::create_dir_all(first_path.parent().unwrap()).unwrap();
        std::fs::write(first_path.with_file_name("window-state.json"), "{}").unwrap();
        let first = open_application_store(&first_path).unwrap();
        assert!(!read_meta(&first).register_block_unknown);
        assert_eq!(read_meta(&first).source, "anonymous");

        let old_path = root.join("old").join("hongguo.db");
        let old = Store::open(&old_path).unwrap();
        drop(old);
        let reopened = open_application_store(&old_path).unwrap();
        assert!(read_meta(&reopened).register_block_unknown);
        drop(first);
        drop(reopened);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn interrupted_first_start_resumes_registration_without_changing_the_anonymous_profile() {
        let root = std::env::temp_dir().join(format!("hg-resume-install-{}", fresh_cdid()));
        let path = root.join("hongguo.db");
        let old = Store::open(&path).unwrap();
        let (device, _) = adopt_sync(&old);
        assert!(read_meta(&old).register_block_unknown);
        drop(old);

        let reopened = open_application_store(&path).unwrap();
        assert!(!read_meta(&reopened).register_block_unknown);
        assert_eq!(reopened.device_profile().unwrap(), Some(device));
        drop(reopened);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn upgrade_initializes_only_recognized_legacy_anonymous_profiles() {
        let root = std::env::temp_dir().join(format!("hg-upgrade-{}", fresh_cdid()));
        for (name, version, version_name, channel, blocked) in [
            ("v065", "73932", "7.3.9.32", "seo_laxin_pc_android", false),
            (
                "v0512",
                "74232",
                "7.4.2.32",
                "seo_laxin_pc_pinzhuan_android",
                false,
            ),
            ("v0511", "74232", "7.4.2.32", "redmi_8662_64", false),
            ("v0507", "74232", "7.4.2.32", "xiaomi_8662_64", false),
            ("current", "74332", "7.4.3.32", "update_64", true),
            ("unknown-channel", "73932", "7.3.9.32", "unknown", true),
            ("retained-block", "74332", "7.4.3.32", "update_64", true),
            (
                "previously-registered",
                "73932",
                "7.3.9.32",
                "seo_laxin_pc_android",
                true,
            ),
        ] {
            let path = root.join(name).join("hongguo.db");
            let store = Store::open(&path).unwrap();
            let cdid = fresh_cdid();
            let mut profile = anonymous_profile(&cdid);
            for field in [
                "version_code",
                "manifest_version_code",
                "update_version_code",
                "pv_player",
            ] {
                profile.set(field, version);
            }
            profile.set("channel", channel);
            if let Some(brand) = channel.strip_suffix("_8662_64") {
                profile.set("device_brand", brand);
            }
            profile.set("version_name", version_name);
            profile.set_user_agent(
                &profile
                    .user_agent()
                    .replace(hongguo_signer::APP_VERSION_CODE_STR, version),
            );
            if name == "previously-registered" {
                profile.set("iid", "123");
                profile.set("device_id", "456");
            }
            if name == "current" {
                profile.set("device_type", "unrecognized");
            }
            store.save_device_profile(&profile).unwrap();
            if name == "retained-block" {
                write_block_sidecar(
                    &store,
                    &BootstrapMeta {
                        register_block_manual: true,
                        ..BootstrapMeta::default()
                    },
                )
                .unwrap();
            }
            drop(store);

            let upgraded = open_application_store(&path).unwrap();
            assert_eq!(
                registration_blocked(&read_meta(&upgraded), now_ms()),
                blocked,
                "{name}"
            );
            let preserved = upgraded.device_profile().unwrap().unwrap();
            assert_eq!(preserved.get("cdid"), cdid);
            assert_eq!(preserved.get("iid"), profile.get("iid"));
        }
        let _ = std::fs::remove_dir_all(&root);
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

    fn clear_force_block() {
        REGISTER_FORCE_BLOCK.store(false, Ordering::Relaxed);
    }

    #[test]
    fn activation_rolls_back_steps_one_and_three_without_a_success_event() {
        clear_force_block();
        for step in [1_u8, 3] {
            let dir = std::env::temp_dir()
                .join(format!("hg-activate-fault-{step}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let db = Store::open(dir.join("device.db")).unwrap();
            let old = registered_profile(&reg_result());
            db.save_device_profile(&old).unwrap();
            let mut meta = BootstrapMeta {
                source: "registered".into(),
                register_regime: "keep-me".into(),
                ..BootstrapMeta::default()
            };
            write_meta_strict(&db, &meta).unwrap();
            let state = std::sync::Arc::new(crate::app_state::AppStateInner::with_device(
                db.clone(),
                old.clone(),
            ));
            let mut rx = state.events().subscribe();
            state.store.fail_next_device_swap(step);
            let fresh = RegisterResult {
                install_id: "3333333333333333".into(),
                device_id: "4444444444444444".into(),
                ..reg_result()
            };
            let got = activate_registration(&state, &mut meta, &fresh, "故障注入");
            assert!(!got.ok, "第 {step} 步失败不能报激活成功");
            assert_eq!(state.device().get("iid"), old.get("iid"));
            assert_eq!(
                db.device_profile().unwrap().unwrap().get("iid"),
                old.get("iid")
            );
            assert_eq!(read_meta(&db).register_regime, "keep-me");
            assert!(rx.try_recv().is_err(), "第 {step} 步失败不发成功事件");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn activation_commit_updates_memory_and_emits_once() {
        clear_force_block();
        let dir = std::env::temp_dir().join(format!("hg-activate-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Store::open(dir.join("device.db")).unwrap();
        let old = registered_profile(&reg_result());
        db.save_device_profile(&old).unwrap();
        let state = std::sync::Arc::new(crate::app_state::AppStateInner::with_device(
            db.clone(),
            old,
        ));
        let mut rx = state.events().subscribe();
        let mut meta = read_meta(&db);
        let fresh = RegisterResult {
            install_id: "5555555555555555".into(),
            device_id: "6666666666666666".into(),
            ..reg_result()
        };
        let got = activate_registration(&state, &mut meta, &fresh, "提交成功");
        assert!(got.ok);
        assert_eq!(state.device().get("iid"), "5555555555555555");
        assert_eq!(
            db.device_profile().unwrap().unwrap().get("iid"),
            "5555555555555555"
        );
        assert!(matches!(
            rx.try_recv().unwrap(),
            ServiceEvent::DeviceRegistered(_)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn failed_meta_write_still_blocks_after_reopen() {
        clear_force_block();
        let dir = std::env::temp_dir().join(format!("hg-block-side-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("device.db");
        let db = Store::open(&path).unwrap();
        let old = registered_profile(&reg_result());
        db.save_device_profile(&old).unwrap();
        let state = std::sync::Arc::new(crate::app_state::AppStateInner::with_device(
            db.clone(),
            old.clone(),
        ));
        db.fail_next_meta_writes(1);
        let mut meta = read_meta(&db);
        persist_block(
            &state,
            &mut meta,
            RegisterBlock::Until {
                until_ms: now_ms() + 120_000,
            },
        )
        .expect("主行失败后旁路应留下限制");
        drop(state);
        drop(db);
        clear_force_block();
        let reopened = Store::open(&path).unwrap();
        assert!(
            registration_blocked(&read_meta(&reopened), now_ms()),
            "重启后仍应读到限制"
        );
        let state =
            std::sync::Arc::new(crate::app_state::AppStateInner::with_device(reopened, old));
        hongguo_api::register::reset_register_http_sends();
        let got = register_and_swap(&state, "重启后").await;
        assert!(!got.ok);
        assert_eq!(hongguo_api::register::register_http_sends(), 0);
        assert_eq!(state.device().get("iid"), "9999999999999999");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn memory_store_cannot_hide_a_failed_block_write() {
        clear_force_block();
        let db = store();
        let state = std::sync::Arc::new(crate::app_state::AppStateInner::with_db(db));
        state.store.fail_next_meta_writes(1);
        let mut meta = BootstrapMeta::default();
        let err = persist_block(&state, &mut meta, RegisterBlock::Manual).unwrap_err();
        assert!(err.to_string().contains("旁路也失败"));
        assert!(REGISTER_FORCE_BLOCK.load(Ordering::Relaxed));
        assert!(registration_blocked(&BootstrapMeta::default(), now_ms()));
        clear_force_block();
    }

    #[tokio::test]
    async fn reuse_entries_reject_incomplete_profiles_without_http() {
        clear_force_block();
        hongguo_api::register::reset_register_http_sends();
        let db = store();
        let old = registered_profile(&reg_result());
        db.save_device_profile(&old).unwrap();
        let state = std::sync::Arc::new(crate::app_state::AppStateInner::with_device(
            db,
            old.clone(),
        ));
        let dir = std::env::temp_dir().join(format!("hg-reuse-fields-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut files = Vec::new();
        files.push(dir.join("missing.json"));
        let broken = dir.join("broken.json");
        std::fs::write(&broken, "{").unwrap();
        files.push(broken);
        for key in ["cdid", "openudid", "device_id", "iid"] {
            let mut profile = old.clone();
            profile.set(key, "");
            let path = dir.join(format!("{key}.json"));
            std::fs::write(&path, serde_json::to_string(&profile).unwrap()).unwrap();
            files.push(path);
        }
        let mut no_ticket = old.clone();
        no_ticket.set_ttreq(None);
        let ticket_path = dir.join("ttreq.json");
        std::fs::write(&ticket_path, serde_json::to_string(&no_ticket).unwrap()).unwrap();
        files.push(ticket_path);
        let prev = std::env::var_os("HONGGUO_REUSE_DEVICE_JSON");
        for path in &files {
            unsafe { std::env::set_var("HONGGUO_REUSE_DEVICE_JSON", path) };
            let before = hongguo_api::register::register_http_sends();
            let registered = register_and_swap(&state, "复用").await;
            ensure_registered(&state).await;
            assert!(!registered.ok);
            assert_eq!(hongguo_api::register::register_http_sends(), before);
            assert_eq!(state.device().get("iid"), old.get("iid"));
        }
        unsafe {
            match prev {
                Some(v) => std::env::set_var("HONGGUO_REUSE_DEVICE_JSON", v),
                None => std::env::remove_var("HONGGUO_REUSE_DEVICE_JSON"),
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retry_after_blocks_until_the_deadline_and_not_after() {
        clear_force_block();
        let meta = BootstrapMeta {
            register_block_until_ms: Some(1_000_000),
            ..BootstrapMeta::default()
        };
        assert!(registration_blocked(&meta, 999_999));
        assert!(!registration_blocked(&meta, 1_000_000));
        assert!(!registration_blocked(&meta, 1_000_001));
        let manual = BootstrapMeta {
            register_block_manual: true,
            register_block_until_ms: Some(1),
            ..BootstrapMeta::default()
        };
        assert!(registration_blocked(&manual, i64::MAX));
    }

    #[test]
    fn corrupt_or_unreadable_sidecar_stays_blocked_until_explicit_clear() {
        clear_force_block();
        assert_ne!(libc_euid(), 0, "权限测试不能在 root 下得出假通过");
        let dir = std::env::temp_dir().join(format!("hg-side-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("device.db");
        let db = Store::open(&path).unwrap();
        let old = registered_profile(&reg_result());
        db.save_device_profile(&old).unwrap();
        let state = std::sync::Arc::new(crate::app_state::AppStateInner::with_device(
            db.clone(),
            old.clone(),
        ));
        db.fail_next_meta_writes(1);
        let mut meta = BootstrapMeta::default();
        persist_block(&state, &mut meta, RegisterBlock::Manual).unwrap();
        let side = block_sidecar(&db).unwrap();
        std::fs::write(&side, "{").unwrap();
        drop(state);
        drop(db);
        clear_force_block();
        let reopened = Store::open(&path).unwrap();
        let loaded = read_meta(&reopened);
        assert!(loaded.register_block_unknown);
        assert!(registration_blocked(&loaded, now_ms()));
        #[cfg(windows)]
        let exclusive_sidecar = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&side)
                .unwrap()
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut no_read = std::fs::metadata(&side).unwrap().permissions();
            no_read.set_mode(0o000);
            std::fs::set_permissions(&side, no_read).unwrap();
        }
        assert!(
            std::fs::read_to_string(&side).is_err(),
            "测试必须让旁文件读取实际失败"
        );
        let unreadable = read_meta(&reopened);
        assert!(unreadable.register_block_unknown);
        assert!(registration_blocked(&unreadable, now_ms()));
        FAIL_SIDECAR_DELETES.store(1, Ordering::Relaxed);
        assert!(clear_register_block(&reopened).is_err());
        assert!(registration_blocked(&read_meta(&reopened), now_ms()));
        #[cfg(windows)]
        drop(exclusive_sidecar);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut back = std::fs::metadata(&side).unwrap().permissions();
            back.set_mode(0o644);
            std::fs::set_permissions(&side, back).unwrap();
        }
        clear_register_block(&reopened).unwrap();
        let cleared = read_meta(&reopened);
        assert!(!cleared.register_block_unknown);
        assert!(!registration_blocked(&cleared, now_ms()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_sidecar_does_not_replace_a_newer_db_block() {
        clear_force_block();
        let dir = std::env::temp_dir().join(format!("hg-side-seq-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Store::open(dir.join("device.db")).unwrap();
        let newer = BootstrapMeta {
            register_block_manual: true,
            register_block_seq: 4,
            ..BootstrapMeta::default()
        };
        write_meta_strict(&db, &newer).unwrap();
        let stale = BootstrapMeta {
            register_block_seq: 1,
            ..BootstrapMeta::default()
        };
        std::fs::write(
            block_sidecar(&db).unwrap(),
            serde_json::to_string(&stale).unwrap(),
        )
        .unwrap();
        let loaded = read_meta(&db);
        assert!(loaded.register_block_manual);
        assert_eq!(loaded.register_block_seq, 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn newer_sidecar_still_blocks_when_db_block_expired() {
        clear_force_block();
        let dir = std::env::temp_dir().join(format!("hg-side-new-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Store::open(dir.join("device.db")).unwrap();
        write_meta_strict(
            &db,
            &BootstrapMeta {
                register_block_until_ms: Some(1),
                register_block_seq: 1,
                ..BootstrapMeta::default()
            },
        )
        .unwrap();
        std::fs::write(
            block_sidecar(&db).unwrap(),
            serde_json::to_string(&BootstrapMeta {
                register_block_manual: true,
                register_block_seq: 5,
                ..BootstrapMeta::default()
            })
            .unwrap(),
        )
        .unwrap();
        assert!(registration_blocked(&read_meta(&db), i64::MAX));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn dual_write_failure_leaves_unknown_marker_across_reopen() {
        clear_force_block();
        FAIL_SIDECAR_WRITES.store(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("hg-dual-fail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("device.db");
        let db = Store::open(&path).unwrap();
        let old = registered_profile(&reg_result());
        db.save_device_profile(&old).unwrap();
        let state = std::sync::Arc::new(crate::app_state::AppStateInner::with_device(
            db.clone(),
            old.clone(),
        ));
        db.fail_next_meta_writes(1);
        let mut meta = BootstrapMeta::default();
        persist_block(&state, &mut meta, RegisterBlock::Manual).unwrap();
        drop(state);
        drop(db);
        clear_force_block();
        let reopened = Store::open(&path).unwrap();
        assert!(read_meta(&reopened).register_block_unknown);
        hongguo_api::register::reset_register_http_sends();
        let state =
            std::sync::Arc::new(crate::app_state::AppStateInner::with_device(reopened, old));
        let got = register_and_swap(&state, "双写失败后重启").await;
        assert!(!got.ok);
        assert_eq!(hongguo_api::register::register_http_sends(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_block_reads_do_not_clear_the_restriction() {
        clear_force_block();
        let dir = std::env::temp_dir().join(format!("hg-block-race-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Store::open(dir.join("device.db")).unwrap();
        let state = std::sync::Arc::new(crate::app_state::AppStateInner::with_db(db));
        let mut meta = BootstrapMeta::default();
        persist_block(&state, &mut meta, RegisterBlock::Manual).unwrap();
        let mut joins = Vec::new();
        for _ in 0..4 {
            let state = state.clone();
            joins.push(std::thread::spawn(move || {
                for _ in 0..20 {
                    assert!(registration_blocked(&read_meta(&state.store), now_ms()));
                    let mut meta = read_meta(&state.store);
                    persist_block(&state, &mut meta, RegisterBlock::Manual).unwrap();
                }
            }));
        }
        for join in joins {
            join.join().unwrap();
        }
        assert!(registration_blocked(&read_meta(&state.store), now_ms()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn missing_trusted_state_has_no_automatic_registration() {
        clear_force_block();
        hongguo_api::register::reset_register_http_sends();
        let memory = std::sync::Arc::new(crate::app_state::AppStateInner::with_db(store()));
        assert!(read_meta(&memory.store).register_block_unknown);
        ensure_registered(&memory).await;
        let registered = register_and_swap(&memory, "无可信状态").await;
        assert!(!registered.ok);
        assert_eq!(hongguo_api::register::register_http_sends(), 0);

        let dir = std::env::temp_dir().join(format!("hg-no-trust-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Store::open(dir.join("device.db")).unwrap();
        let fresh = std::sync::Arc::new(crate::app_state::AppStateInner::with_db(db));
        assert!(read_meta(&fresh.store).register_block_unknown);
        ensure_registered(&fresh).await;
        assert_eq!(hongguo_api::register::register_http_sends(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn offline_qa_does_not_lift_a_missing_trusted_block() {
        clear_force_block();
        hongguo_api::register::reset_register_http_sends();
        let previous = std::env::var_os(hongguo_common::offline_qa::ENV);
        unsafe { std::env::set_var(hongguo_common::offline_qa::ENV, "1") };
        let memory = std::sync::Arc::new(crate::app_state::AppStateInner::with_db(store()));
        ensure_registered(&memory).await;
        let reuse_path = std::env::current_dir()
            .unwrap()
            .join(".qa-a-offline-scratch")
            .join("reuse-not-created.json");
        let previous_reuse = std::env::var_os("HONGGUO_REUSE_DEVICE_JSON");
        unsafe { std::env::set_var("HONGGUO_REUSE_DEVICE_JSON", &reuse_path) };
        let registered = register_and_swap(&memory, "离线").await;
        assert!(!registered.ok);
        assert!(!reuse_path.exists());
        assert!(read_meta(&memory.store).register_block_unknown);
        assert!(registration_blocked(&read_meta(&memory.store), now_ms()));
        assert_eq!(hongguo_api::register::register_http_sends(), 0);
        match previous_reuse {
            Some(value) => unsafe { std::env::set_var("HONGGUO_REUSE_DEVICE_JSON", value) },
            None => unsafe { std::env::remove_var("HONGGUO_REUSE_DEVICE_JSON") },
        }
        match previous {
            Some(value) => unsafe { std::env::set_var(hongguo_common::offline_qa::ENV, value) },
            None => unsafe { std::env::remove_var(hongguo_common::offline_qa::ENV) },
        }
    }

    #[tokio::test]
    async fn normal_reuse_keeps_the_block_and_does_not_register() {
        clear_force_block();
        hongguo_api::register::reset_register_http_sends();
        let previous_qa = std::env::var_os(hongguo_common::offline_qa::ENV);
        let previous_reuse = std::env::var_os("HONGGUO_REUSE_DEVICE_JSON");
        unsafe { std::env::remove_var(hongguo_common::offline_qa::ENV) };
        let dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.qa-a-premise-scratch")
            })
            .join(format!("reuse-normal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let profile = registered_profile(&reg_result());
        let path = dir.join("device.json");
        std::fs::write(&path, serde_json::to_string(&profile).unwrap()).unwrap();
        unsafe { std::env::set_var("HONGGUO_REUSE_DEVICE_JSON", &path) };
        let memory = std::sync::Arc::new(crate::app_state::AppStateInner::with_db(store()));
        ensure_registered(&memory).await;
        assert!(read_meta(&memory.store).register_block_unknown);
        assert_eq!(memory.device().get("iid"), "9999999999999999");
        assert_eq!(hongguo_api::register::register_http_sends(), 0);
        unsafe { std::env::set_var(hongguo_common::offline_qa::ENV, "1") };
        assert!(read_meta(&memory.store).register_block_unknown);
        assert_eq!(hongguo_api::register::register_http_sends(), 0);
        match previous_reuse {
            Some(value) => unsafe { std::env::set_var("HONGGUO_REUSE_DEVICE_JSON", value) },
            None => unsafe { std::env::remove_var("HONGGUO_REUSE_DEVICE_JSON") },
        }
        match previous_qa {
            Some(value) => unsafe { std::env::set_var(hongguo_common::offline_qa::ENV, value) },
            None => unsafe { std::env::remove_var(hongguo_common::offline_qa::ENV) },
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn temp_symlinks_do_not_change_an_outside_sentinel() {
        clear_force_block();
        let root = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.qa-a-io-scratch")
            })
            .join(format!("side-tmp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let isolated = root.join("isolated");
        let outside = root.join("mock-prod");
        std::fs::create_dir_all(&isolated).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let isolated = std::fs::canonicalize(&isolated).unwrap();
        let sentinel = outside.join("SENTINEL");
        std::fs::write(&sentinel, b"KEEP").unwrap();
        let _scope = hongguo_store::paths::ScopedDataDir::new(&isolated);
        let previous_qa = std::env::var_os(hongguo_common::offline_qa::ENV);
        unsafe { std::env::set_var(hongguo_common::offline_qa::ENV, "1") };
        let db = Store::open(isolated.join("hongguo.db")).unwrap();
        let link_to = |name: &std::path::Path| {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&sentinel, name).unwrap();
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(&sentinel, name).unwrap();
        };
        let unknown_tmp =
            std::path::PathBuf::from(format!("{}.tmp", unknown_marker(&db).unwrap().display()));
        link_to(&unknown_tmp);
        assert!(write_unknown_marker(&db).is_err());
        let journal_tmp =
            std::path::PathBuf::from(format!("{}.tmp", journal_path(&db).unwrap().display()));
        link_to(&journal_tmp);
        assert!(write_journal(&db, &BootstrapMeta::default()).is_err());
        let restore_tmp = std::path::PathBuf::from(format!(
            "{}.restore.tmp",
            block_sidecar(&db).unwrap().display()
        ));
        link_to(&restore_tmp);
        assert!(restore_bytes(block_sidecar(&db), Some(b"RESTORED")).is_err());
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"KEEP");
        assert!(
            std::fs::symlink_metadata(&unknown_tmp)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            std::fs::symlink_metadata(&journal_tmp)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            std::fs::symlink_metadata(&restore_tmp)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        match previous_qa {
            Some(value) => unsafe { std::env::set_var(hongguo_common::offline_qa::ENV, value) },
            None => unsafe { std::env::remove_var(hongguo_common::offline_qa::ENV) },
        }
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn clear_keeps_sidecar_block_when_db_write_fails() {
        clear_force_block();
        let dir = std::env::temp_dir().join(format!("hg-clear-db-fail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("device.db");
        let db = Store::open(&path).unwrap();
        db.fail_next_meta_writes(1);
        let state = std::sync::Arc::new(crate::app_state::AppStateInner::with_db(db.clone()));
        let mut meta = BootstrapMeta::default();
        persist_block(&state, &mut meta, RegisterBlock::Manual).unwrap();
        assert!(block_sidecar(&db).unwrap().is_file());
        db.fail_next_meta_writes(1);
        assert!(clear_register_block(&db).is_err());
        drop(state);
        clear_force_block();
        let reopened = Store::open(&path).unwrap();
        assert!(registration_blocked(&read_meta(&reopened), now_ms()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_left_by_a_crash_still_blocks_after_reopen() {
        clear_force_block();
        let dir = std::env::temp_dir().join(format!("hg-journal-crash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("device.db");
        let db = Store::open(&path).unwrap();
        let journal = journal_path(&db).unwrap();
        std::fs::write(
            &journal,
            serde_json::to_string(&BootstrapMeta {
                register_block_manual: true,
                register_block_seq: 3,
                ..BootstrapMeta::default()
            })
            .unwrap(),
        )
        .unwrap();
        drop(db);
        clear_force_block();
        let reopened = Store::open(&path).unwrap();
        assert!(read_meta(&reopened).register_block_unknown);
        assert!(registration_blocked(&read_meta(&reopened), i64::MAX));
        FAIL_JOURNAL_RENAMES.store(1, Ordering::Relaxed);
        let err = clear_register_block(&reopened).unwrap_err();
        assert!(err.to_string().contains("改名"));
        let tmp = std::path::PathBuf::from(format!("{}.tmp", journal.display()));
        assert!(!tmp.exists(), "改名失败不能留下半截日志");
        assert!(journal.is_file());
        assert!(registration_blocked(&read_meta(&reopened), now_ms()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retry_after_survives_reopen_until_the_deadline() {
        clear_force_block();
        let dir = std::env::temp_dir().join(format!("hg-until-reopen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("device.db");
        let db = Store::open(&path).unwrap();
        let state = std::sync::Arc::new(crate::app_state::AppStateInner::with_db(db));
        let mut meta = BootstrapMeta::default();
        let until = now_ms() + 60_000;
        persist_block(&state, &mut meta, RegisterBlock::Until { until_ms: until }).unwrap();
        drop(state);
        clear_force_block();
        let reopened = Store::open(&path).unwrap();
        let loaded = read_meta(&reopened);
        assert!(!loaded.register_block_unknown);
        assert!(registration_blocked(&loaded, until - 1));
        assert!(!registration_blocked(&loaded, until));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn libc_euid() -> u32 {
        #[cfg(unix)]
        {
            unsafe { libc_geteuid() }
        }
        #[cfg(not(unix))]
        {
            1
        }
    }

    #[cfg(unix)]
    unsafe fn libc_geteuid() -> u32 {
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        unsafe { geteuid() }
    }
}
