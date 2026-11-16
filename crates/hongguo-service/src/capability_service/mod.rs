//! 播放能力、设置页硬件探测与解码分流。

use hongguo_api::stream_pick::DecoderCapability;
pub use hongguo_media_platform::capability::probe::DecodeCapability;

pub async fn detect() -> hongguo_common::AppResult<DecodeCapability> {
    crate::runtime::spawn_blocking(hongguo_media_platform::capability::probe::detect)
        .await
        .map_err(|error| hongguo_common::AppError::Internal(error.to_string()))
}

pub async fn redetect() -> hongguo_common::AppResult<DecodeCapability> {
    crate::runtime::spawn_blocking(hongguo_media_platform::capability::probe::redetect)
        .await
        .map_err(|error| hongguo_common::AppError::Internal(error.to_string()))
}

pub(crate) fn playback_cap() -> DecoderCapability {
    DecoderCapability::builtin()
        .with_bytevc2(hongguo_media_platform::capability::bytevc2_decode_available())
}

pub(crate) fn plays_direct(codec: &str) -> bool {
    hongguo_api::stream_pick::builtin_decodable(codec)
}
