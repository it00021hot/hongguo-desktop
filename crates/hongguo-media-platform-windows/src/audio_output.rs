//! Windows 音频输出：cpal（WASAPI 共享模式）。
//!
//! Windows 没有 macOS 那类「建输出流即触发麦克风授权」的语义，cpal 在这边
//! 是干净的；macOS 因为该语义改走 AudioToolbox（见
//! `hongguo-media-platform-macos::audio_output`）。
//!
//! 设备引线口径：cpal 的 `OutputCallbackInfo` 给出 playback 与 callback 两个
//! 时间戳，差值即这批数据到 DAC 的提前量——直接换算成帧交给调用方。

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use hongguo_common::{AppError, AppResult};
use hongguo_media_platform_api::audio_output::{AudioOutputHost, AudioOutputStream, OutputFormat};

/// cpal 输出流（输出域）。
pub struct CpalStream {
    stream: cpal::Stream,
    playing: std::sync::atomic::AtomicBool,
}

// SAFETY：WASAPI 的流对象在 cpal 里标了 !Send（保守），实际由设备线程驱动；
// 句柄跨执行器搬运的约束由本声明给出。
unsafe impl Send for CpalStream {}

impl AudioOutputStream for CpalStream {
    fn set_playing(&self, playing: bool) -> AppResult<()> {
        use std::sync::atomic::Ordering;
        if self.playing.swap(playing, Ordering::Relaxed) == playing {
            return Ok(());
        }
        if playing {
            self.stream
                .play()
                .map_err(|e| AppError::Media(format!("启动音频输出失败: {e}")))
        } else {
            self.stream
                .pause()
                .map_err(|e| AppError::Media(format!("暂停音频输出失败: {e}")))
        }
    }
}

/// Windows 的输出宿主（门面按目标 re-export）。
pub struct CpalAudioOutput;

impl AudioOutputHost for CpalAudioOutput {
    type Stream = CpalStream;

    fn default_output_format() -> AppResult<OutputFormat> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| AppError::Media("没有默认音频输出设备".into()))?;
        let config = device
            .default_output_config()
            .map_err(|e| AppError::Media(format!("读音频设备混音格式失败: {e}")))?;
        Ok(OutputFormat {
            sample_rate: config.sample_rate().0,
            channels: config.channels(),
        })
    }

    fn open<F>(format: OutputFormat, mut render: F) -> AppResult<Self::Stream>
    where
        F: FnMut(&mut [f32], u32) + Send + 'static,
    {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| AppError::Media("没有默认音频输出设备".into()))?;
        let config = cpal::StreamConfig {
            channels: format.channels,
            sample_rate: cpal::SampleRate(format.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };
        let channels = usize::from(format.channels.max(1));
        let sample_rate = format.sample_rate;
        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                    // 设备引线（帧）：这批数据到 DAC 的提前量
                    let lead = info
                        .timestamp()
                        .playback
                        .duration_since(&info.timestamp().callback)
                        .map(|d| d.as_secs_f64() * f64::from(sample_rate))
                        .unwrap_or(0.0);
                    let frames = (data.len() / channels) as u32;
                    render(data, lead.round() as u32);
                    let _ = frames;
                },
                |e| log::warn!("[Platform/audio] 输出流错误: {e}"),
                None,
            )
            .map_err(|e| AppError::Media(format!("建音频输出流失败: {e}")))?;
        log::info!(
            "[Platform/audio] 输出流已建（{}ch {}Hz，WASAPI）",
            format.channels,
            format.sample_rate
        );
        Ok(CpalStream {
            stream,
            playing: std::sync::atomic::AtomicBool::new(false),
        })
    }
}
