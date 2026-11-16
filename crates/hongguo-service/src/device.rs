//! 设备档案装载与注册。已有身份本地对齐当前版本，缺失身份才注册。

use std::sync::atomic::{AtomicBool, Ordering};

use hongguo_api::register::{RegisterBlock, RegisterResult, load_reuse_profile, reuse_json_path};
use hongguo_signer::device::{DeviceProfile, align_app_version, anonymous_profile, fresh_cdid};
use hongguo_store::Store;

use crate::app_state::AppState;
use crate::events::ServiceEvent;

/// 启动注册完成前挡住在线取流。壳在拉起 [`ensure_registered`] 之前调用。
pub use crate::cold_start::arm as arm_cold_start;
pub use crate::cold_start::{RegistrationPhase, RegistrationStatus};

mod registration;

pub fn start_startup_registration(state: &AppState) {
    registration::start_startup(state);
}

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
    match hongguo_common::file_io::read_to_string(&path) {
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
    let path = block_sidecar(store).ok_or_else(|| {
        hongguo_common::AppError::StoreCorrupt("内存库没有可落盘的限制旁路".into())
    })?;
    let raw = serde_json::to_string(meta)
        .map_err(|e| hongguo_common::AppError::StoreCorrupt(e.to_string()))?;
    let tmp = std::path::PathBuf::from(format!("{}.tmp", path.display()));
    hongguo_common::file_io::write(&tmp, raw.as_bytes())
        .map_err(|e| hongguo_common::AppError::Io(e.to_string()))?;
    hongguo_common::file_io::rename(&tmp, &path).map_err(|e| {
        let _ = hongguo_common::file_io::remove_file(&tmp);
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
    hongguo_common::file_io::write(&tmp, b"unknown\n")
        .map_err(|e| hongguo_common::AppError::Io(e.to_string()))?;
    hongguo_common::file_io::rename(&tmp, &path).map_err(|e| {
        let _ = hongguo_common::file_io::remove_file(&tmp);
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
    match hongguo_common::file_io::read_to_string(&path) {
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

fn remove_unknown_marker(store: &Store) -> hongguo_common::AppResult<()> {
    let Some(path) = unknown_marker(store) else {
        return Ok(());
    };
    match hongguo_common::file_io::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(hongguo_common::AppError::Io(err.to_string())),
    }
}

fn remove_block_sidecar(store: &Store) -> hongguo_common::AppResult<()> {
    let Some(path) = block_sidecar(store) else {
        return Ok(());
    };
    match hongguo_common::file_io::remove_file(&path) {
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

fn write_meta_strict(store: &Store, meta: &BootstrapMeta) -> hongguo_common::AppResult<()> {
    let v = serde_json::to_value(meta)
        .map_err(|e| hongguo_common::AppError::StoreCorrupt(e.to_string()))?;
    store.device_meta_set(&v)
}

/// 只初始化新设备库或可识别的匿名档案，保留既有注册限制。
pub fn open_application_store(path: &std::path::Path) -> hongguo_common::AppResult<Store> {
    let first_install = is_new_device_db(path);
    let store = Store::open_with_lock_retry(path)?;
    if reuse_json_path().is_some() {
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
    registration::ensure(state).await;
}

/// 预取与在线播放共用内置解码约束。
pub(crate) fn stream_cap() -> hongguo_api::stream_pick::DecoderCapability {
    crate::capability_service::playback_cap()
}
