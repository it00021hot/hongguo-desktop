use hongguo_media::Backend;
use parking_lot::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodeCapability {
    pub platform_encoder: bool,
    pub platform_hw_encoder: bool,
}

static CAPABILITY: RwLock<Option<DecodeCapability>> = RwLock::new(None);

pub fn detect() -> DecodeCapability {
    if let Some(capability) = *CAPABILITY.read() {
        return capability;
    }
    let capability = DecodeCapability {
        platform_encoder: crate::platform::encoder_available(),
        platform_hw_encoder: crate::platform::hevc_hw_encoder_available(),
    };
    *CAPABILITY.write() = Some(capability);
    capability
}

pub fn redetect() -> DecodeCapability {
    *CAPABILITY.write() = None;
    crate::platform::clear_probe_cache();
    detect()
}

pub fn selected_backend() -> Option<Backend> {
    crate::platform::encoder_available().then_some(Backend::Platform)
}

pub fn warm_up() {
    let capability = detect();
    log::info!(
        "[Media] 归档编码：平台={} 硬件={}",
        capability.platform_encoder,
        capability.platform_hw_encoder
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_roundtrips_without_external_codec_fields() {
        let cap = DecodeCapability {
            platform_encoder: true,
            platform_hw_encoder: false,
        };
        let json = serde_json::to_value(cap).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "platformEncoder": true,
                "platformHwEncoder": false
            })
        );
        assert_eq!(
            serde_json::from_value::<DecodeCapability>(json).unwrap(),
            cap
        );
    }

    #[test]
    fn detect_is_cached_and_stable() {
        assert_eq!(detect(), detect());
    }
}
