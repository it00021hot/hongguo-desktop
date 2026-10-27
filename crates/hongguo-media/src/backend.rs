//! 转码后端的统一标识。
//!
//! 探测、并行度、超订信号都围绕「这次转码实际走了哪条路」。曾经各处用
//! `decoder` 字符串 `contains("rusty")` 互相猜口径，一个枚举说清楚。
//! 顺序即优先级：平台硬编 → ffmpeg 硬编 → ffmpeg 软编。
//!
//! **没有第四档**：纯 Rust 软解软编已随播放兜底链一起移除。机器上既没有
//! 平台编码器、也没装 ffmpeg 时，转码根本做不了——那是「没有后端」，不是
//! 枚举里的一个「档次」，用 `Option::None` 表达。

/// 转码实际走的后端。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Backend {
    /// 平台原生硬编（macOS VideoToolbox / Windows Media Foundation）
    Platform,
    /// 外部 ffmpeg + **真硬件**编码器（nvenc/qsv/amf/能开 `-hw_encoding` 的 mf）
    FfmpegHw,
    /// 外部 ffmpeg + 软件编码器（libx264 等）
    FfmpegSw,
}

impl Backend {
    /// 编码是否真的发生在硬件上。
    ///
    /// 与设置页的速度三档措辞（硬件/软件加速）同一个口径：`FfmpegSw`
    /// 尽管经过 ffmpeg，编码仍在 CPU 上，不算硬件。
    pub fn is_hardware(self) -> bool {
        matches!(self, Backend::Platform | Backend::FfmpegHw)
    }

    /// 解码/转码链路的稳定标识（日志与界面提示）。
    pub fn decoder_label(self) -> &'static str {
        match self {
            Backend::Platform => "platform",
            Backend::FfmpegHw | Backend::FfmpegSw => "ffmpeg",
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
