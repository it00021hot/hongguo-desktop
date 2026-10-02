//! H.264 → MP4 封装（muxide，faststart）。
//!
//! 从 `transcode.rs` 拆出：编码与封装是两步不同职责，
//! 拆开后各自都短到能一眼看完。

use std::path::Path;

use crate::error::{AppError, AppResult};

use super::audio::AudioFormat;
use super::TranscodeOptions;

/// 音轨的封装参数与样本（时间戳秒, AAC 帧）。
pub type AudioInput<'a> = (AudioFormat, &'a [(f64, Vec<u8>)]);

/// 封装成 MP4（faststart）。
///
/// `audio` 传 `None` 表示源里没有音轨。有音轨时**直接打包 AAC 帧**——
/// 解密后的音轨样本已是明文，不需要解码成 PCM 再重编码，
/// 那样既慢又会有 generational loss。
pub fn mux_h264(
    output: &Path,
    frames: &[(f64, Vec<u8>, bool)],
    width: usize,
    height: usize,
    audio_in: Option<AudioInput<'_>>,
    options: &TranscodeOptions,
) -> AppResult<u64> {
    use muxide::api::{AacProfile, AudioCodec, MuxerBuilder, VideoCodec};

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    let temp = crate::service::download_service::worker::temp_path_for(output);

    // 直接写文件而不是攒在内存里：短剧单集可能上百 MB
    let file = std::fs::File::create(&temp).map_err(|e| AppError::Io(e.to_string()))?;
    let mut builder = MuxerBuilder::new(file).video(
        VideoCodec::H264,
        width as u32,
        height as u32,
        f64::from(options.framerate.max(1.0)),
    );
    let audio: &[(f64, Vec<u8>)] = match audio_in {
        Some((_, s)) => s,
        None => &[],
    };
    let audio_len = audio.len();
    if let Some((fmt, _)) = audio_in {
        builder = builder.audio(
            // 源片源几乎都是 AAC-LC；profile 写错只会让播放器按默认 profile 试解
            AudioCodec::Aac(AacProfile::Lc),
            fmt.sample_rate,
            fmt.channels,
        );
    }
    let mut muxer = builder
        .with_fast_start(true)
        .build()
        .map_err(|e| AppError::Media(format!("MP4 封装器初始化失败: {e}")))?;

    // 音视频必须**按时间戳交织**写入：
    //   - muxide 要求先落一帧视频才接受音频
    //   - 整段音频先写会导致播放器缓冲很久才出声，音画也容易不同步
    let mut v_idx = 0usize;
    let mut a_idx = 0usize;
    while v_idx < frames.len() || a_idx < audio_len {
        let take_video = match (frames.get(v_idx), audio.get(a_idx)) {
            (Some((v_pts, ..)), Some((a_pts, _))) => v_pts <= a_pts,
            (Some(_), None) => true,
            _ => false,
        };
        if take_video {
            let (pts, data, is_key) = &frames[v_idx];
            muxer
                .write_video(*pts, data, *is_key)
                .map_err(|e| AppError::Media(format!("写入视频样本失败: {e}")))?;
            v_idx += 1;
        } else {
            let (pts, data) = &audio[a_idx];
            muxer
                .write_audio(*pts, data)
                .map_err(|e| AppError::Media(format!("写入音频样本失败: {e}")))?;
            a_idx += 1;
        }
    }

    muxer
        .finish()
        .map_err(|e| AppError::Media(format!("完成 MP4 封装失败: {e}")))?;

    let size = std::fs::metadata(&temp).map(|m| m.len()).unwrap_or(0);
    std::fs::rename(&temp, output).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        AppError::Io(format!("原子替换失败: {e}"))
    })?;

    Ok(size)
}
