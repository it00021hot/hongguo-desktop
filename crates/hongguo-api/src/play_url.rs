//! 取流地址：平台把每集的可用流放在 `video_model`（**JSON 字符串**）里二次解析，
//! 挑哪一条由 [`super::stream_pick`] 决定；批量/单集同端点，见 [`fetch_play_url`]。
//! ⚠️ 服务端不发货时返回 **HTTP 200 + 0 字节**——排查「接口没数据」先看字节数，不是状态码。

use std::collections::HashMap;
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
    match fetch_model_light_batch(vid, ct, definition, cap, env).await {
        Ok(Some(play)) => {
            log::info!(
                "[PlayUrl] {vid} 轻请求取流命中（ct={ct}，定义 {}）",
                play.definition
            );
            Ok(play)
        }
        // 服务端没对这个设备发货（200+0 或响应里没有这一集）：设备侧信任问题，不做身份层面兜底
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

/// 轻请求批量取流（单 vid）：新通道优先，空响应/网络失败回退旧通道；
/// 两次都空才算未命中（返回 `None`）。
async fn fetch_model_light_batch(
    vid: &str,
    content_type: i64,
    definition: Option<u32>,
    cap: super::stream_pick::DecoderCapability,
    env: &super::client::ApiEnv,
) -> AppResult<Option<PlayInfo>> {
    let map = std::collections::BTreeMap::from([(content_type.to_string(), vec![vid.to_string()])]);
    let new_payload = serde_json::to_vec(&crate::params::single_model_payload(content_type, vid))
        .map_err(|e| AppError::Media(e.to_string()))?;
    // 新通道一次即回：空响应不重试（重试只会白等），直接回退
    let bytes = match crate::client::api_call_model_light(
        crate::client::READING_API_ORIGIN,
        crate::params::SINGLE_MODEL_PATH,
        new_payload,
        env,
    )
    .await
    {
        Ok(b) if !b.is_empty() => b,
        Ok(_) => {
            log::info!("[PlayUrl] 单集新通道空响应，回退旧通道");
            fallback_model_light(&map, env).await?
        }
        Err(e) => {
            log::warn!("[PlayUrl] 单集新通道请求失败（{e}），回退旧通道");
            fallback_model_light(&map, env).await?
        }
    };
    if bytes.is_empty() {
        return Ok(None);
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析取流响应失败: {e}")))?;
    check_code(&value)?;
    check_model_code(&value)?;
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

/// 旧通道（竞品形态）取流：lq 域 + [`crate::params::MODEL_LIGHT_PATH`]，作为新通道回退保留。
async fn fallback_model_light(
    map: &std::collections::BTreeMap<String, Vec<String>>,
    env: &super::client::ApiEnv,
) -> AppResult<Vec<u8>> {
    let payload = serde_json::to_vec(&crate::params::model_light_payload(map))
        .map_err(|e| AppError::Media(e.to_string()))?;
    crate::client::api_call_model_light(
        crate::client::LQ_API_ORIGIN,
        crate::params::MODEL_LIGHT_PATH,
        payload,
        env,
    )
    .await
}

/// 批量取流：起播/切剧时对「当前 + 信息流后续几部」的入口 vid 打一次，命中落暂存表由 [`fetch_play_url`] 取走。
/// 新通道优先、空响应/失败回退旧通道；`mixed_video_id_map` 按条目 content_type 原样字符串分组（错配不发货）；空响应静默跳过。
pub async fn fetch_play_batch(
    mixed_video_id_map: &std::collections::BTreeMap<String, Vec<String>>,
    cap: super::stream_pick::DecoderCapability,
    env: &super::client::ApiEnv,
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
    let new_payload =
        serde_json::to_vec(&crate::params::player_model_payload(mixed_video_id_map))
            .map_err(|e| AppError::Media(e.to_string()))?;
    let bytes = match crate::client::api_call_model_light(
        crate::client::READING_API_ORIGIN,
        crate::params::PLAYER_MODEL_PATH,
        new_payload,
        env,
    )
    .await
    {
        Ok(b) if !b.is_empty() => b,
        Ok(_) => {
            log::info!("[PlayUrl] 批量新通道空响应，回退旧通道");
            fallback_model_light(mixed_video_id_map, env).await?
        }
        Err(e) => {
            log::warn!("[PlayUrl] 批量新通道请求失败（{e}），回退旧通道");
            fallback_model_light(mixed_video_id_map, env).await?
        }
    };
    if bytes.is_empty() {
        log::info!("[PlayUrl] 批量取流空响应（{} vid）", vids.len());
        return Ok(0);
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析批量取流响应失败: {e}")))?;
    check_code(&value)?;
    check_model_code(&value)?;
    let mut hits = 0;
    {
        let mut cache = batch_cache().lock();
        for vid in &vids {
            let Some(model) = value
                .pointer(&format!("/data/{vid}/video_model"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let Ok(parsed) = serde_json::from_str::<Value>(model) else {
                continue;
            };
            let Some(stream) = pick_stream_at(&parsed, None, cap) else {
                continue;
            };
            cache.insert(
                vid.clone(),
                (
                    Instant::now(),
                    PlayInfo {
                        url: stream.url,
                        encrypted: !stream.spade.is_empty(),
                        key_material: stream.spade.clone().into_bytes(),
                        codec: stream.codec.unwrap_or_default(),
                        definition: stream.definition,
                        definitions: list_definitions(&parsed, cap),
                    },
                ),
            );
            hits += 1;
        }
    }
    log::info!("[PlayUrl] 批量取流 {}/{} vid 落暂存表", hits, vids.len());
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_roundtrip_and_zero_skip() {
        register_content_type("v1", 1004);
        register_content_type("", 1);
        register_content_type("v-zero", 0);
        assert_eq!(content_type_of("v1"), Some(1004));
        assert_eq!(content_type_of(""), None);
        assert_eq!(content_type_of("v-zero"), None);
        assert_eq!(content_type_of("未注册"), None);
        register_content_type("v1", 1);
        assert_eq!(content_type_of("v1"), Some(1));
    }

    #[test]
    fn clear_batch_cache_drops_stale_prefetch() {
        batch_cache().lock().insert(
            "vid-old-device".into(),
            (Instant::now(), PlayInfo::default()),
        );
        assert!(batch_cache().lock().contains_key("vid-old-device"));
        clear_batch_cache();
        assert!(
            batch_cache().lock().is_empty(),
            "换档后旧设备预取必须清空"
        );
    }

    #[test]
    fn model_error_envelope_surfaces_with_server_message() {
        let v: Value = serde_json::from_str(
            r#"{"Code":110001,"Message":"未知异常","BaseResp":{"StatusCode":110001,"StatusMessage":"unknown error"}}"#,
        )
        .unwrap();
        let err = check_model_code(&v).unwrap_err().to_string();
        assert!(err.contains("110001"), "{err}");
        assert!(err.contains("unknown error"), "{err}");
    }

    #[test]
    fn model_success_envelope_without_code_passes() {
        check_model_code(&serde_json::json!({ "data": {} })).unwrap();
        check_model_code(&serde_json::json!({ "Code": 0, "data": {} })).unwrap();
    }
}

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
