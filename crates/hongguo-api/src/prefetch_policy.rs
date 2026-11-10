use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::Value;

use crate::client::{ApiEnv, api_call_full_with_headers};
use hongguo_common::{AppError, AppResult};

const AB_ORIGIN: &str = "https://abtest3-misc-lq.zijieapi.com";
const AB_PATH: &str = "/common";
const SETTINGS_ORIGIN: &str = "https://is-lq.snssdk.com";
const SETTINGS_PATH: &str = "/service/settings/v3/";
const PRELOAD_KEY: &str = "video_preload_prepare_v619";
const PEAK_KEY: &str = "peak_time_v629";
const REDUCE_KEY: &str = "peak_time_video_preload_opt_v605";
const MIB: u64 = 1024 * 1024;
pub const DEFAULT_PREFETCH_COUNT: u32 = 3;
const DEFAULT_PREFETCH_BYTES: u64 = 5 * MIB;
const MINUTES_PER_DAY: u32 = 24 * 60;

#[derive(Debug, Clone, Default)]
pub struct PrefetchRules {
    preload: Preload,
    peak: PeakTime,
    reduce_at_peak: bool,
    settings: SettingsFallback,
    experiments: SourceValues,
}

impl PrefetchRules {
    pub fn same_policy(&self, other: &Self) -> bool {
        self.preload == other.preload
            && self.peak == other.peak
            && self.reduce_at_peak == other.reduce_at_peak
    }

    fn from_sources(experiments: SourceValues, settings: SettingsFallback) -> Self {
        let fallback = &settings.values;
        let preload = experiments
            .preload
            .clone()
            .or_else(|| fallback.preload.clone());
        let peak = experiments.peak.clone().or_else(|| fallback.peak.clone());
        let reduce = experiments.reduce.or(fallback.reduce);
        Self {
            preload: preload.unwrap_or_default(),
            peak: peak.unwrap_or_default(),
            reduce_at_peak: reduce.is_some_and(|reduce| reduce.strategy != 0),
            settings,
            experiments,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct SettingsFallback {
    timestamp: u64,
    values: SourceValues,
}

#[derive(Debug, Clone, Default)]
struct SourceValues {
    preload: Option<Preload>,
    peak: Option<PeakTime>,
    reduce: Option<PeakReduction>,
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(default)]
struct PeakReduction {
    strategy: i64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct Preload {
    preload_num: u32,
    preload_size: u64,
    preload_hot_time_num: u32,
    preload_hot_time_size: u64,
    preload_low_num: u32,
    preload_low_num_wifi: u32,
    preload_low_size: u64,
    preload_low_size_wifi: u64,
    #[serde(deserialize_with = "suppress_values")]
    preload_suppress: Vec<(u32, u64)>,
}

fn suppress_values<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(u32, u64)>, D::Error> {
    let raw = Option::<Vec<String>>::deserialize(d)?.unwrap_or_default();
    Ok(raw
        .iter()
        .filter_map(|value| value.split_once(['x', 'X']))
        .map(|(count, bytes)| {
            (
                count.parse().unwrap_or_default(),
                bytes.parse().unwrap_or_default(),
            )
        })
        .collect())
}

impl Default for Preload {
    fn default() -> Self {
        Self {
            preload_num: DEFAULT_PREFETCH_COUNT,
            preload_size: DEFAULT_PREFETCH_BYTES,
            preload_hot_time_num: 0,
            preload_hot_time_size: MIB,
            preload_low_num: DEFAULT_PREFETCH_COUNT,
            preload_low_num_wifi: DEFAULT_PREFETCH_COUNT,
            preload_low_size: MIB,
            preload_low_size_wifi: MIB,
            preload_suppress: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct PeakTime {
    peak_start: String,
    peak_end: String,
    peak_random_range_min: u32,
    suppress_range: u32,
    suppress_random: u32,
    enable_low_time: bool,
    low_start: String,
    low_end: String,
    low_random_range_min: u32,
}

impl Default for PeakTime {
    fn default() -> Self {
        Self {
            peak_start: "20:30".into(),
            peak_end: "22:30".into(),
            peak_random_range_min: 30,
            suppress_range: 0,
            suppress_random: 0,
            enable_low_time: false,
            low_start: "06:00".into(),
            low_end: "20:00".into(),
            low_random_range_min: 30,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Normal,
    Peak,
    Suppressed,
    OffPeak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefetchBudget {
    pub phase: Phase,
    pub count: u32,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub struct PrefetchPolicy {
    pub rules: PrefetchRules,
    peak: Option<Window>,
    suppress: Option<(Window, Window)>,
    low: Option<Window>,
}

#[derive(Debug, Clone, Copy)]
struct Window(u32, u32);

impl Window {
    fn contains(self, minute: u32) -> bool {
        distance(self.0, minute) <= distance(self.0, self.1)
    }

    fn section(self, minute: u32, count: usize) -> usize {
        let duration = distance(self.0, self.1) as usize;
        if duration == 0 {
            return 0;
        }
        (distance(self.0, minute) as usize * count / duration).min(count - 1)
    }
}

fn distance(from: u32, to: u32) -> u32 {
    (to + MINUTES_PER_DAY - from) % MINUTES_PER_DAY
}

fn add_minutes(at: u32, minutes: u32) -> u32 {
    (at + minutes.min(MINUTES_PER_DAY)) % MINUTES_PER_DAY
}

fn minute(value: &str) -> Option<u32> {
    let (hour, min) = value.split_once(':')?;
    let hour = hour.parse::<u32>().ok()?;
    let min = min.parse::<u32>().ok()?;
    (hour < 24 && min < 60).then_some(hour * 60 + min)
}

impl PrefetchPolicy {
    pub fn from_rules(rules: PrefetchRules) -> Self {
        Self::with_jitter(rules, |max| {
            rand::random_range(0..=max.min(MINUTES_PER_DAY))
        })
    }

    fn with_jitter(rules: PrefetchRules, mut jitter: impl FnMut(u32) -> u32) -> Self {
        let time = &rules.peak;
        let (peak, suppress) = match (minute(&time.peak_start), minute(&time.peak_end)) {
            (Some(start), Some(end)) => (
                rules
                    .reduce_at_peak
                    .then(|| Window(start, add_minutes(end, jitter(time.peak_random_range_min)))),
                Some((
                    Window(end, add_minutes(end, time.peak_random_range_min)),
                    Window(
                        end,
                        add_minutes(
                            add_minutes(end, time.suppress_range),
                            jitter(time.suppress_random),
                        ),
                    ),
                )),
            ),
            _ => (None, None),
        };
        let low = match (minute(&time.low_start), minute(&time.low_end)) {
            (Some(start), Some(end)) if time.enable_low_time => Some(Window(
                add_minutes(start, jitter(time.low_random_range_min)),
                end,
            )),
            _ => None,
        };
        Self {
            rules,
            peak,
            suppress,
            low,
        }
    }

    pub fn select(&self, minute: u32, wifi: bool) -> PrefetchBudget {
        let cfg = &self.rules.preload;
        let (phase, count, bytes) = if self.peak.is_some_and(|window| window.contains(minute)) {
            (
                Phase::Peak,
                cfg.preload_hot_time_num,
                cfg.preload_hot_time_size,
            )
        } else if let Some((count, bytes)) = self.suppressed(minute) {
            (Phase::Suppressed, count, bytes)
        } else if self.low.is_some_and(|window| window.contains(minute)) {
            if wifi {
                (
                    Phase::OffPeak,
                    cfg.preload_low_num_wifi,
                    cfg.preload_low_size_wifi,
                )
            } else {
                (Phase::OffPeak, cfg.preload_low_num, cfg.preload_low_size)
            }
        } else {
            (Phase::Normal, cfg.preload_num, cfg.preload_size)
        };
        PrefetchBudget {
            phase,
            count: count.min(DEFAULT_PREFETCH_COUNT),
            bytes,
        }
    }

    fn suppressed(&self, minute: u32) -> Option<(u32, u64)> {
        let pairs = &self.rules.preload.preload_suppress;
        let (early, full) = self.suppress.filter(|_| !pairs.is_empty())?;
        if early.contains(minute) {
            return pairs.get(early.section(minute, pairs.len())).copied();
        }
        full.contains(minute)
            .then(|| pairs.last().copied())
            .flatten()
    }
}

pub async fn fetch_prefetch_rules(env: &ApiEnv, previous: Option<&PrefetchRules>) -> PrefetchRules {
    let query = [
        ("cj_pay_sdk_version".into(), "8.1.7".into()),
        ("ecomterm_sdk_version".into(), "44900".into()),
        ("account_region".into(), "cn-gd".into()),
    ];
    let headers = headers();
    let mut anonymous = env.clone();
    anonymous.x_tt_token = None;
    anonymous.cookie = None;
    let prior = previous
        .map(|value| value.settings.clone())
        .unwrap_or_default();
    let (ab, settings) = tokio::join!(
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            api_call_full_with_headers(AB_ORIGIN, AB_PATH, None, &query, &headers, &anonymous)
        ),
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            fetch_settings(&anonymous, prior.clone())
        )
    );
    let settings = match settings {
        Ok(Ok(settings)) => {
            log::debug!(
                "[Prefetch] 官方普通设置已读取，版本 {}，压制档 {}",
                settings.timestamp,
                settings
                    .values
                    .preload
                    .as_ref()
                    .map_or(0, |preload| preload.preload_suppress.len())
            );
            settings
        }
        failed => {
            log::debug!("[Prefetch] 普通设置读取失败: {failed:?}");
            prior
        }
    };
    let experiments = match ab.map(|result| result.and_then(|bytes| parse_ab(&bytes))) {
        Ok(Ok(values)) => {
            log::debug!("[Prefetch] 官方 AB 预取配置已更新");
            values
        }
        failed => {
            log::debug!("[Prefetch] AB 预取配置获取失败: {failed:?}");
            previous
                .map(|value| value.experiments.clone())
                .unwrap_or_default()
        }
    };
    PrefetchRules::from_sources(experiments, settings)
}

fn headers() -> Vec<(String, String)> {
    vec![
        ("x-ss-dp".into(), "8662".into()),
        ("lc".into(), "101".into()),
        ("x-tt-store-region".into(), "cn-gd".into()),
        ("x-tt-store-region-src".into(), "did".into()),
    ]
}

async fn fetch_settings(env: &ApiEnv, previous: SettingsFallback) -> AppResult<SettingsFallback> {
    let mut query = vec![
        ("app".into(), "1".into()),
        ("settings_time".into(), previous.timestamp.to_string()),
        ("font_scale".into(), "100".into()),
        ("account_region".into(), "cn-gd".into()),
    ];
    if env.device.get("ac") == "wifi" {
        query.push(("network_type".into(), "4".into()));
    }
    let mut headers = headers();
    headers.push(("Content-Type".into(), "application/json".into()));
    let bytes = api_call_full_with_headers(
        SETTINGS_ORIGIN,
        SETTINGS_PATH,
        Some(vec![0]),
        &query,
        &headers,
        env,
    )
    .await?;
    parse_settings(&bytes, previous)
}

fn parse_settings(bytes: &[u8], mut previous: SettingsFallback) -> AppResult<SettingsFallback> {
    let json: Value = serde_json::from_slice(bytes)
        .map_err(|err| AppError::Media(format!("预取普通设置响应解析失败: {err}")))?;
    let data = json
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::Media("预取普通设置响应缺少 data".into()))?;
    if let Some(timestamp) = data.get("settings_time").and_then(Value::as_u64) {
        previous.timestamp = timestamp;
    }
    if let Some(settings) = data.get("settings") {
        let values = &mut previous.values;
        values.preload = decode(settings.get(PRELOAD_KEY), PRELOAD_KEY)?.or(values.preload.take());
        values.peak = decode(settings.get(PEAK_KEY), PEAK_KEY)?.or(values.peak.take());
        values.reduce = decode(settings.get(REDUCE_KEY), REDUCE_KEY)?.or(values.reduce);
    }
    Ok(previous)
}

fn parse_ab(bytes: &[u8]) -> AppResult<SourceValues> {
    let json: Value = serde_json::from_slice(bytes)
        .map_err(|err| AppError::Media(format!("预取 AB 响应解析失败: {err}")))?;
    if json.get("code").and_then(Value::as_i64) != Some(0) {
        return Err(AppError::Media("预取 AB 请求未成功".into()));
    }
    let data = json
        .get("data")
        .filter(|data| data.is_object())
        .ok_or_else(|| AppError::Media("预取 AB 响应缺少 data".into()))?;
    let experiment = |key: &str| data.get(key).and_then(|value| value.get("val"));
    Ok(SourceValues {
        preload: decode(experiment(PRELOAD_KEY), PRELOAD_KEY)?,
        peak: decode(experiment(PEAK_KEY), PEAK_KEY)?,
        reduce: decode(experiment(REDUCE_KEY), REDUCE_KEY)?,
    })
}

fn decode<T: DeserializeOwned>(value: Option<&Value>, key: &str) -> AppResult<Option<T>> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let decoded = match value.as_str() {
        Some(json) => serde_json::from_str(json),
        None => serde_json::from_value(value.clone()),
    };
    decoded
        .map(Some)
        .map_err(|err| AppError::Media(format!("预取配置字段 {key} 无效: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_peak_rules_stop_prefetch_without_changing_normal_budget() {
        let response = br#"{"code":0,"data":{
            "peak_time_v629":{"val":{"peak_start":"19:15","peak_end":"23:00","peak_random_range_min":45,"suppress_range":85,"suppress_random":20}},
            "peak_time_video_preload_opt_v605":{"val":{"strategy":2}}
        }}"#;
        let rules = PrefetchRules::from_sources(parse_ab(response).unwrap(), Default::default());
        let policy = PrefetchPolicy::with_jitter(rules, |_| 32);
        assert_eq!(
            policy.select(22 * 60, true),
            PrefetchBudget {
                phase: Phase::Peak,
                count: 0,
                bytes: MIB
            }
        );
        assert_eq!(
            policy.select(10 * 60, true),
            PrefetchBudget {
                phase: Phase::Normal,
                count: 3,
                bytes: 5 * MIB
            }
        );
    }

    #[test]
    fn suppression_beats_low_time_and_selects_the_final_pair_after_midnight() {
        let response = br#"{"code":0,"data":{
            "peak_time_v629":{"val":{"peak_start":"20:30","peak_end":"23:00","peak_random_range_min":30,"suppress_range":85,"enable_low_time":true,"low_start":"23:00","low_end":"06:00","low_random_range_min":0}},
            "peak_time_video_preload_opt_v605":{"val":{"strategy":2}},
            "video_preload_prepare_v619":{"val":{"preload_suppress":["2x2097152","0x1048576"],"preload_low_num_wifi":2,"preload_low_num":1}}
        }}"#;
        let rules = PrefetchRules::from_sources(parse_ab(response).unwrap(), Default::default());
        let policy = PrefetchPolicy::with_jitter(rules, |_| 0);
        assert_eq!(policy.select(23 * 60, true).phase, Phase::Peak);
        assert_eq!(policy.select(23 * 60 + 1, true).count, 2);
        assert_eq!(policy.select(23 * 60 + 15, true).count, 0);
        assert_eq!(policy.select(15, true).phase, Phase::Suppressed);
        assert_eq!(
            policy.select(90, true),
            PrefetchBudget {
                phase: Phase::OffPeak,
                count: 2,
                bytes: MIB
            }
        );
        assert_eq!(policy.select(90, false).count, 1);
    }

    #[test]
    fn settings_delta_preserves_suppression_and_ab_overrides_peak() {
        let settings = br#"{"data":{"settings_time":1,"settings":{
            "peak_time_v629":{"peak_start":"20:30","peak_end":"22:30"},
            "video_preload_prepare_v619":{"preload_suppress":["3x5242880","2x5242880","1x5242880"]}
        }}}"#;
        let settings = parse_settings(settings, Default::default()).unwrap();
        let settings =
            parse_settings(br#"{"data":{"settings_time":2,"settings":{}}}"#, settings).unwrap();
        assert_eq!(settings.timestamp, 2);
        let ab = br#"{"code":0,"data":{
            "peak_time_v629":{"val":{"peak_start":"19:15","peak_end":"23:00","peak_random_range_min":45,"suppress_range":85}},
            "peak_time_video_preload_opt_v605":{"val":{"strategy":2}}
        }}"#;
        let rules = PrefetchRules::from_sources(parse_ab(ab).unwrap(), settings);
        let policy = PrefetchPolicy::with_jitter(rules, |_| 0);
        assert_eq!(policy.select(19 * 60 + 30, true).phase, Phase::Peak);
        for (minute, count) in [(5, 3), (20, 2), (35, 1), (55, 1)] {
            let budget = policy.select(23 * 60 + minute, true);
            assert_eq!(
                (budget.phase, budget.count, budget.bytes),
                (Phase::Suppressed, count, 5 * MIB)
            );
        }
    }
}
