use serde::{Deserialize, Serialize};

pub(super) const COMPONENT: &str = "hongguo.player-navigation";
pub(super) const ENABLED: &str = "enabled";
pub(super) const WHEEL: &str = "wheel";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct WheelEvent {
    pub delta_y: f64,
}
