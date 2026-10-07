//! 转码后端的统一标识。
//!
//! 探测、并行度、超订信号都围绕「这次转码实际走了哪条路」。曾经各处用
//! `decoder` 字符串 `contains("rusty")` 互相猜口径，一个枚举说清楚。
//! 顺序即优先级：平台硬编 → ffmpeg 硬编 → ffmpeg 软编 → 纯 Rust 软解。

/// 转码实际走的后端。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Backend {
    /// 平台原生硬编（macOS VideoToolbox / Windows Media Foundation）
    Platform,
    /// 外部 ffmpeg + **真硬件**编码器（nvenc/qsv/amf/能开 `-hw_encoding` 的 mf）
    FfmpegHw,
    /// 外部 ffmpeg + 软件编码器（libx264 等）
    FfmpegSw,
    /// 纯 Rust 软解软编（rusty_h265 + rusty_h264）
    #[default]
    Rust,
}

impl Backend {
    /// 编码是否真的发生在硬件上。
    ///
    /// 与设置页的速度三档措辞（硬件/软件加速/标准）同一个口径：`FfmpegSw`
    /// 尽管经过 ffmpeg，编码仍在 CPU 上，不算硬件。
    pub fn is_hardware(self) -> bool {
        matches!(self, Backend::Platform | Backend::FfmpegHw)
    }

    /// 解码/转码链路的稳定标识（日志与 `TranscodeResult.decoder`）。
    pub fn decoder_label(self) -> &'static str {
        match self {
            Backend::Platform => "platform_hw",
            Backend::FfmpegHw | Backend::FfmpegSw => "ffmpeg",
            Backend::Rust => "rusty_h265",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_gates_on_where_the_encoder_actually_runs() {
        assert!(Backend::Platform.is_hardware());
        assert!(Backend::FfmpegHw.is_hardware());
        assert!(
            !Backend::FfmpegSw.is_hardware(),
            "libx264 经 ffmpeg 也不是硬件"
        );
        assert!(!Backend::Rust.is_hardware());
    }

    #[test]
    fn serializes_as_camel_case() {
        // 前端直接消费这个枚举的序列化形态
        assert_eq!(
            serde_json::to_string(&Backend::FfmpegHw).unwrap(),
            "\"ffmpegHw\""
        );
        let back: Backend = serde_json::from_str("\"platform\"").unwrap();
        assert_eq!(back, Backend::Platform);
    }
}
