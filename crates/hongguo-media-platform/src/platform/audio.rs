//! 音频出口：AAC 解码 + 输出宿主，契约在 api crate，具体类型按编译目标
//! re-export。
//!
//! macOS 解码走 AudioToolbox（AudioConverter，mpv 的 coreaudio AAC 解码
//! 同款），输出走 AudioToolbox **默认输出单元**（output-only，不碰输入
//! 作用域——cpal 的 macOS 后端会，导致每次起播弹麦克风授权）；Windows
//! 解码走 Media Foundation，输出走 cpal（WASAPI 无此语义）。

pub use hongguo_media_platform_api::audio::AudioDecoder;
pub use hongguo_media_platform_api::audio_output::{
    AudioOutputHost, AudioOutputStream, OutputFormat,
};

#[cfg(target_os = "macos")]
pub use hongguo_media_platform_macos::audio::AacDecoder;
#[cfg(target_os = "windows")]
pub use hongguo_media_platform_windows::audio::AacDecoder;

#[cfg(target_os = "macos")]
pub use hongguo_media_platform_macos::audio_output::MacAudioOutput as PlatformAudioOutput;
#[cfg(target_os = "windows")]
pub use hongguo_media_platform_windows::audio_output::CpalAudioOutput as PlatformAudioOutput;
