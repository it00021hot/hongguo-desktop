//! 音频输出契约：默认设备格式查询 + 拉模式输出流。
//!
//! **为什么自建契约而不是直接用 cpal**：cpal 0.15 的 macOS 后端在
//! `build_output_stream` / `default_output_config` 路径上会初始化
//! AudioUnit（`audio_unit_from_device`），本机实测**只要建流就会触发系统
//! 麦克风授权弹窗**（`probe-build` 单独建流、不 start 也照样触发），而播放
//! 器只需要输出、完全不碰输入。macOS 实现因此改走 AudioToolbox 原生单元
//! （[`hongguo-media-platform-macos`] 的 `audio_output`），Windows 仍用 cpal
//! （WASAPI 没有这类权限语义）。
//!
//! 实现 crate 依赖本契约；门面 `hongguo-media-platform` 按编译目标 re-export
//! 具体类型，调用方（hongguo-playback）只认 [`AudioOutputHost`]。

use hongguo_common::AppResult;

/// 设备混音格式（输出域）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputFormat {
    /// 采样率（Hz）。
    pub sample_rate: u32,
    /// 声道数。
    pub channels: u16,
}

/// 已打开的输出流。数据由设备按固定节拍拉取，暂停不销毁流。
pub trait AudioOutputStream: Send {
    /// 启动 / 暂停设备（幂等）。暂停保留已缓冲的数据，恢复零延迟。
    fn set_playing(&self, playing: bool) -> AppResult<()>;
}

/// 平台音频输出宿主：查询默认输出格式、打开输出流。
///
/// 契约约束：`default_output_format` **不得**触发任何输入设备访问（macOS
/// 上即不得申请麦克风权限）。
pub trait AudioOutputHost {
    /// 具体流类型（静态分派，不开 `dyn`——与仓库既有范式一致）。
    type Stream: AudioOutputStream;

    /// 默认输出设备的混音格式。
    fn default_output_format() -> AppResult<OutputFormat>;

    /// 打开输出流。`render` 在**音频线程**回调：
    ///
    /// - `data`：设备域交错 f32，长度恒为 `frames × channels`；
    /// - `frames_ahead`：这批数据到 DAC 的提前量（帧）。时钟扣掉它才是
    ///   可闻位置（不扣会让时钟快于耳朵）。
    ///
    /// 回调里不得阻塞、不得分配大块内存——它是实时线程。
    fn open<F>(format: OutputFormat, render: F) -> AppResult<Self::Stream>
    where
        F: FnMut(&mut [f32], u32) + Send + 'static;
}
