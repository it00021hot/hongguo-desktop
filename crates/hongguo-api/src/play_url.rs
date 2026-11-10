//! 取流地址：平台把每集的可用流放在 `video_model`（**JSON 字符串**）里二次解析，
//! 挑哪一条由 [`super::stream_pick`] 决定；批量/单集同端点，见 [`fetch_play_url`]。
//! ⚠️ 服务端不发货时返回 **HTTP 200 + 0 字节**——排查「接口没数据」先看字节数，不是状态码。

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::Value;

use super::stream_pick::{Definition, list_definitions, pick_stream_at};
use hongguo_common::utils::json::check_code;
use hongguo_common::{AppError, AppResult};

/// 已取到的原始流表，切档时从同一模型重新选流。
type ModelCache = HashMap<String, (Instant, Arc<Value>)>;
static BATCH_CACHE: OnceLock<Mutex<ModelCache>> = OnceLock::new();
/// 批量结果有效期：响应自带 expire_time（小时级），取保守 10 分钟。
const BATCH_TTL: Duration = Duration::from_secs(600);

fn batch_cache() -> &'static Mutex<ModelCache> {
    BATCH_CACHE.get_or_init(Mutex::default)
}

/// 同一 vid 服务端只发一次流。并行取流会让落选的那次拿到空包，
/// 之后再打同一集也是空的。所有取流共用这一把锁，成功结果留在暂存表里复用。
fn play_flight() -> &'static tokio::sync::Mutex<()> {
    static FLIGHT: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    FLIGHT.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn cached_model(vid: &str) -> Option<Arc<Value>> {
    let mut cache = batch_cache().lock();
    let fresh = cache
        .get(vid)
        .is_some_and(|(at, _)| at.elapsed() < BATCH_TTL);
    if !fresh {
        cache.remove(vid);
        return None;
    }
    cache.get(vid).map(|(_, model)| Arc::clone(model))
}

fn cached_play(
    vid: &str,
    definition: Option<u32>,
    cap: super::stream_pick::DecoderCapability,
) -> Option<PlayInfo> {
    select_play(cached_model(vid)?.as_ref(), definition, cap)
}

/// 这一集的流是否已经取到、还在有效期内。切集前用来决定要不要再打一次批量。
pub fn has_play_cached(vid: &str) -> bool {
    cached_model(vid).is_some()
}

fn store_models(models: impl IntoIterator<Item = (String, Value)>) {
    let now = Instant::now();
    let mut cache = batch_cache().lock();
    cache.retain(|_, (stored_at, _)| now.duration_since(*stored_at) < BATCH_TTL);
    cache.extend(
        models
            .into_iter()
            .map(|(vid, model)| (vid, (now, Arc::new(model)))),
    );
}

/// 清空批量取流暂存（设备换档后调用：旧设备预取的 PlayInfo 不能给新身份复用）。
pub fn clear_batch_cache() {
    batch_cache().lock().clear();
}

/// vid → content_type 注册表（进程内缓存）：批量取流的 `mixed_video_id_map`
/// 分组 key 必须与条目 content_type 一致（错配 → 空响应）；未注册的 vid 不猜 key，走单集回落。
static CONTENT_TYPES: OnceLock<Mutex<HashMap<String, i64>>> = OnceLock::new();

fn content_types() -> &'static Mutex<HashMap<String, i64>> {
    CONTENT_TYPES.get_or_init(Mutex::default)
}

/// 记录一个 vid 的条目类型（重复注册后值覆盖，服务端口径为准）。壳在起播时
/// 也应把**整剧各集**注册进去——连播的后续集只有这里能提前拿到类型，否则走单集回落。
pub fn register_content_type(vid: &str, content_type: i64) {
    if vid.is_empty() || content_type == 0 {
        return;
    }
    content_types().lock().insert(vid.to_string(), content_type);
}

fn content_type_of(vid: &str) -> Option<i64> {
    content_types().lock().get(vid).copied()
}

/// 一集的取流结果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayInfo {
    /// 视频直链
    pub url: String,
    pub file_hash: Option<String>,
    /// 是否为 CENC 加密流
    pub encrypted: bool,
    /// 加密密钥材料（`spade_a` 原始值，派生在 worker 里做）
    pub key_material: Vec<u8>,
    /// 编码器标识
    pub codec: String,
    /// 实际选中的档位
    pub definition: u32,
    /// 本集提供的全部档位，供前端渲染切换菜单
    pub definitions: Vec<Definition>,
}

#[derive(Clone, Copy)]
struct PlayOrigins<'a> {
    player: &'a str,
    saas: &'a str,
    light: &'a str,
}

fn default_origins() -> PlayOrigins<'static> {
    PlayOrigins {
        player: crate::client::SINFONLINEA_API_ORIGIN,
        saas: crate::client::READING_API_ORIGIN,
        light: crate::client::LQ_API_ORIGIN,
    }
}

/// 取一集的播放地址：批量暂存命中直接用；`definition` 缺档自动回退最高档（[`pick_stream_at`]）；
/// `cap` 决定选流模式（在线传本机探测结论，转码/下载传 `Full`）。content_type 未注册报 [`AppError::ContentTypeUnknown`]。
pub async fn fetch_play_url(
    vid: &str,
    definition: Option<u32>,
    cap: super::stream_pick::DecoderCapability,
    env: &super::client::ApiEnv,
) -> AppResult<PlayInfo> {
    if let Some(play) = cached_play(vid, definition, cap) {
        log::info!(
            "[PlayUrl] {vid} 命中已取到的流（定义 {}，编码 {}）",
            play.definition,
            if play.codec.is_empty() {
                "未标"
            } else {
                &play.codec
            }
        );
        return Ok(play);
    }

    let Some(ct) = content_type_of(vid) else {
        return Err(AppError::ContentTypeUnknown(format!(
            "vid={vid} 的条目类型未注册（先拉分集档案后再取流）"
        )));
    };
    let _flight = play_flight().lock().await;
    if let Some(play) = cached_play(vid, definition, cap) {
        return Ok(play);
    }
    match fetch_single_at(vid, ct, definition, cap, env, default_origins()).await {
        Ok(SingleFetch::Hit { model, play }) => {
            log::info!(
                "[PlayUrl] {vid} 轻请求取流命中（ct={ct}，定义 {}）",
                play.definition
            );
            store_models([(vid.to_string(), model)]);
            Ok(play)
        }
        // 有响应体但挑不出本机能播的流（编码/档位）——不是设备空响应。
        Ok(SingleFetch::NoPlayable) => Err(AppError::StreamMiss(format!(
            "取流未命中 vid={vid}（ct={ct}）"
        ))),
        // 各通道都是空字节：设备身份被静默拒发货。
        Ok(SingleFetch::SilentEmpty) => Err(AppError::EmptyResponse(format!(
            "取流空响应 vid={vid}（ct={ct}）"
        ))),
        Err(e) => Err(e),
    }
}

/// 单集取流结论：命中 / 有包不可播 / 全程空响应。
enum SingleFetch {
    Hit { model: Value, play: PlayInfo },
    NoPlayable,
    SilentEmpty,
}

#[cfg(test)]
impl SingleFetch {
    fn into_play(self) -> Option<PlayInfo> {
        match self {
            Self::Hit { play, .. } => Some(play),
            Self::NoPlayable | Self::SilentEmpty => None,
        }
    }
}

/// 取流端点的错误信封是 **base 族**（`Code` 大写 + `BaseResp`），不是 reading 系
/// 的小写 `code`——语义是「服务端对这个设备/请求不发货」，归 [`AppError::StreamMiss`]。
fn check_model_code(value: &Value) -> AppResult<()> {
    let code = value.get("Code").and_then(Value::as_i64).unwrap_or(0);
    if code == 0 {
        return Ok(());
    }
    let msg = value
        .pointer("/BaseResp/StatusMessage")
        .and_then(Value::as_str)
        .or_else(|| value.get("Message").and_then(Value::as_str))
        .unwrap_or("未知错误");
    Err(AppError::StreamMiss(format!("取流接口返回 {code}: {msg}")))
}

async fn fetch_single_at(
    vid: &str,
    content_type: i64,
    definition: Option<u32>,
    cap: super::stream_pick::DecoderCapability,
    env: &super::client::ApiEnv,
    origins: PlayOrigins<'_>,
) -> AppResult<SingleFetch> {
    let map = BTreeMap::from([(content_type.to_string(), vec![vid.to_string()])]);
    let mut saw_payload = false;

    let single_payload =
        serde_json::to_vec(&crate::params::single_model_payload(vid, content_type))
            .map_err(|e| AppError::Media(e.to_string()))?;
    let saas_payload = serde_json::to_vec(&crate::params::saas_model_payload(&map))
        .map_err(|e| AppError::Media(e.to_string()))?;
    for (origin, path, payload) in [
        (
            origins.player,
            crate::params::SINGLE_MODEL_PATH,
            single_payload,
        ),
        (origins.saas, crate::params::MODEL_LIGHT_PATH, saas_payload),
    ] {
        let bytes = match crate::client::api_call_model_light(origin, path, payload, env).await {
            Ok(bytes) => bytes,
            Err(e) => {
                log::warn!("[PlayUrl] {path} 请求失败 vid={vid}: {e}");
                continue;
            }
        };
        saw_payload |= !bytes.is_empty();
        match play_from_bytes(&bytes, vid, definition, cap) {
            Ok(Some((model, play))) => return Ok(SingleFetch::Hit { model, play }),
            Ok(None) => log::info!("[PlayUrl] {path} 无可播流 vid={vid} bytes={}", bytes.len()),
            Err(e) => log::warn!("[PlayUrl] {path} 响应不可用 vid={vid}: {e}"),
        }
    }

    let light_origins: &[&str] = if origins.saas == origins.light {
        &[origins.light]
    } else {
        &[origins.light, origins.saas]
    };
    for origin in light_origins {
        let bytes = match fallback_model_light(&map, env, origin).await {
            Ok(b) => b,
            Err(e) => {
                log::warn!("[PlayUrl] 旧 light @{origin} 失败（{e}）");
                continue;
            }
        };
        if !bytes.is_empty() {
            saw_payload = true;
        }
        match play_from_bytes(&bytes, vid, definition, cap) {
            Ok(Some((model, play))) => {
                log::info!(
                    "[PlayUrl] 旧 light 命中（vid={vid} origin={origin} codec={}）",
                    play.codec
                );
                return Ok(SingleFetch::Hit { model, play });
            }
            Ok(None) => log::info!(
                "[PlayUrl] 旧 light @{origin} 无可播流（vid={vid} bytes={}）",
                bytes.len()
            ),
            Err(e) => log::warn!("[PlayUrl] 旧 light @{origin} 响应不可用（{e}）"),
        }
    }

    Ok(if saw_payload {
        SingleFetch::NoPlayable
    } else {
        SingleFetch::SilentEmpty
    })
}

fn parse_model_response(bytes: &[u8]) -> AppResult<Option<Value>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|e| AppError::Media(format!("解析取流响应失败: {e}")))?;
    check_code(&value)?;
    check_model_code(&value)?;
    Ok(Some(value))
}

fn play_from_bytes(
    bytes: &[u8],
    vid: &str,
    definition: Option<u32>,
    cap: super::stream_pick::DecoderCapability,
) -> AppResult<Option<(Value, PlayInfo)>> {
    let Some(value) = parse_model_response(bytes)? else {
        return Ok(None);
    };
    let selected = parse_video_model(&value, vid)?
        .and_then(|model| select_play(&model, definition, cap).map(|play| (model, play)));
    if selected.is_none() {
        log::info!(
            "[PlayUrl] 有响应但挑不出系统能播的流 vid={vid} {}",
            stream_brief(&value)
        );
    }
    Ok(selected)
}

/// 挑流失败时记下各档编码，方便对上 `VideoInfo.extractFields` 的 `codec_type`。
fn stream_brief(value: &Value) -> String {
    let Some(model) = value
        .pointer("/data/video_model")
        .and_then(Value::as_str)
        .or_else(|| {
            value.get("data").and_then(Value::as_object).and_then(|o| {
                o.values()
                    .find_map(|v| v.get("video_model").and_then(Value::as_str))
            })
        })
    else {
        return "无 video_model".to_string();
    };
    let Ok(inner) = serde_json::from_str::<Value>(model) else {
        return "video_model 不是 JSON".to_string();
    };
    let Some(list) = inner.get("video_list").and_then(Value::as_array) else {
        return format!("video_list 不是数组 version={:?}", inner.get("version"));
    };
    let brief: Vec<String> = list
        .iter()
        .take(8)
        .map(|item| {
            let meta = item.get("video_meta");
            let def = meta
                .and_then(|m| m.get("definition"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            let codec = meta
                .and_then(|m| m.get("codec_type"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            format!("{def}/{codec}")
        })
        .collect();
    format!("version={:?} {}", inner.get("version"), brief.join(","))
}

fn parse_video_model(value: &Value, vid: &str) -> AppResult<Option<Value>> {
    // 新通道单集端点按 video_id 归位；兼容旧通道的 map 形态，两个指针都试
    let model = value
        .pointer(&format!("/data/{vid}/video_model"))
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/data/video_model").and_then(Value::as_str));
    let Some(model) = model else {
        log::info!("[PlayUrl] data 里没有 video_model vid={vid}");
        return Ok(None);
    };
    serde_json::from_str(model)
        .map(Some)
        .map_err(|e| AppError::Media(format!("video_model 解析失败: {e}")))
}

fn select_play(
    model: &Value,
    definition: Option<u32>,
    cap: super::stream_pick::DecoderCapability,
) -> Option<PlayInfo> {
    let stream = pick_stream_at(model, definition, cap)?;
    Some(PlayInfo {
        url: stream.url,
        file_hash: stream.file_hash,
        encrypted: !stream.spade.is_empty(),
        key_material: stream.spade.into_bytes(),
        codec: stream.codec.unwrap_or_default(),
        definition: stream.definition,
        definitions: list_definitions(model, cap),
    })
}

/// AB 打开时的多集路径：[`crate::params::PLAYER_MODEL_PATH`]，`dr_scene` 在 body 里是 `player`。
async fn fallback_player_model(
    map: &BTreeMap<String, Vec<String>>,
    env: &super::client::ApiEnv,
    origin: &str,
) -> AppResult<Vec<u8>> {
    let scene = crate::params::PlayerModelScene::Player;
    let payload = serde_json::to_vec(&crate::params::player_model_payload(map, scene))
        .map_err(|e| AppError::Media(e.to_string()))?;
    crate::client::api_call_model_light(origin, scene.path(), payload, env).await
}

/// 旧通道取流：[`crate::params::MODEL_LIGHT_PATH`]，origin 由调用方传入（生产为 lq 域）。
async fn fallback_model_light(
    map: &BTreeMap<String, Vec<String>>,
    env: &super::client::ApiEnv,
    origin: &str,
) -> AppResult<Vec<u8>> {
    let payload = serde_json::to_vec(&crate::params::model_light_payload(map))
        .map_err(|e| AppError::Media(e.to_string()))?;
    crate::client::api_call_model_light(origin, crate::params::MODEL_LIGHT_PATH, payload, env).await
}

/// 批量取流：起播/切剧时对「当前 + 信息流后续几部」的入口 vid 打一次，命中落暂存表由 [`fetch_play_url`] 取走。
/// 先走 7.4.3.32 默认的 `multi_video_model/v1/`，缺的再试 `/player/v1`，最后才是旧 light 体。
pub async fn fetch_play_batch(
    mixed_video_id_map: &BTreeMap<String, Vec<String>>,
    cap: super::stream_pick::DecoderCapability,
    env: &super::client::ApiEnv,
) -> AppResult<usize> {
    fetch_play_batch_at(mixed_video_id_map, cap, env, default_origins()).await
}

/// Run one observed v5 batch scene with the current device and reuse fresh models.
pub async fn fetch_play_scene(
    mixed_video_id_map: &BTreeMap<String, Vec<String>>,
    scene: crate::params::PlayerModelScene,
    cap: super::stream_pick::DecoderCapability,
    env: &super::client::ApiEnv,
) -> AppResult<usize> {
    let _flight = play_flight().lock().await;
    let pending: BTreeMap<_, Vec<_>> = mixed_video_id_map
        .iter()
        .filter_map(|(ct, vids)| {
            let vids: Vec<_> = vids
                .iter()
                .filter(|vid| !vid.is_empty() && !has_play_cached(vid))
                .cloned()
                .collect();
            (!vids.is_empty()).then(|| (ct.clone(), vids))
        })
        .collect();
    if pending.is_empty() {
        return Ok(0);
    }
    let payload = serde_json::to_vec(&crate::params::player_model_payload(&pending, scene))
        .map_err(|error| AppError::Media(error.to_string()))?;
    let bytes =
        crate::client::api_call_model_light(default_origins().player, scene.path(), payload, env)
            .await?;
    let response = parse_model_response(&bytes)?;
    let vids: Vec<_> = pending.into_values().flatten().collect();
    let models = batch_models(response.as_ref(), &vids, cap);
    let hits = models.len();
    store_models(models);
    Ok(hits)
}

async fn fetch_play_batch_at(
    mixed_video_id_map: &BTreeMap<String, Vec<String>>,
    cap: super::stream_pick::DecoderCapability,
    env: &super::client::ApiEnv,
    origins: PlayOrigins<'_>,
) -> AppResult<usize> {
    let vids: Vec<String> = mixed_video_id_map
        .values()
        .flatten()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect();
    if vids.is_empty() {
        return Ok(0);
    }
    let _flight = play_flight().lock().await;
    // 分组 key 即条目类型：顺手进注册表（单集取流选路要用）
    for (key, group) in mixed_video_id_map {
        if let Ok(ct) = key.parse::<i64>() {
            for vid in group {
                register_content_type(vid, ct);
            }
        }
    }
    // 7.4.3.32 默认批量取流是 `mGetVideoModelRxJava`（无 dr_scene 路径段）。
    // `/player/v1` 只在 AB `video_player_api_dr_scene` 打开时才走，留在下面的回退里。
    let new_payload = serde_json::to_vec(&crate::params::saas_model_payload(mixed_video_id_map))
        .map_err(|e| AppError::Media(e.to_string()))?;
    let primary_value = match crate::client::api_call_model_light(
        origins.saas,
        crate::params::MODEL_LIGHT_PATH,
        new_payload,
        env,
    )
    .await
    {
        Ok(bytes) => match parse_model_response(&bytes) {
            Ok(value) => value,
            Err(e) => {
                log::warn!("[PlayUrl] 默认批量响应不可用（{e}），继续回退");
                None
            }
        },
        Err(e) => {
            log::warn!("[PlayUrl] 默认批量请求失败（{e}），继续回退");
            None
        }
    };
    let mut models = batch_models(primary_value.as_ref(), &vids, cap);
    // `/player/v1` 留在主域：7.4.3.32 只在 AB `video_player_api_dr_scene` 打开、
    // 且 drScene 为 player 时才走这条。
    let missing = missing_groups(mixed_video_id_map, &models);
    if !missing.is_empty() {
        log::info!(
            "[PlayUrl] 默认批量 {}/{} vid 无可播放流，改走 /player/v1",
            missing.values().map(Vec::len).sum::<usize>(),
            vids.len()
        );
        match fallback_player_model(&missing, env, origins.player)
            .await
            .and_then(|bytes| parse_model_response(&bytes))
        {
            Ok(value) => {
                let retry: Vec<String> = missing.values().flatten().cloned().collect();
                models.extend(batch_models(value.as_ref(), &retry, cap));
            }
            Err(e) => log::warn!("[PlayUrl] /player/v1 回退失败: {e}"),
        }
    }
    let missing = missing_groups(mixed_video_id_map, &models);
    if !missing.is_empty() {
        match fallback_model_light(&missing, env, origins.light)
            .await
            .and_then(|bytes| parse_model_response(&bytes))
        {
            Ok(value) => {
                let retry: Vec<String> = missing.values().flatten().cloned().collect();
                models.extend(batch_models(value.as_ref(), &retry, cap));
            }
            Err(e) if models.is_empty() => return Err(e),
            Err(e) => log::warn!("[PlayUrl] light 体回退失败，保留已命中流: {e}"),
        }
    }
    let hits = models.len();
    store_models(models);
    log::info!("[PlayUrl] 批量取流 {}/{} vid 落暂存表", hits, vids.len());
    Ok(hits)
}

fn missing_groups(
    map: &BTreeMap<String, Vec<String>>,
    models: &HashMap<String, Value>,
) -> BTreeMap<String, Vec<String>> {
    map.iter()
        .filter_map(|(ct, group)| {
            let missing: Vec<String> = group
                .iter()
                .map(|vid| vid.trim())
                .filter(|vid| !vid.is_empty() && !models.contains_key(*vid))
                .map(str::to_string)
                .collect();
            (!missing.is_empty()).then(|| (ct.clone(), missing))
        })
        .collect()
}

fn batch_models(
    value: Option<&Value>,
    vids: &[String],
    cap: super::stream_pick::DecoderCapability,
) -> HashMap<String, Value> {
    let Some(value) = value else {
        return HashMap::new();
    };
    vids.iter()
        .filter_map(|vid| match parse_video_model(value, vid) {
            Ok(Some(model)) if pick_stream_at(&model, None, cap).is_some() => {
                Some((vid.clone(), model))
            }
            Ok(_) => None,
            Err(e) => {
                log::warn!("[PlayUrl] 批量条目 {vid} 解析失败，跳过: {e}");
                None
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "play_url_tests.rs"]
mod tests;

#[cfg(test)]
mod probes {
    use hongguo_common::model::ProxyConfig;
    use hongguo_signer::device::DeviceProfile;

    /// 注册→取流全链路活体探针（注册口径升级后的必跑闸门）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_register_then_play_live() {
        let reg_env = crate::client::ApiEnv::anonymous(ProxyConfig::default());
        let reg = match crate::register::register_device(&reg_env).await {
            Ok(r) => r,
            Err(e) => {
                println!("[reg-play] register ERR {e}");
                return;
            }
        };
        println!(
            "[reg-play] registered device_id={} iid={} new_user={} 机型={}（{}）",
            reg.device_id,
            reg.install_id,
            reg.new_user,
            hongguo_signer::device::fingerprint_for_cdid(&reg.cdid).display,
            hongguo_signer::device::fingerprint_for_cdid(&reg.cdid).brand
        );

        let mut device = DeviceProfile::fresh_register_profile(&reg.cdid, &reg.openudid);
        device.set("device_id", &reg.device_id);
        device.set("iid", &reg.install_id);
        device.set_ttreq(Some(reg.ttreq.clone()));
        let env = crate::client::ApiEnv {
            proxy: ProxyConfig::default(),
            cookie: Some(hongguo_signer::device::anonymous_cookie(&device)),
            x_tt_token: None,
            device,
        };

        let cap = crate::stream_pick::DecoderCapability::full().with_bytevc2(true);
        let map = std::collections::BTreeMap::from([(
            "1004".to_string(),
            vec![
                "7703109289593818136".to_string(),
                "7703109289329576984".to_string(),
            ],
        )]);
        match super::fetch_play_batch(&map, cap, &env).await {
            Ok(n) => println!("[reg-play] batch hits={n}"),
            Err(e) => println!("[reg-play] batch ERR {e}"),
        }
        for vid in ["7703109289593818136", "7703109289329576984"] {
            match super::fetch_play_url(vid, None, cap, &env).await {
                Ok(play) => println!(
                    "[reg-play] {vid} OK 定义={} codec={}",
                    play.definition, play.codec
                ),
                Err(e) => println!("[reg-play] {vid} ERR {e}"),
            }
        }
    }

    /// 只打新通道，落盘一份 bytevc2 样本。解码验证走内置 Rust 路径，不调用 ffmpeg。
    /// `HONGGUO_PROBE_DEVICE_JSON` 指向已注册档案 JSON 时复用，避免白烧设备。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_bytevc2_sample_live() {
        let env = match probe_env_from_file_or_register().await {
            Ok(e) => e,
            Err(e) => {
                println!("[bytevc2] env ERR {e}");
                return;
            }
        };
        let home = "7703109289593818136";
        let payload = serde_json::to_vec(&crate::params::single_model_payload(home, 1004)).unwrap();
        let bytes = match crate::client::api_call_model_light(
            crate::client::SINFONLINEA_API_ORIGIN,
            crate::params::SINGLE_MODEL_PATH,
            payload,
            &env,
        )
        .await
        {
            Ok(b) => b,
            Err(e) => {
                println!("[bytevc2] single ERR {e}");
                return;
            }
        };
        println!("[bytevc2] single bytes={}", bytes.len());
        if bytes.is_empty() {
            return;
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
        let model = value
            .pointer("/data/video_model")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let Ok(inner) = serde_json::from_str::<serde_json::Value>(model) else {
            println!("[bytevc2] model 不是字符串");
            return;
        };
        let Some(list) = inner.get("video_list").and_then(|v| v.as_array()) else {
            println!("[bytevc2] no video_list");
            return;
        };
        let mut picked: Option<(String, String)> = None;
        for item in list {
            let meta = item.get("video_meta");
            let def = meta
                .and_then(|m| m.get("definition"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let codec = meta
                .and_then(|m| m.get("codec_type"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let url = item.get("main_url").and_then(|u| u.as_str()).unwrap_or("");
            let spade = item
                .pointer("/encrypt_info/spade_a")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            println!(
                "[bytevc2] def={def} codec={codec} url_len={} spade_len={}",
                url.len(),
                spade.len()
            );
            if codec == "bytevc2" && picked.is_none() && !url.is_empty() {
                picked = Some((url.to_string(), spade.to_string()));
            }
        }
        let Some((url, spade)) = picked else {
            println!("[bytevc2] 没有 bytevc2 档");
            return;
        };
        let client = reqwest::Client::new();
        let sample = match client.get(&url).send().await {
            Ok(r) => r.bytes().await.unwrap_or_default(),
            Err(e) => {
                println!("[bytevc2] download ERR {e}");
                return;
            }
        };
        let path = "/tmp/hg-bytevc2-sample.mp4";
        std::fs::write(path, &sample).unwrap();
        println!("[bytevc2] wrote {path} bytes={}", sample.len());
        if !spade.is_empty() {
            match hongguo_video::crypto::key_derive::derive_key(spade.as_bytes()) {
                Ok(key) => match hongguo_video::decrypt_buffer::decrypt_mp4_buffer(&sample, &key) {
                    Ok(plain) => {
                        let plain_path = "/tmp/hg-bytevc2-plain.mp4";
                        std::fs::write(plain_path, &plain).unwrap();
                        println!("[bytevc2] decrypted {plain_path} bytes={}", plain.len());
                        match hongguo_bytevc2::decode_plaintext_mp4(
                            std::path::Path::new(plain_path),
                            |_| Ok(()),
                        ) {
                            Ok(fps) => println!("[bytevc2] rust decode ok fps={fps}"),
                            Err(e) => println!("[bytevc2] rust decode ERR {e}"),
                        }
                    }
                    Err(e) => println!("[bytevc2] decrypt ERR {e}"),
                },
                Err(e) => println!("[bytevc2] key ERR {e}"),
            }
        }
    }

    async fn probe_env_from_file_or_register()
    -> Result<crate::client::ApiEnv, Box<dyn std::error::Error>> {
        if let Some(path) = crate::register::reuse_json_path() {
            let device = crate::register::load_reuse_profile(&path)?;
            return Ok(crate::client::ApiEnv {
                proxy: ProxyConfig::default(),
                cookie: Some(hongguo_signer::device::anonymous_cookie(&device)),
                x_tt_token: None,
                device,
            });
        }
        if let Ok(path) = std::env::var("HONGGUO_PROBE_DEVICE_JSON") {
            let raw = std::fs::read_to_string(path)?;
            let device: DeviceProfile = serde_json::from_str(&raw)?;
            return Ok(crate::client::ApiEnv {
                proxy: ProxyConfig::default(),
                cookie: Some(hongguo_signer::device::anonymous_cookie(&device)),
                x_tt_token: None,
                device,
            });
        }
        let reg_env = crate::client::ApiEnv::anonymous(ProxyConfig::default());
        let reg = crate::register::register_device(&reg_env).await?;
        let mut device = DeviceProfile::fresh_register_profile(&reg.cdid, &reg.openudid);
        device.set("device_id", &reg.device_id);
        device.set("iid", &reg.install_id);
        device.set_ttreq(Some(reg.ttreq));
        Ok(crate::client::ApiEnv {
            proxy: ProxyConfig::default(),
            cookie: Some(hongguo_signer::device::anonymous_cookie(&device)),
            x_tt_token: None,
            device,
        })
    }
}
