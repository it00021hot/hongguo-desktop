//! 所有壳共用的内置播放解码能力。

use hongguo_api::stream_pick::DecoderCapability;

pub(crate) fn playback_cap() -> DecoderCapability {
    DecoderCapability::builtin()
        .with_bytevc2(hongguo_media_platform::capability::bytevc2_decode_available())
}

pub fn plays_direct(codec: &str) -> bool {
    hongguo_api::stream_pick::builtin_decodable(codec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_is_builtin_without_shell_opt_in() {
        assert!(playback_cap().builtin_only);
        for codec in ["bytevc1", "h265", "hvc1", "hev1", "bytevc2", "bvc2"] {
            assert!(plays_direct(codec), "{codec} requires builtin playback");
        }
    }

    #[test]
    fn unsupported_local_codecs_are_not_sent_to_the_builtin_decoder() {
        for codec in ["h264", "avc1", "", "unknown"] {
            assert!(!plays_direct(codec));
        }
    }

    #[test]
    fn online_selection_and_quality_menu_exclude_system_only_gears() {
        use hongguo_api::stream_pick::{list_definitions, pick_stream_at};
        let model = serde_json::json!({ "video_list": [
            { "main_url": "h264", "video_meta": { "codec_type": "h264", "definition": "1080p" } },
            { "main_url": "bvc2", "video_meta": { "codec_type": "bytevc2", "definition": "720p" } },
            { "main_url": "hevc", "video_meta": { "codec_type": "bytevc1", "definition": "480p" } }
        ] });
        let cap = playback_cap();
        let menu: Vec<_> = list_definitions(&model, cap)
            .iter()
            .map(|gear| gear.value)
            .collect();
        let expected = if cap.bytevc2 {
            vec![720, 480]
        } else {
            vec![480]
        };
        assert_eq!(menu, expected);
        assert_eq!(
            pick_stream_at(&model, None, cap).unwrap().definition,
            expected[0]
        );
    }

    #[test]
    fn h264_only_online_response_has_no_playable_candidate() {
        let model = serde_json::json!({ "video_list": [
            { "main_url": "h264", "video_meta": { "codec_type": "h264", "definition": "1080p" } }
        ] });
        assert!(hongguo_api::stream_pick::pick_stream_at(&model, None, playback_cap()).is_none());
    }
}
