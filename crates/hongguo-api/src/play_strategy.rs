//! 播放策略轮询（官方 74232 virgin 实流对齐，2026-11-03 抓包）。
//!
//! 官方播放器栈在播放期每 `query_gap` 秒（响应下发，默认 300s）拉一次
//! `GET /reading/media/play/strategy/`，拿回默认倍速、高峰期降档、分辨率
//! 干预等策略面板。**只取不施策**：本模块只负责按官方节拍发这条请求补全
//! 行为画像，策略内容不落地（用户明确：不对齐降清晰度/倍速行为）。
//!
//! 两个栈的版本口径并存（各照抄各自实流）：业务栈 query 自报灰度版本
//! 73932（见 [`hongguo_signer::device::APP_VERSION_CODE`]），**播放器栈
//! query/UA 自报真实版本 74232**——本模块属于播放器栈。
//!
//! ```bash
//! cargo test -p hongguo-api --lib play_strategy
//! ```

use serde_json::Value;

use crate::client::ApiEnv;
use hongguo_common::{AppError, AppResult};
use hongguo_signer::device::fingerprint_for_cdid;

/// 播放策略端点（官方播放器栈，api5 sinfonlinea 边缘）。
pub const PLAY_STRATEGY_PATH: &str = "/reading/media/play/strategy/";

/// 播放器栈 origin（2026-11-03 抓包：play/strategy 全部打这台）。
pub const MEDIA_API_ORIGIN: &str = "https://api5-normal-sinfonlinea.fqnovel.com";

/// 播放器栈自报的真实版本（79/79 条 play/strategy 全为 74232；与业务栈
/// 的灰度自报 73932 并存，各照抄各自实流）。
const MEDIA_VERSION_CODE_STR: &str = "74232";

/// 服务端未下发 `query_gap` 时的轮询间隔（官方响应默认值）。
pub const DEFAULT_QUERY_GAP_SECS: u64 = 300;

/// 派生播放器栈口径的档案副本：版本族与 UA 覆写为 74232 世代（query
/// 由签名器从档案拼出，覆写必须在签名前），其余指纹字段原样。
fn media_stack_env(env: &ApiEnv) -> ApiEnv {
    let mut device = env.device.clone();
    device.set("version_code", MEDIA_VERSION_CODE_STR);
    device.set("version_name", "7.4.2.32");
    device.set("manifest_version_code", MEDIA_VERSION_CODE_STR);
    device.set("update_version_code", MEDIA_VERSION_CODE_STR);
    device.set("pv_player", MEDIA_VERSION_CODE_STR);
    let ua = fingerprint_for_cdid(device.get("cdid"))
        .register_ua()
        .replacen(hongguo_signer::REGISTER_VERSION_CODE_STR, MEDIA_VERSION_CODE_STR, 1);
    device.set_user_agent(&ua);
    let mut media_env = env.clone();
    media_env.device = device;
    media_env
}

/// 拉一次播放策略。成功返回服务端下发的轮询间隔（秒）。
///
/// 失败如实上抛（网络/信封错误），调用方按既有节拍重试；策略面板内容
/// （`strategies`/`is_peak_period` 等）**刻意不返回**——不施策。
pub async fn fetch_play_strategy(env: &ApiEnv) -> AppResult<u64> {
    let media_env = media_stack_env(env);
    let biz = [
        ("device_level".to_string(), "3".to_string()),
        ("device_score".to_string(), "0".to_string()),
        ("need_personal_recommend".to_string(), "1".to_string()),
    ];
    let bytes = crate::client::api_call_full(
        MEDIA_API_ORIGIN,
        PLAY_STRATEGY_PATH,
        None,
        &biz,
        &media_env,
    )
    .await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("播放策略响应非 JSON: {e}")))?;
    let code = v.pointer("/code").and_then(Value::as_i64).unwrap_or(-1);
    if code != 0 {
        let msg = v
            .pointer("/message")
            .and_then(Value::as_str)
            .unwrap_or("未知");
        return Err(AppError::Media(format!("播放策略接口返回 {code}: {msg}")));
    }
    Ok(v.pointer("/data/query_gap")
        .and_then(Value::as_u64)
        .filter(|g| *g > 0)
        .unwrap_or(DEFAULT_QUERY_GAP_SECS))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// query_gap 缺省：响应无该键（或 0）时按官方默认 300s。
    #[test]
    fn default_gap_is_official_300s() {
        assert_eq!(DEFAULT_QUERY_GAP_SECS, 300);
    }

    /// 解析形态锁定（2026-11-03 官方响应样本）：code=0 + query_gap 数值。
    #[test]
    fn parses_official_response_shape() {
        let sample = r#"{"code":0,"data":{"query_gap":300,"is_peak_period":false,
            "strategies":{"peak_demotion":"{}","play_strategy_enable_server":"true",
            "playrate_strategy":"{\"default_play_speed\":\"1.0\"}","resolution_strategy":""}},
            "message":"MediaApiERR_SUCCESS"}"#;
        let v: Value = serde_json::from_str(sample).unwrap();
        assert_eq!(v.pointer("/code").and_then(Value::as_i64), Some(0));
        assert_eq!(
            v.pointer("/data/query_gap").and_then(Value::as_u64),
            Some(300)
        );
    }

    /// 播放器栈档案派生：版本族覆写 74232、其余字段与 UA 世代就位。
    #[test]
    fn media_env_overrides_version_family_only() {
        let env = ApiEnv::anonymous(Default::default());
        let media = media_stack_env(&env);
        assert_eq!(media.device.get("version_code"), MEDIA_VERSION_CODE_STR);
        assert_eq!(media.device.get("pv_player"), MEDIA_VERSION_CODE_STR);
        assert_eq!(media.device.get("manifest_version_code"), MEDIA_VERSION_CODE_STR);
        // 非版本字段不动
        assert_eq!(media.device.get("channel"), env.device.get("channel"));
        assert_eq!(media.device.get("device_type"), env.device.get("device_type"));
        // UA 世代换到播放器栈版本
        assert!(media.device.user_agent().contains(MEDIA_VERSION_CODE_STR));
        assert!(!media.device.user_agent().contains("Cronet"));
    }
}
