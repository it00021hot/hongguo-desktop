//! 内置播放解码能力（service 内部：play_service 起播分流与设备档案徽标共用）。

use hongguo_api::stream_pick::DecoderCapability;

pub(crate) fn playback_cap() -> DecoderCapability {
    DecoderCapability::builtin()
        .with_bytevc2(hongguo_media_platform::capability::bytevc2_decode_available())
}

pub(crate) fn plays_direct(codec: &str) -> bool {
    hongguo_api::stream_pick::builtin_decodable(codec)
}
