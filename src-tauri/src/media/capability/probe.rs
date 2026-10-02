//! 能力探测。
//!
//! 探测的是「当前这台机器转码会走哪条路」：
//! - 有 ffmpeg 且选得到硬件编码器 → 接近实时
//! - 只有 ffmpeg 但只有软编码器 → 中速
//! - 没有 ffmpeg → 纯 Rust 软解，慢
//!
//! 探测结果全局缓存（探测会真的试编一帧，不能每次都做）。

use std::sync::OnceLock;

/// 探测结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodeCapability {
    /// 装了 ffmpeg
    pub has_ffmpeg: bool,
    /// ffmpeg 可用的 H.264 硬件编码器（nvenc/qsv/amf/mf）
    pub h264_hw_encoder: bool,
}

static CAPABILITY: OnceLock<DecodeCapability> = OnceLock::new();

/// 探测一次并缓存。
pub fn detect() -> DecodeCapability {
    *CAPABILITY.get_or_init(|| {
        let ffmpeg = crate::media::ffmpeg::ffmpeg_path().is_some();
        let encoder = crate::media::ffmpeg::h264_encoder().map(str::to_string);
        let h264_hw_encoder = encoder.as_deref().is_some_and(|e| e != "libx264");

        DecodeCapability {
            has_ffmpeg: ffmpeg,
            h264_hw_encoder,
        }
    })
}

/// 预热探测（启动时调用，避免首次转码时的额外延迟）。
pub fn warm_up() {
    let c = detect();
    let backend = crate::media::backend_info();
    log::info!(
        "[Media] ffmpeg={} 编码器={} → {}",
        c.has_ffmpeg,
        backend.encoder,
        backend.transcode_with
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_is_cached_and_stable() {
        let a = detect();
        let b = detect();
        assert_eq!(a, b, "探测结果应被缓存，不重复试编");
    }

    #[test]
    fn capability_is_serializable() {
        // 反序列化回来必须字段无损，前端直接消费这个结构
        let cap = detect();
        let json = serde_json::to_string(&cap).unwrap();
        let back: DecodeCapability = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, back, "序列化往返应无损");
    }
}
