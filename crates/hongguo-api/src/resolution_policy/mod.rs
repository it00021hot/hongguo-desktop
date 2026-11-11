mod model;
mod network;
pub use model::{
    BASE_DEFINITION, Candidate, DEFINITION_ORDER, Enabled, Environment, HeldStrategy,
    NetworkOptions, PeakDemotion, PeakOptions, Purpose, Selection, SeriesOptions, ServerOptions,
    SpeedCondition, Strategy, StrategyRule, UpgradeOptions,
};
pub use network::{network_speed, record_transfer};

use hongguo_common::{AppError, AppResult};
use serde::de::DeserializeOwned;
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rules {
    pub series: SeriesOptions,
    pub upgrade: UpgradeOptions,
    pub server: ServerOptions,
    pub network: NetworkOptions,
    pub peak: PeakOptions,
    pub v713: Enabled,
    pub bitrate_polynomial: Option<[f64; 4]>,
}

#[derive(Debug, Clone, Default)]
pub struct Sources {
    series: Option<SeriesOptions>,
    upgrade: Option<UpgradeOptions>,
    server: Option<ServerOptions>,
    network: Option<NetworkOptions>,
    peak: Option<PeakOptions>,
    v713: Option<Enabled>,
    bitrate_polynomial: Option<Option<[f64; 4]>>,
}

impl Sources {
    pub fn read(values: &Value, experiments: bool) -> AppResult<Self> {
        let value = |key: &str| {
            values.get(key).and_then(|value| {
                if experiments {
                    value.get("val")
                } else {
                    Some(value)
                }
            })
        };
        let net: Option<Value> = decode(value("engine_net_report_v649"))?;
        let bitrate_polynomial = net.as_ref().map(|value| {
            value
                .pointer("/bitrate_opt_function/ShortPlay")
                .and_then(Value::as_str)
                .and_then(|value| {
                    let coefficients: Vec<_> = value
                        .split(',')
                        .take(4)
                        .map(str::parse::<f64>)
                        .collect::<Result<_, _>>()
                        .ok()?;
                    let coefficients: [f64; 4] = coefficients.try_into().ok()?;
                    coefficients
                        .iter()
                        .all(|value| value.is_finite())
                        .then_some(coefficients)
                })
        });
        Ok(Self {
            series: decode(value("series_resolution_v635"))?,
            upgrade: decode(value("video_play_resolution_strategy_1080_v671"))?,
            server: decode(value("server_series_resolution_v655"))?,
            network: decode(value("resolution_net_strategy_v649"))?,
            peak: decode(value("resolution_in_peak_v629"))?,
            v713: decode(value("short_video_resolution_v713"))?,
            bitrate_polynomial,
        })
    }

    pub fn merge(&mut self, delta: Self) {
        self.series = delta.series.or(self.series.take());
        self.upgrade = delta.upgrade.or(self.upgrade.take());
        self.server = delta.server.or(self.server.take());
        self.network = delta.network.or(self.network.take());
        self.peak = delta.peak.or(self.peak.take());
        self.v713 = delta.v713.or(self.v713.take());
        self.bitrate_polynomial = delta.bitrate_polynomial.or(self.bitrate_polynomial);
    }
}

impl Rules {
    pub fn from_sources(ab: &Sources, settings: &Sources) -> Self {
        Self {
            series: ab
                .series
                .clone()
                .or_else(|| settings.series.clone())
                .unwrap_or_default(),
            upgrade: ab
                .upgrade
                .clone()
                .or_else(|| settings.upgrade.clone())
                .unwrap_or_default(),
            server: ab
                .server
                .clone()
                .or_else(|| settings.server.clone())
                .unwrap_or_default(),
            network: ab
                .network
                .clone()
                .or_else(|| settings.network.clone())
                .unwrap_or_default(),
            peak: ab
                .peak
                .clone()
                .or_else(|| settings.peak.clone())
                .unwrap_or_default(),
            v713: ab
                .v713
                .clone()
                .or_else(|| settings.v713.clone())
                .unwrap_or_default(),
            bitrate_polynomial: ab
                .bitrate_polynomial
                .or(settings.bitrate_polynomial)
                .flatten(),
        }
    }

    pub fn allowed(&self) -> Vec<u32> {
        let definitions: Vec<_> = DEFINITION_ORDER
            .into_iter()
            .filter(|value| {
                self.series
                    .supported_definitions
                    .contains(&format!("{value}P"))
            })
            .collect();
        if definitions.is_empty() {
            DEFINITION_ORDER.to_vec()
        } else {
            definitions
        }
    }

    pub fn select(&self, input: &Selection<'_>, held: &mut HeldStrategy) -> Option<u32> {
        let allowed = self.allowed();
        let support: Vec<_> = input
            .candidates
            .iter()
            .map(|value| value.definition)
            .collect();
        if support.is_empty() {
            return None;
        }
        let desired = input
            .preferred
            .or(input.cached.filter(|_| self.series.opt_cache))
            .or_else(|| self.strategy_resolution(input, held))
            .unwrap_or(BASE_DEFINITION);
        if support.contains(&desired) && allowed.contains(&desired) {
            return Some(desired);
        }
        if support.contains(&BASE_DEFINITION) {
            return Some(BASE_DEFINITION);
        }
        match input.purpose {
            Purpose::Playback => support.into_iter().find(|value| allowed.contains(value)),
            Purpose::Prefetch => allowed.into_iter().find(|value| support.contains(value)),
        }
    }

    fn strategy_resolution(&self, input: &Selection<'_>, held: &mut HeldStrategy) -> Option<u32> {
        let suggestion = if self.upgrade.enable_server || self.v713.enable {
            self.suggestion(input, held)
        } else {
            None
        };
        let env = input.environment;
        let strategy = input.strategy;
        let downgrade = if (self.server.enable_server || strategy.use_videoarch_peak_period)
            && strategy.peak_demotion.is_some()
        {
            strategy
                .peak_demotion
                .as_ref()
                .filter(|rule| {
                    strategy.is_peak_period
                        && (rule.resolution_in_peak_min_episode_index == -1
                            || env.episode == -1
                            || env.episode > rule.resolution_in_peak_min_episode_index)
                })
                .and_then(|rule| rule.resolution.as_deref())
                .and_then(|value| {
                    DEFINITION_ORDER
                        .into_iter()
                        .find(|definition| value == format!("{definition}P"))
                })
        } else {
            let kind = if env.low_device && self.peak.resolution_type_for_low != 0 {
                self.peak.resolution_type_for_low
            } else if env.middle_low_device && self.peak.resolution_type_for_mid_low != 0 {
                self.peak.resolution_type_for_mid_low
            } else {
                self.peak.resolution_type
            };
            match (env.peak, kind) {
                (true, 1) => Some(540),
                (true, 2) => Some(480),
                _ => None,
            }
            .filter(|value| self.allowed().contains(value))
        };
        if !self.network.enable_net_resolution && !strategy.use_videoarch_peak_period {
            return match suggestion {
                Some((value, ignore_peak)) if ignore_peak || downgrade.is_none() => Some(value),
                _ => downgrade,
            };
        }
        let network = self.network_resolution(
            input.candidates,
            env,
            self.network.net_speed_type,
            self.network.not_match_use_smallest,
        );
        if env.peak {
            return match (network, downgrade) {
                (Some(network), Some(downgrade)) => self.compare_allowed(network, downgrade, false),
                (network, downgrade) => network.or(downgrade),
            };
        }
        if input.purpose == Purpose::Prefetch && self.network.lower_time_preload_best_quality {
            return self.compare_allowed(BASE_DEFINITION, network.unwrap_or(BASE_DEFINITION), true);
        }
        network
    }

    fn compare_allowed(&self, a: u32, b: u32, higher: bool) -> Option<u32> {
        let allowed = self.allowed();
        match (
            allowed.iter().position(|value| *value == a),
            allowed.iter().position(|value| *value == b),
        ) {
            (Some(i), Some(j)) => Some(if (i > j) == higher { a } else { b }),
            (Some(_), None) => Some(a),
            (None, Some(_)) => Some(b),
            (None, None) => None,
        }
    }

    fn suggestion(&self, input: &Selection<'_>, held: &mut HeldStrategy) -> Option<(u32, bool)> {
        let rules = input.strategy.rules.as_ref()?;
        if let Some(rule) = &held.rule
            && held.selected_at_ms > 0
            && rule.keep_alive_duration > 0
            && input.now_ms.saturating_sub(held.selected_at_ms)
                < u64::from(rule.keep_alive_duration) * 1000
            && let Some(value) = rule.suggest_resolution_type.as_deref().and_then(definition)
        {
            return Some((value, rule.ignore_peak));
        }
        let (rule, value) = rules
            .iter()
            .filter_map(|rule| {
                rule.suggest_resolution_type
                    .as_deref()
                    .and_then(definition)
                    .map(|value| (rule, value))
            })
            .find(|(rule, value)| self.matches(input, rule, *value))?;
        held.rule = Some(rule.clone());
        held.selected_at_ms = input.now_ms;
        Some((value, rule.ignore_peak))
    }

    fn matches(&self, input: &Selection<'_>, rule: &StrategyRule, desired: u32) -> bool {
        let Some(conditions) = &rule.limit_condition else {
            return true;
        };
        let env = input.environment;
        if self.upgrade.digg_strategy || (conditions.is_wifi && !env.wifi) {
            return false;
        }
        if conditions.check_net_speed
            && self
                .network_resolution(input.candidates, env, 2, false)
                .is_none_or(|value| value < desired)
        {
            return false;
        }
        if let Some(speed) = &conditions.net_speed {
            let measured = (env.predictor_bps / 1000.0).trunc() / 8.0;
            return match speed.check_net_type {
                1 => measured >= speed.network_speed,
                2 => measured < speed.network_speed,
                _ => false,
            };
        }
        true
    }

    fn network_resolution(
        &self,
        candidates: &[Candidate],
        env: Environment,
        kind: u32,
        smallest: bool,
    ) -> Option<u32> {
        let mut mbps = match kind {
            1 => env.target_mbps,
            2 => (env.predictor_bps / 1_000_000.0).trunc(),
            _ => 0.0,
        };
        if !mbps.is_finite() || mbps <= 0.0 {
            return None;
        }
        if let Some([a, b, c, d]) = self.bitrate_polynomial {
            mbps = a * mbps.powi(3) + b * mbps.powi(2) + c * mbps + d;
        }
        if !mbps.is_finite() || mbps <= 0.0 {
            return None;
        }
        let ceiling = mbps * 1_000_000.0;
        candidates
            .iter()
            .filter(|candidate| f64::from(candidate.bitrate) <= ceiling)
            .reduce(|best, value| {
                if value.bitrate > best.bitrate {
                    value
                } else {
                    best
                }
            })
            .or_else(|| {
                smallest
                    .then(|| candidates.iter().min_by_key(|value| value.bitrate))
                    .flatten()
            })
            .map(|candidate| candidate.definition)
    }
}

pub fn definition(value: &str) -> Option<u32> {
    DEFINITION_ORDER
        .into_iter()
        .find(|candidate| value.eq_ignore_ascii_case(&format!("{candidate}P")))
}

pub fn decode<T: DeserializeOwned>(value: Option<&Value>) -> AppResult<Option<T>> {
    let Some(value) = value.filter(|value| !value.is_null() && value.as_str() != Some("")) else {
        return Ok(None);
    };
    let parsed = match value.as_str() {
        Some(text) => serde_json::from_str::<Value>(text)
            .map_err(|error| AppError::Media(format!("播放选档配置无效: {error}")))?,
        None => value.clone(),
    };
    if parsed.is_null() {
        return Ok(None);
    }
    serde_json::from_value(parsed)
        .map(Some)
        .map_err(|error| AppError::Media(format!("播放选档配置无效: {error}")))
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
