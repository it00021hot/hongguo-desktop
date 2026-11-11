use serde::Deserialize;

pub const DEFINITION_ORDER: [u32; 5] = [360, 480, 540, 720, 1080];
pub const BASE_DEFINITION: u32 = 720;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SeriesOptions {
    pub single_progress: bool,
    pub single_series: bool,
    pub show: bool,
    pub opt_cache: bool,
    pub supported_definitions: String,
}

impl Default for SeriesOptions {
    fn default() -> Self {
        Self {
            single_progress: true,
            single_series: false,
            show: true,
            opt_cache: true,
            supported_definitions: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct UpgradeOptions {
    pub enable_server: bool,
    pub digg_strategy: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServerOptions {
    pub enable_server: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Enabled {
    pub enable: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct NetworkOptions {
    pub enable_net_resolution: bool,
    pub lower_time_preload_best_quality: bool,
    pub net_speed_type: u32,
    pub not_match_use_smallest: bool,
}

impl Default for NetworkOptions {
    fn default() -> Self {
        Self {
            enable_net_resolution: false,
            lower_time_preload_best_quality: false,
            net_speed_type: 1,
            not_match_use_smallest: false,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PeakOptions {
    pub resolution_type: u32,
    pub resolution_type_for_low: u32,
    pub resolution_type_for_mid_low: u32,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct StrategyRule {
    pub suggest_resolution_type: Option<String>,
    pub ignore_peak: bool,
    pub keep_alive_duration: u32,
    pub limit_condition: Option<Conditions>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Conditions {
    pub is_wifi: bool,
    pub check_net_speed: bool,
    pub net_speed: Option<SpeedCondition>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SpeedCondition {
    pub check_net_type: u32,
    pub network_speed: f64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PeakDemotion {
    pub resolution: Option<String>,
    pub resolution_in_peak_min_episode_index: i32,
}

impl Default for PeakDemotion {
    fn default() -> Self {
        Self {
            resolution: None,
            resolution_in_peak_min_episode_index: -1,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Strategy {
    pub query_gap: u64,
    pub is_peak_period: bool,
    pub use_videoarch_peak_period: bool,
    pub peak_demotion: Option<PeakDemotion>,
    pub rules: Option<Vec<StrategyRule>>,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub definition: u32,
    pub bitrate: u32,
    pub file_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Playback,
    Prefetch,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Environment {
    pub wifi: bool,
    pub peak: bool,
    pub low_device: bool,
    pub middle_low_device: bool,
    pub episode: i32,
    pub predictor_bps: f64,
    pub target_mbps: f64,
}

#[derive(Debug, Clone, Default)]
pub struct HeldStrategy {
    pub rule: Option<StrategyRule>,
    pub selected_at_ms: u64,
}

pub struct Selection<'a> {
    pub candidates: &'a [Candidate],
    pub preferred: Option<u32>,
    pub cached: Option<u32>,
    pub purpose: Purpose,
    pub strategy: &'a Strategy,
    pub environment: Environment,
    pub now_ms: u64,
}
