//! 取流地址。
//!
//! 平台把每集的可用流放在 `video_model`（一个 **JSON 字符串**）里，需要二次解析；
//! 挑哪一条由 [`super::stream_pick`] 决定。
//!
//! ⚠️ 签名失败时服务端返回 **HTTP 200 + 0 字节**，不是错误码。
//!    排查「接口没数据」时第一件事是看字节数，不是状态码。

use serde_json::Value;

use super::stream_pick::{list_definitions, pick_stream_at, Definition};
use crate::error::{AppError, AppResult};

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
pub async fn fetch_play_url(
    vid: &str,
    definition: Option<u32>,
    env: &super::client::ApiEnv,
) -> AppResult<PlayInfo> {
    let payload = serde_json::to_vec(&crate::domain::api::params::model_payload(vid))
        .map_err(|e| AppError::Signer(e.to_string()))?;

    let bytes = crate::domain::api::client::api_call(
        crate::domain::api::params::MODEL_PATH,
        Some(payload),
        env,
    )
    .await?;

    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析响应失败: {e}")))?;

    // 业务错误码：签名对了但参数/内容不对会走这里
    if let Some(code) = value.get("code").and_then(Value::as_i64)
        && code != 0 {
            let msg = value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误");
            return Err(AppError::Media(format!("接口返回 {code}: {msg}")));
        }

    // data[vid].video_model 是 JSON 字符串
    let item = value
        .get("data")
        .and_then(|d| d.get(vid))
        .ok_or_else(|| AppError::Media(format!("响应里没有 {vid} 的数据")))?;

    let model = item
        .get("video_model")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Media("缺少 video_model".into()))?;

    let parsed: Value = serde_json::from_str(model)
        .map_err(|e| AppError::Media(format!("video_model 解析失败: {e}")))?;

    let stream = pick_stream_at(&parsed, definition)
        .ok_or_else(|| AppError::Media("没有可用的视频流".into()))?;

    Ok(PlayInfo {
        url: stream.url,
        encrypted: !stream.spade.is_empty(),
        key_material: stream.spade.into_bytes(),
        codec: stream.codec.unwrap_or_default(),
        definition: stream.definition,
        definitions: list_definitions(&parsed),
    })
}
