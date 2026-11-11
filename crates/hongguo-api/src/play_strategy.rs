//! 官方播放策略响应与清晰度策略。

use serde_json::Value;

use crate::client::ApiEnv;
use crate::resolution_policy::{Strategy, decode};
use hongguo_common::{AppError, AppResult};
use hongguo_signer::device::fingerprint_for_cdid;

/// 播放策略端点（官方播放器栈，api5 sinfonlinea 边缘）。
pub const PLAY_STRATEGY_PATH: &str = "/reading/media/play/strategy/";

/// 播放器栈 origin（play/strategy 官方实流落点）。
pub const MEDIA_API_ORIGIN: &str = "https://api5-normal-sinfonlinea.fqnovel.com";

/// 播放策略接口单独自报的版本。业务 query 的版本身份在 signer，是 74332 / 7.4.3.32。
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
        .replacen(
            hongguo_signer::REGISTER_VERSION_CODE_STR,
            MEDIA_VERSION_CODE_STR,
            1,
        );
    device.set_user_agent(&ua);
    let mut media_env = env.clone();
    media_env.device = device;
    media_env
}

pub async fn fetch_play_strategy(env: &ApiEnv) -> AppResult<Strategy> {
    let media_env = media_stack_env(env);
    let biz = [
        ("device_level".to_string(), "3".to_string()),
        ("device_score".to_string(), "0".to_string()),
        ("need_personal_recommend".to_string(), "1".to_string()),
    ];
    let bytes =
        crate::client::api_call_full(MEDIA_API_ORIGIN, PLAY_STRATEGY_PATH, None, &biz, &media_env)
            .await?;
    parse_strategy(&bytes)
}

fn parse_strategy(bytes: &[u8]) -> AppResult<Strategy> {
    let v: Value = serde_json::from_slice(bytes)
        .map_err(|e| AppError::Media(format!("播放策略响应非 JSON: {e}")))?;
    let code = v.pointer("/code").and_then(Value::as_i64).unwrap_or(-1);
    if code != 0 {
        let msg = v
            .pointer("/message")
            .and_then(Value::as_str)
            .unwrap_or("未知");
        return Err(AppError::Media(format!("播放策略接口返回 {code}: {msg}")));
    }
    let query_gap = v
        .pointer("/data/query_gap")
        .and_then(Value::as_u64)
        .filter(|g| *g > 0)
        .unwrap_or(DEFAULT_QUERY_GAP_SECS);
    #[derive(serde::Deserialize)]
    struct RuleList {
        #[serde(default)]
        list: Option<Vec<crate::resolution_policy::StrategyRule>>,
    }
    // 官方 JSONUtils.getSafeObject 将无效的可选策略置空，不保留旧规则。
    let optional = |key: &str| {
        v.pointer("/data/strategies")
            .and_then(|value| value.get(key))
    };
    let rules: Option<RuleList> = decode(optional("resolution_strategy")).unwrap_or_else(|error| {
        log::warn!("[PlayStrategy] 忽略无效分辨率规则: {error}");
        None
    });
    let peak_demotion = decode(optional("peak_demotion")).unwrap_or_else(|error| {
        log::warn!("[PlayStrategy] 忽略无效高峰规则: {error}");
        None
    });
    Ok(Strategy {
        query_gap,
        is_peak_period: v
            .pointer("/data/is_peak_period")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        use_videoarch_peak_period: v
            .pointer("/data/use_videoarch_peak_period")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        peak_demotion,
        rules: rules.and_then(|rules| rules.list),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_gap_is_official_300s() {
        assert_eq!(DEFAULT_QUERY_GAP_SECS, 300);
    }

    #[test]
    fn parses_official_response_shape() {
        let sample = r#"{"code":0,"data":{"query_gap":300,"is_peak_period":false,
            "strategies":{"peak_demotion":"{}","play_strategy_enable_server":"true",
            "playrate_strategy":"{\"default_play_speed\":\"1.0\"}","resolution_strategy":""}},
            "message":"MediaApiERR_SUCCESS"}"#;
        let parsed = parse_strategy(sample.as_bytes()).unwrap();
        assert_eq!(parsed.query_gap, 300);
        assert!(parsed.rules.is_none());
        assert!(parsed.peak_demotion.unwrap().resolution.is_none());
    }

    #[test]
    fn decodes_nested_rules_and_clears_missing_or_invalid_optional_rules() {
        let raw = serde_json::json!({"code":0,"data":{"query_gap":123,"is_peak_period":true,
            "use_videoarch_peak_period":true,"strategies":{
                "peak_demotion":"{\"resolution\":\"480P\",\"resolution_in_peak_min_episode_index\":12}",
                "resolution_strategy":"{\"list\":[{\"suggest_resolution_type\":\"1080P\",\"keep_alive_duration\":30}]}"}}});
        let parsed = parse_strategy(raw.to_string().as_bytes()).unwrap();
        assert_eq!(parsed.query_gap, 123);
        assert!(parsed.is_peak_period && parsed.use_videoarch_peak_period);
        assert_eq!(
            parsed
                .peak_demotion
                .unwrap()
                .resolution_in_peak_min_episode_index,
            12
        );
        assert_eq!(parsed.rules.unwrap()[0].keep_alive_duration, 30);
        for value in ["{}", "null", "broken", ""] {
            let parsed = parse_strategy(
                serde_json::json!({"code":0,"data":{"strategies":{
                "resolution_strategy":value}}})
                .to_string()
                .as_bytes(),
            )
            .unwrap();
            assert!(parsed.rules.is_none());
        }
    }

    #[test]
    fn media_env_overrides_version_family_only() {
        let env = ApiEnv::anonymous(Default::default());
        let media = media_stack_env(&env);
        assert_eq!(media.device.get("version_code"), MEDIA_VERSION_CODE_STR);
        assert_eq!(media.device.get("pv_player"), MEDIA_VERSION_CODE_STR);
        assert_eq!(
            media.device.get("manifest_version_code"),
            MEDIA_VERSION_CODE_STR
        );
        assert_eq!(media.device.get("channel"), env.device.get("channel"));
        assert_eq!(
            media.device.get("device_type"),
            env.device.get("device_type")
        );
        assert!(media.device.user_agent().contains(MEDIA_VERSION_CODE_STR));
        assert!(!media.device.user_agent().contains("Cronet"));
    }
}
