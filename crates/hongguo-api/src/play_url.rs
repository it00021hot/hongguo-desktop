//! 取流地址：平台把每集的可用流放在 `video_model`（**JSON 字符串**）里二次解析，
//! 挑哪一条由 [`super::stream_pick`] 决定；批量/单集同端点，见 [`fetch_play_url`]。
//! ⚠️ 服务端不发货时返回 **HTTP 200 + 0 字节**——排查「接口没数据」先看字节数，不是状态码。

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::Value;

use super::stream_pick::{Definition, list_definitions, pick_stream_at};
use hongguo_common::utils::json::check_code;
use hongguo_common::{AppError, AppResult};

/// 批量取流结果的暂存表：`fetch_play_batch` 命中时落这里，
/// [`fetch_play_url`] 使用时取走（pop + TTL 双保险——直链有时效）。
static BATCH_CACHE: OnceLock<Mutex<HashMap<String, (Instant, PlayInfo)>>> = OnceLock::new();
/// 批量结果有效期：响应自带 expire_time（小时级），取保守 10 分钟。
const BATCH_TTL: Duration = Duration::from_secs(600);

fn batch_cache() -> &'static Mutex<HashMap<String, (Instant, PlayInfo)>> {
    BATCH_CACHE.get_or_init(Mutex::default)
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

/// `(新通道 origin, 旧通道 origin)`；生产固定 reading → lq，测试可注入本地 mock。
type PlayOrigins<'a> = (&'a str, &'a str);

fn default_origins() -> PlayOrigins<'static> {
    (
        crate::client::READING_API_ORIGIN,
        crate::client::LQ_API_ORIGIN,
    )
}

/// 取一集的播放地址：批量暂存命中直接用；`definition` 缺档自动回退最高档（[`pick_stream_at`]）；
/// `cap` 决定选流模式（在线传本机探测结论，转码/下载传 `Full`）。content_type 未注册报 [`AppError::ContentTypeUnknown`]。
pub async fn fetch_play_url(
    vid: &str,
    definition: Option<u32>,
    cap: super::stream_pick::DecoderCapability,
    env: &super::client::ApiEnv,
) -> AppResult<PlayInfo> {
    if let Some((at, play)) = batch_cache().lock().remove(vid)
        && at.elapsed() < BATCH_TTL
    {
        log::info!(
            "[PlayUrl] {vid} 命中批量预取（定义 {}，编码 {}）",
            play.definition,
            if play.codec.is_empty() {
                "未标"
            } else {
                &play.codec
            }
        );
        return Ok(play);
    }
    batch_cache().lock().remove(vid);

    let Some(ct) = content_type_of(vid) else {
        return Err(AppError::ContentTypeUnknown(format!(
            "vid={vid} 的条目类型未注册（先拉分集档案后再取流）"
        )));
    };
    match fetch_single_at(vid, ct, definition, cap, env, default_origins()).await {
        Ok(Some(play)) => {
            log::info!(
                "[PlayUrl] {vid} 轻请求取流命中（ct={ct}，定义 {}）",
                play.definition
            );
            Ok(play)
        }
        // 两个通道都没有可用流；仅凭此结果不能判定设备身份失效。
        Ok(None) => Err(AppError::StreamMiss(format!(
            "取流未命中 vid={vid}（ct={ct}）"
        ))),
        Err(e) => Err(e),
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

/// 新通道给出可播放流才算命中；空响应、请求失败、响应不可用或无可播放流时回退旧通道。
async fn fetch_single_at(
    vid: &str,
    content_type: i64,
    definition: Option<u32>,
    cap: super::stream_pick::DecoderCapability,
    env: &super::client::ApiEnv,
    (primary, fallback): PlayOrigins<'_>,
) -> AppResult<Option<PlayInfo>> {
    let new_payload = serde_json::to_vec(&crate::params::single_model_payload(content_type, vid))
        .map_err(|e| AppError::Media(e.to_string()))?;
    match crate::client::api_call_model_light(
        primary,
        crate::params::SINGLE_MODEL_PATH,
        new_payload,
        env,
    )
    .await
    {
        Ok(bytes) => match play_from_bytes(&bytes, vid, definition, cap) {
            Ok(Some(play)) => return Ok(Some(play)),
            Ok(None) => {
                log::info!("[PlayUrl] 单集新通道无可播放流，回退旧通道（vid={vid}）");
            }
            Err(e) => {
                log::warn!("[PlayUrl] 单集新通道响应不可用（{e}），回退旧通道");
            }
        },
        Err(e) => {
            log::warn!("[PlayUrl] 单集新通道请求失败（{e}），回退旧通道");
        }
    }
    let map = BTreeMap::from([(content_type.to_string(), vec![vid.to_string()])]);
    let bytes = fallback_model_light(&map, env, fallback).await?;
    play_from_bytes(&bytes, vid, definition, cap)
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
) -> AppResult<Option<PlayInfo>> {
    let Some(value) = parse_model_response(bytes)? else {
        return Ok(None);
    };
    play_info(&value, vid, definition, cap)
}

fn play_info(
    value: &Value,
    vid: &str,
    definition: Option<u32>,
    cap: super::stream_pick::DecoderCapability,
) -> AppResult<Option<PlayInfo>> {
    // 新通道单集端点按 video_id 归位；兼容旧通道的 map 形态，两个指针都试
    let model = value
        .pointer(&format!("/data/{vid}/video_model"))
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/data/video_model").and_then(Value::as_str));
    let Some(model) = model else {
        return Ok(None);
    };
    let parsed: Value = serde_json::from_str(model)
        .map_err(|e| AppError::Media(format!("video_model 解析失败: {e}")))?;
    let Some(stream) = pick_stream_at(&parsed, definition, cap) else {
        return Ok(None);
    };
    Ok(Some(PlayInfo {
        url: stream.url,
        encrypted: !stream.spade.is_empty(),
        key_material: stream.spade.into_bytes(),
        codec: stream.codec.unwrap_or_default(),
        definition: stream.definition,
        definitions: list_definitions(&parsed, cap),
    }))
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
/// 新通道中缺失或不可播的条目单独回退旧通道，保留已命中的流。
pub async fn fetch_play_batch(
    mixed_video_id_map: &BTreeMap<String, Vec<String>>,
    cap: super::stream_pick::DecoderCapability,
    env: &super::client::ApiEnv,
) -> AppResult<usize> {
    fetch_play_batch_at(mixed_video_id_map, cap, env, default_origins()).await
}

async fn fetch_play_batch_at(
    mixed_video_id_map: &BTreeMap<String, Vec<String>>,
    cap: super::stream_pick::DecoderCapability,
    env: &super::client::ApiEnv,
    (primary, fallback): PlayOrigins<'_>,
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
    // 分组 key 即条目类型：顺手进注册表（单集取流选路要用）
    for (key, group) in mixed_video_id_map {
        if let Ok(ct) = key.parse::<i64>() {
            for vid in group {
                register_content_type(vid, ct);
            }
        }
    }
    let new_payload = serde_json::to_vec(&crate::params::player_model_payload(mixed_video_id_map))
        .map_err(|e| AppError::Media(e.to_string()))?;
    let primary_value = match crate::client::api_call_model_light(
        primary,
        crate::params::PLAYER_MODEL_PATH,
        new_payload,
        env,
    )
    .await
    {
        Ok(bytes) => match parse_model_response(&bytes) {
            Ok(value) => value,
            Err(e) => {
                log::warn!("[PlayUrl] 批量新通道响应不可用（{e}），回退旧通道");
                None
            }
        },
        Err(e) => {
            log::warn!("[PlayUrl] 批量新通道请求失败（{e}），回退旧通道");
            None
        }
    };
    let mut plays = batch_plays(primary_value.as_ref(), &vids, cap);
    let missing: BTreeMap<String, Vec<String>> = mixed_video_id_map
        .iter()
        .filter_map(|(ct, group)| {
            let missing: Vec<String> = group
                .iter()
                .filter(|vid| !plays.contains_key(vid.trim()))
                .map(|vid| vid.trim().to_string())
                .filter(|vid| !vid.is_empty())
                .collect();
            (!missing.is_empty()).then(|| (ct.clone(), missing))
        })
        .collect();
    if !missing.is_empty() {
        log::info!(
            "[PlayUrl] 批量新通道 {}/{} vid 无可播放流，回退旧通道",
            missing.values().map(Vec::len).sum::<usize>(),
            vids.len()
        );
        match fallback_model_light(&missing, env, fallback)
            .await
            .and_then(|bytes| parse_model_response(&bytes))
        {
            Ok(value) => {
                let missing_vids: Vec<String> = missing.values().flatten().cloned().collect();
                plays.extend(batch_plays(value.as_ref(), &missing_vids, cap));
            }
            Err(e) if plays.is_empty() => return Err(e),
            Err(e) => log::warn!("[PlayUrl] 批量回退失败，保留已命中流: {e}"),
        }
    }
    let hits = plays.len();
    {
        let mut cache = batch_cache().lock();
        for (vid, play) in plays {
            cache.insert(vid, (Instant::now(), play));
        }
    }
    log::info!("[PlayUrl] 批量取流 {}/{} vid 落暂存表", hits, vids.len());
    Ok(hits)
}

fn batch_plays(
    value: Option<&Value>,
    vids: &[String],
    cap: super::stream_pick::DecoderCapability,
) -> HashMap<String, PlayInfo> {
    let Some(value) = value else {
        return HashMap::new();
    };
    vids.iter()
        .filter_map(|vid| match play_info(value, vid, None, cap) {
            Ok(Some(play)) => Some((vid.clone(), play)),
            Ok(None) => None,
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
    /// 注册→取流全链路活体探针（注册口径升级后的必跑闸门）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_register_then_play_live() {
        use hongguo_common::model::ProxyConfig;

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

        let mut device =
            hongguo_signer::device::DeviceProfile::fresh_register_profile(&reg.cdid, &reg.openudid);
        device.set("device_id", &reg.device_id);
        device.set("iid", &reg.install_id);
        device.set_ttreq(Some(reg.ttreq.clone()));
        let env = crate::client::ApiEnv {
            proxy: ProxyConfig::default(),
            cookie: Some(hongguo_signer::device::anonymous_cookie(&device)),
            x_tt_token: None,
            device,
        };

        let vid = "7677936624126151705";
        super::register_content_type(vid, 1);
        match super::fetch_play_url(vid, None, crate::stream_pick::DecoderCapability::Full, &env)
            .await
        {
            Ok(play) => println!(
                "[reg-play] play OK 定义={} 加密={} url_len={} -> STREAM_OK",
                play.definition,
                play.encrypted,
                play.url.len()
            ),
            Err(e) => println!("[reg-play] play ERR {e}"),
        }
    }
}
