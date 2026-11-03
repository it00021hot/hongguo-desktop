//! 取流地址。
//!
//! 平台把每集的可用流放在 `video_model`（一个 **JSON 字符串**）里，需要二次解析；
//! 挑哪一条由 [`super::stream_pick`] 决定。取流走**无签名轻请求**
//! `multi_video_model/v1/`（2026-11-01 竞品实流抓包对齐；旧的
//! `multi_video_model/player/v1` 与单集 `video_model/v1/` 已整体回
//! 200+0），批量/单集同端点、只差 `mixed_video_id_map` 里几组 vid，
//! 见 [`fetch_play_url`] 注释。
//!
//! ⚠️ 服务端不发货时返回 **HTTP 200 + 0 字节**，不是错误码。
//!    排查「接口没数据」时第一件事是看字节数，不是状态码。

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

/// vid → content_type 注册表（服务端下发的档案元数据，进程内缓存）。
///
/// 批量取流端点的 `mixed_video_id_map` 分组 key 必须与条目 content_type
/// 一致（错配 → 空响应），注册来源：
/// - 推荐流解析（入口 vid，[`crate::recommend`]）；
/// - 分集档案解析（整剧各集，[`crate::detail`]，档案 `video_data.content_type`）；
/// - 批量取流分组（[`fetch_play_batch`] 的 map key）。
///
/// 未注册的 vid 不猜 key，直接走单集回落（见 [`fetch_play_url`]）。
static CONTENT_TYPES: OnceLock<Mutex<HashMap<String, i64>>> = OnceLock::new();

fn content_types() -> &'static Mutex<HashMap<String, i64>> {
    CONTENT_TYPES.get_or_init(Mutex::default)
}

/// 记录一个 vid 的条目类型（重复注册后值覆盖，服务端口径为准）。
///
/// 除 crate 内的自动注册点（推荐流/档案解析/批量预取）外，壳在起播时
/// 也应把**整剧各集**注册进去（feed 条目带类型、分集表在手）——连播的
/// 后续集只有这里能提前拿到类型，否则走单集回落。
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

/// 取一集的播放地址。
///
/// `definition` 为 `Some` 时取该档位；平台没提供这一档时自动回退到最高档
/// （见 [`pick_stream_at`]），不会因此播放失败。
///
/// `cap` 决定选流模式：在线播放传本机探测结论（无 HEVC 扩展的机器自动
/// 翻转到 h264）；转码源/下载选流传 `Full` 保持全量候选。
///
/// 取一集的播放地址——取流链（2026-11-01 实流形态）：
///
/// 1. 批量取流暂存命中（[`fetch_play_batch`] 落的）直接用，不发请求；
/// 2. 轻请求批量端点带这一集打一次——分组 key 用注册表里的
///    content_type（见 [`register_content_type`]；类型未知时**不猜**，
///    报 [`AppError::ContentTypeUnknown`] 交调用方先拉分集档案补注册）。
///
/// `definition` 为 `Some` 时取该档位；平台没提供这一档时自动回退到最高档
/// （见 [`pick_stream_at`]），不会因此播放失败。`cap` 决定选流模式：在线
/// 播放传本机探测结论（无 HEVC 扩展的机器自动翻转 h264）；转码源/下载
/// 选流传 `Full` 保持全量候选。
pub async fn fetch_play_url(
    vid: &str,
    definition: Option<u32>,
    cap: super::stream_pick::DecoderCapability,
    env: &super::client::ApiEnv,
) -> AppResult<PlayInfo> {
    // 1) 批量预取命中（批量给的直接用，不发请求）
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

    // 2) 轻请求批量端点（key = 注册表的 content_type）。类型未知**不猜**：
    //    编一个 ct 打过去即使偶然命中也是纯运气，报
    //    [`AppError::ContentTypeUnknown`] 交调用方先拉分集档案补注册。
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
        // 服务端没对这个设备发货（200+0 或响应里没有这一集）。这是设备
        // 侧信任问题，交给上层如实报错——不做任何身份层面的兜底。
        Ok(None) => Err(AppError::StreamMiss(format!(
            "取流未命中 vid={vid}（ct={ct}）"
        ))),
        Err(e) => Err(e),
    }
}

/// 取流端点的错误信封是 **base 族**（`Code` 大写 + `BaseResp`），不是
/// reading 系的小写 `code`——[`check_code`] 认不得它，服务端的显式报错会
/// 被吞成「取流未命中」，把排障引向「设备信任」死路。实测样本（2026-11-02
/// 端点收紧，各身份统一回，125 字节）：`{"Code":110001,"Message":"未知异常",
/// "BaseResp":{"StatusCode":110001,"StatusMessage":"unknown error"}}`。
/// 语义上这就是「服务端对这个设备/请求不发货」，归 [`AppError::StreamMiss`]
/// ——取流健康守护（device::note_stream_miss）据此换新注册自愈，UI 侧
/// 透传服务端原文。
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

/// 轻请求批量取流：单 vid 按 content_type 走
/// 新通道（官方 74232 形态：`reading.snssdk.com + video_model/v1/`，
/// 官方 body 逐字段）优先，空响应/网络失败回退旧通道
/// （`api5-normal-lq + MODEL_LIGHT_PATH`，2026-11-01 竞品形态，同探针
/// 实测仍发货）。两次都空才算未命中（返回 `None`，交调用方报
/// 「取流未命中」）。
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

/// 旧通道（竞品形态）取流：lq 域 + [`crate::params::MODEL_LIGHT_PATH`]。
/// 2026-11-03 同窗探针实测仍发货，作为新通道的回退保留。
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

/// 批量取流：起播/切剧时对「当前 + 信息流后续几部」（官方 6 个）的入口
/// vid 打一次取流接口。命中落暂存表，[`fetch_play_url`] 使用时取走，
/// 不发单集请求。
///
/// 新通道优先（官方 74232 实流形态：`reading.snssdk.com +
/// multi_video_model/player/v1`，官方 body 含 `dr_scene:"player"`），
/// 空响应/网络失败回退旧通道（`api5-normal-lq + MODEL_LIGHT_PATH`，
/// 2026-11-01 竞品形态，同探针实测仍发货）。
///
/// `mixed_video_id_map` 按**条目 content_type 原样字符串**分组（key 错配 →
/// 不发货；漫剧 feed ct=1004→"1004"、真人剧 ct=1→"1"）。
/// 空响应时静默跳过（不报错，调用方走单集轻请求）。
///
/// 返回成功暂存的条数。
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
        // 注册→查询回环；vid 空串与类型 0 不收（0 = 未知，不能当 key）
        register_content_type("v1", 1004);
        register_content_type("", 1);
        register_content_type("v-zero", 0);
        assert_eq!(content_type_of("v1"), Some(1004));
        assert_eq!(content_type_of(""), None);
        assert_eq!(content_type_of("v-zero"), None);
        assert_eq!(content_type_of("未注册"), None);
        // 覆盖注册：后值生效
        register_content_type("v1", 1);
        assert_eq!(content_type_of("v1"), Some(1));
    }

    /// base 族错误信封必须如实上抛（带服务端 code/message），不许被吞成
    /// 「取流未命中」——2026-11-02 端点收紧时 `110001 未知异常` 被吞，
    /// 排障被引向「设备信任」死路的实录。回归锁。
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
    /// 注册→取流全链路活体探针：按当前注册口径注册新设备，立刻用产物
    /// 档案打轻请求取流。**注册口径升级（`REGISTER_VERSION_*` 变动）后的
    /// 必跑闸门**——注册身份异常时这里会回 200+0/服务异常。
    /// `cargo test -p hongguo-api --lib probe_register_then_play_live -- --ignored --nocapture`
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

        // 注册产物档案（与 service::registered_profile 同构）：注册指纹 + 身份字段
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

        // 生产链探针：先注册该 vid 的条目类型（7677936624126151705→ct 1
        // 为 2026-11-01 实流验证过的配对），再走 `fetch_play_url` 全链
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
