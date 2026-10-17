//! 取流地址。
//!
//! 平台把每集的可用流放在 `video_model`（一个 **JSON 字符串**）里，需要二次解析；
//! 挑哪一条由 [`super::stream_pick`] 决定。端点走单集
//! `video_model/v1/`（lq 域），见 [`fetch_play_url`] 注释。
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

/// 批量预取结果的暂存表：`fetch_play_batch` 命中时落这里，
/// [`fetch_play_url`] 使用时取走（pop + TTL 双保险——直链有时效）。
static BATCH_CACHE: OnceLock<Mutex<HashMap<String, (Instant, PlayInfo)>>> = OnceLock::new();
/// 批量结果有效期：响应自带 expire_time（小时级），取保守 10 分钟。
const BATCH_TTL: Duration = Duration::from_secs(600);

fn batch_cache() -> &'static Mutex<HashMap<String, (Instant, PlayInfo)>> {
    BATCH_CACHE.get_or_init(Mutex::default)
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
/// 端点走**单集** [`crate::params::SINGLE_MODEL_PATH`]（lq 域）：2026-10-14
/// 抓包实锤批量形态 `multi_video_model/player/v1` 连 hgplayer 自己（全新
/// 注册设备）都收 200+0，真实播放路径是批量空 → 回落单集。我们直接单集，
/// 省一次注定为空的请求。
pub async fn fetch_play_url(
    vid: &str,
    definition: Option<u32>,
    cap: super::stream_pick::DecoderCapability,
    env: &super::client::ApiEnv,
) -> AppResult<PlayInfo> {
    // 批量预取命中（hgplayer 同款：批量给的直接用，不发单集）
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

    let payload = serde_json::to_vec(&crate::params::single_model_payload(vid))
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

/// 批量预取（hgplayer 同款行为）：起播/切剧时对「当前 + 信息流后续几部」
/// 打一次批量 `multi_video_model/player/v1`。2026-10-14 起该形态服务端
/// **恒回 200+0**（连 hgplayer 全新注册设备也 4 连空），但形态保留——
/// 服务端恢复发货时预取自动生效（[`fetch_play_url`] 会命中暂存表）。
/// 解析不出 vid 对应数据时静默跳过（不报错，调用方回落单集）。
///
/// 返回成功暂存的条数。
pub async fn fetch_play_batch(
    vids: &[String],
    cap: super::stream_pick::DecoderCapability,
    env: &super::client::ApiEnv,
) -> AppResult<usize> {
    let vids: Vec<String> = vids
        .iter()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect();
    if vids.is_empty() {
        return Ok(0);
    }
    let payload = serde_json::to_vec(&crate::params::model_payload(&vids))
        .map_err(|e| AppError::Signer(e.to_string()))?;
    let bytes = crate::client::api_call_once_allow_empty(
        crate::client::LQ_API_ORIGIN,
        crate::params::MODEL_PATH,
        Some(payload),
        &[],
        env,
    )
    .await?;
    if bytes.is_empty() {
        // 服务端不发数据的常态：与 hgplayer 一致，静默回落单集
        log::info!("[PlayUrl] 批量预取空响应（{} vid），回落单集", vids.len());
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
    log::info!("[PlayUrl] 批量预取 {}/{} vid 落暂存表", hits, vids.len());
    Ok(hits)
}
