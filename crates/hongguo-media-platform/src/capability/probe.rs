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
