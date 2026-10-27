//! 取流地址。
//!
//! 平台把每集的可用流放在 `video_model`（一个 **JSON 字符串**）里，需要二次解析；
//! 挑哪一条由 [`super::stream_pick`] 决定。真播链对齐官方 App（2026-10-25
//! 抓 7.3.9.32 实流）：批量 `multi_video_model/player/v1`（dr_scene=player，
//! 按 content_type 分组）优先 → 空/类型未知回落单集 `video_model/v1/`
//! （「批量空 → 回落单集」同链，2026-10-14 抓包实证），
//! 见 [`fetch_play_url`] 注释。
//!
//! ⚠️ 签名失败时服务端返回 **HTTP 200 + 0 字节**，不是错误码。
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
/// 一致（2026-10-25 直连实证：错配 → 空响应），注册来源：
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
/// 取一集的播放地址——真播链对齐官方 App（2026-10-25 抓 7.3.9.32 实流）：
///
/// 1. 批量取流暂存命中（[`fetch_play_batch`] 落的）直接用，不发请求；
/// 2. 批量 `multi_video_model/player/v1`（dr_scene=player，官方真播请求
///    形态）带这一集打一次——分组 key 用注册表里的 content_type（见
///    [`register_content_type`]；**类型未知不猜 key**，直接跳到 3）；
/// 3. 批量空/未命中回落单集 [`crate::params::SINGLE_MODEL_PATH`]——
///    「批量空 → 回落单集」回落链（2026-10-14 抓包实证）。
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

    // 2) 官方真播形态：批量 player（key = 注册表的 content_type）
    if let Some(ct) = content_type_of(vid) {
        match fetch_play_player_batch(vid, ct, definition, cap, env).await {
            Ok(Some(play)) => {
                log::info!(
                    "[PlayUrl] {vid} 真播批量命中（ct={ct}，定义 {}）",
                    play.definition
                );
                return Ok(play);
            }
            Ok(None) => log::info!("[PlayUrl] {vid} 真播批量空，回落单集"),
            Err(e) => log::warn!("[PlayUrl] {vid} 真播批量失败（{e}），回落单集"),
        }
    }

    // 3) 单集回落（官方切集形态：video_model/v1，content_type=剧类型；
    // 类型未知时只能填占位值——服务端对该字段容错，见
    // `SINGLE_CONTENT_TYPE_FALLBACK`）
    let ct = content_type_of(vid).unwrap_or(crate::params::SINGLE_CONTENT_TYPE_FALLBACK);
    log::info!("[PlayUrl] {vid} 单集取流（ct={ct}）");
    let payload = serde_json::to_vec(&crate::params::single_model_payload(vid, ct))
        .map_err(|e| AppError::Signer(e.to_string()))?;

    let bytes = crate::client::api_call_full(
        crate::client::LQ_API_ORIGIN,
        crate::params::SINGLE_MODEL_PATH,
        Some(payload),
        &[],
        env,
    )
    .await?;

    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析响应失败: {e}")))?;

    // 业务错误码：签名对了但参数/内容不对会走这里
    check_code(&value)?;

    // 单集响应：data.video_model 是 JSON 字符串（与批量形态的
    // data[vid].video_model 同构，选流逻辑共用）
    let model = value
        .pointer("/data/video_model")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Media("缺少 video_model".into()))?;

    let parsed: Value = serde_json::from_str(model)
        .map_err(|e| AppError::Media(format!("video_model 解析失败: {e}")))?;

    let stream = pick_stream_at(&parsed, definition, cap)
        .ok_or_else(|| AppError::Media("没有可用的视频流".into()))?;

    Ok(PlayInfo {
        url: stream.url,
        encrypted: !stream.spade.is_empty(),
        key_material: stream.spade.into_bytes(),
        codec: stream.codec.unwrap_or_default(),
        definition: stream.definition,
        definitions: list_definitions(&parsed, cap),
    })
}

/// 真播批量（官方 player 形态）：单 vid 按 content_type 分组打
/// `multi_video_model/player/v1`，命中返回解析结果，空响应返回 `None`
/// （交调用方回落单集）。
async fn fetch_play_player_batch(
    vid: &str,
    content_type: i64,
    definition: Option<u32>,
    cap: super::stream_pick::DecoderCapability,
    env: &super::client::ApiEnv,
) -> AppResult<Option<PlayInfo>> {
    let map = std::collections::BTreeMap::from([(content_type.to_string(), vec![vid.to_string()])]);
    let payload = serde_json::to_vec(&crate::params::player_model_payload(&map))
        .map_err(|e| AppError::Signer(e.to_string()))?;
    // 一次即回：空响应是「回落单集」的正常分支，重试只会白等
    let bytes = crate::client::api_call_once_allow_empty(
        crate::client::LQ_API_ORIGIN,
        crate::params::PLAYER_MODEL_PATH,
        Some(payload),
        &[],
        env,
    )
    .await?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析真播批量响应失败: {e}")))?;
    check_code(&value)?;
    let Some(model) = value
        .pointer(&format!("/data/{vid}/video_model"))
        .and_then(Value::as_str)
    else {
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

/// 批量取流（2026-10-25 抓官方 App 7.3.9.32 对齐）：起播/切剧时对
/// 「当前 + 信息流后续几部」（官方 6 个）的入口 vid 打一次
/// `multi_video_model/player/v1`（dr_scene=player，官方起播批量形态）。
/// 命中落暂存表，[`fetch_play_url`] 使用时取走，不发单集请求。
///
/// `mixed_video_id_map` 按**条目 content_type 原样字符串**分组（直连 probe
/// 实证：key 错配 → 空响应；漫剧 feed ct=1004→"1004"、真人剧 ct=1→"1"）。
/// 空响应时静默跳过（不报错，调用方回落单集）。
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
    // 分组 key 即条目类型：顺手进注册表（真播批量/单集选路要用）
    for (key, group) in mixed_video_id_map {
        if let Ok(ct) = key.parse::<i64>() {
            for vid in group {
                register_content_type(vid, ct);
            }
        }
    }
    let payload = serde_json::to_vec(&crate::params::player_model_payload(mixed_video_id_map))
        .map_err(|e| AppError::Signer(e.to_string()))?;
    let bytes = crate::client::api_call_once_allow_empty(
        crate::client::LQ_API_ORIGIN,
        crate::params::PLAYER_MODEL_PATH,
        Some(payload),
        &[],
        env,
    )
    .await?;
    if bytes.is_empty() {
        log::info!("[PlayUrl] 批量取流空响应（{} vid），回落单集", vids.len());
        return Ok(0);
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析批量取流响应失败: {e}")))?;
    check_code(&value)?;
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
}

#[cfg(test)]
mod probes {
    /// 匿名取流的三域探测（官方 7.4.2.32 virgin 打 reading.snssdk.com，
    /// 本仓历史口径打 lq 域——lq 对匿名 POST 疑似拒绝，见 2026-10-26 冒烟）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_video_model_hosts() {
        use hongguo_common::model::ProxyConfig;
        let device = hongguo_signer::anonymous_profile(&hongguo_signer::fresh_cdid());
        let env = crate::client::ApiEnv {
            // 走本机 mitm 便于与官方 virgin 请求逐字段对照
            proxy: ProxyConfig {
                mode: hongguo_common::model::settings::ProxyMode::Manual,
                url: "http://127.0.0.1:8080".into(),
            },
            device,
            cookie: None,
            x_tt_token: None,
        };
        let vid = "7690617619568675902"; // 2026-10-26 官方 virgin 实流
        let ct = crate::params::SINGLE_CONTENT_TYPE_FALLBACK;
        let payload = serde_json::to_vec(&crate::params::single_model_payload(vid, ct))
            .expect("payload 序列化不可能失败");
        for origin in [
            crate::client::LQ_API_ORIGIN,
            "https://api5-normal.fqnovel.com",
            "https://reading.snssdk.com",
        ] {
            match crate::client::api_call_full(
                origin,
                crate::params::SINGLE_MODEL_PATH,
                Some(payload.clone()),
                &[],
                &env,
            )
            .await
            {
                Ok(b) => println!(
                    "[vm-hosts] {origin} bytes={} head={}",
                    b.len(),
                    &String::from_utf8_lossy(&b)[..80.min(b.len())]
                ),
                Err(e) => println!("[vm-hosts] {origin} ERR {e}"),
            }
        }

        // 官方 virgin 形态：query 只有 8 个版本参数 + cdid（无指纹堆）
        let minimal = hongguo_signer::device::DeviceProfile::from_pairs(
            &[
                ("aid", "8662"),
                ("device_platform", "android"),
                ("update_version_code", hongguo_signer::APP_VERSION_CODE_STR),
                (
                    "manifest_version_code",
                    hongguo_signer::APP_VERSION_CODE_STR,
                ),
                ("version_name", hongguo_signer::APP_VERSION_NAME),
                ("version_code", hongguo_signer::APP_VERSION_CODE_STR),
                ("app_name", "novelread"),
                ("channel", "tengxun_8662_64"),
            ],
            &hongguo_signer::video_ua(),
        );
        let env_min = crate::client::ApiEnv {
            proxy: ProxyConfig::default(),
            device: minimal,
            cookie: None,
            x_tt_token: None,
        };
        for origin in [
            "https://reading.snssdk.com",
            crate::client::LQ_API_ORIGIN,
            "https://api5-normal.fqnovel.com",
        ] {
            match crate::client::api_call_full(
                origin,
                crate::params::SINGLE_MODEL_PATH,
                Some(payload.clone()),
                &[],
                &env_min,
            )
            .await
            {
                Ok(b) => println!(
                    "[vm-hosts] 极简 {origin} bytes={} head={}",
                    b.len(),
                    &String::from_utf8_lossy(&b)[..80.min(b.len())]
                ),
                Err(e) => println!("[vm-hosts] 极简 {origin} ERR {e}"),
            }
        }

        // 官方形态对照：body gzip + content-encoding 头（官方 virgin 实抓）
        // 官方原始 body 逐字节（vid 7697626904827792409，video_id_type 形态）
        let official_payload = br#"{"biz_param":{"detail_page_version":0,"device_level":3,"disable_digg_stat":false,"disable_video_relate_book":false,"from_video_id":"","need_all_video_definition":true,"need_mp4_align":false,"use_os_player":false,"use_server_dns":false,"video_id_type":0,"video_platform":3},"video_id":"7697626904827792409"}"#.to_vec();
        let gz_official = {
            use flate2::write::GzEncoder;
            use std::io::Write;
            let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::best());
            enc.write_all(&official_payload)
                .expect("gzip 写入内存不可能失败");
            enc.finish().expect("gzip 收尾不可能失败")
        };
        match crate::client::api_call_full_response(
            "https://reading.snssdk.com",
            crate::params::SINGLE_MODEL_PATH,
            Some(gz_official),
            &[],
            &[("Content-Encoding".into(), "gzip".into())],
            &env,
        )
        .await
        {
            Ok(r) => println!(
                "[vm-hosts] 官方body bytes={} head={}",
                r.bytes.len(),
                &String::from_utf8_lossy(&r.bytes)[..120.min(r.bytes.len())]
            ),
            Err(e) => println!("[vm-hosts] 官方body ERR {e}"),
        }
    }

    /// 注册→取流全链路活体探针：按当前注册口径注册新设备，立刻用产物
    /// 档案单集取流。**注册口径升级（`REGISTER_VERSION_*` 变动）后的
    /// 必跑闸门**——注册身份与取流身份不同源时这里会回 200+0/服务异常
    /// （docs 第 0 节：匿名/陈旧身份一律 200+0）。
    /// `cargo test -p hongguo-api probe_register_then_play_live -- --ignored --nocapture`
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
            "[reg-play] registered device_id={} iid={} new_user={}",
            reg.device_id, reg.install_id, reg.new_user
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

        let vid = "7677207072068340761"; // 2026-10-08 官方真机 video_model/v1 实流抓包
        let payload = serde_json::to_vec(&crate::params::single_model_payload(
            vid,
            crate::params::SINGLE_CONTENT_TYPE_FALLBACK,
        ))
        .expect("payload 序列化不可能失败");
        match crate::client::api_call_full(
            crate::client::LQ_API_ORIGIN,
            crate::params::SINGLE_MODEL_PATH,
            Some(payload),
            &[],
            &env,
        )
        .await
        {
            Ok(b) => {
                let head = String::from_utf8_lossy(&b[..120.min(b.len())]);
                let ok = b.len() > 200
                    && (b.windows(11).any(|w| w == b"video_model")
                        || b.windows(9).any(|w| w == b"video_id\""));
                println!(
                    "[reg-play] play bytes={} head={head} -> {}",
                    b.len(),
                    if ok { "STREAM_OK" } else { "REJECTED?" }
                );
            }
            Err(e) => println!("[reg-play] play ERR {e}"),
        }
    }
}
