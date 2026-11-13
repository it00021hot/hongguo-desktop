use serde::{Deserialize, Serialize};

pub(super) const COMPONENT: &str = "hongguo.player-navigation";
pub(super) const ENABLED: &str = "enabled";
pub(super) const WHEEL: &str = "wheel";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct WheelEvent {
    pub delta_y: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_wheel_uses_one_strict_camel_case_contract() {
        let event = WheelEvent { delta_y: -24.0 };
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value, serde_json::json!({"deltaY":-24.0}));
        assert_eq!(serde_json::from_value::<WheelEvent>(value).unwrap(), event);
        assert!(serde_json::from_str::<WheelEvent>(r#"{"delta_y":24}"#).is_err());
        assert!(serde_json::from_str::<WheelEvent>(r#"{"deltaY":"24"}"#).is_err());
        assert!(serde_json::from_str::<WheelEvent>(r#"{"deltaY":24,"extra":1}"#).is_err());
    }
}
