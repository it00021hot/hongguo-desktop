#![cfg(target_os = "ios")]

pub use hongguo_media_videotoolbox::{
    YuvEncoder, clear_probe_cache, encoder_available, hevc_hw_encoder_available, transcode_hevc,
    with_encoder_thread,
};

#[cfg(feature = "test-support")]
pub fn transcode_hevc_for_tests(
    request: &hongguo_media_platform_api::PlatformRequest<'_>,
) -> Option<hongguo_common::AppResult<()>> {
    transcode_hevc(request)
}
