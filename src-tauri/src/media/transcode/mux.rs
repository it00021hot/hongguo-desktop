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

    // B 帧流检出：PTS 非单调即解码序样本（平台硬编器关不掉帧重排，
    // 本机 VT 硬编会话对 MaxFrameDelayCount=0 与 AllowFrameReordering=0
    // 都拒收）。muxide 的硬性契约是「写入顺序严格 PTS 递增」，B 帧流
    // 必须走 write_video_with_dts；DTS 取「第 k 小的显示时间」——
    // 解码序第 k 个样本的解码槽位，与 muxide 文档的 I P B B → dts
    // 0,1,2,3 示例同一公式。软解路径显示序单调，直走 write_video。
    let mut dts_sorted: Vec<f64> = frames.iter().map(|(pts, ..)| *pts).collect();
    dts_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let has_bframes = frames.windows(2).any(|w| w[1].0 < w[0].0);

    if let Some((fmt, _)) = audio_in {
        builder = builder.audio(
            // 源片源几乎都是 AAC-LC；profile 写错只会让播放器按默认 profile 试解
            AudioCodec::Aac(AacProfile::Lc),
            fmt.sample_rate,
            fmt.channels,
        );
    }
    // faststart 只对渐进式下载有意义，本仓产物全是本地文件经自定义协议
    // 播放，关掉没有代价；而 B 帧流必须关——muxide 0.2.5 的 faststart
    // 搬移假设写入序≈pts 序，解码序写入时样本边界整段错位（最小回归
    // 测试 `bframe_decode_order_stream_survives_the_mux` 钉住）。
    let mut muxer = builder
        .with_fast_start(!has_bframes)
        .build()
        .map_err(|e| AppError::Media(format!("MP4 封装器初始化失败: {e}")))?;

    // 音视频必须**按时间戳交织**写入：
    //   - muxide 要求先落一帧视频才接受音频
    //   - 整段音频先写会导致播放器缓冲很久才出声，音画也容易不同步
    // 源片音轨可以比视频轨先开始（红果片源实测：音频 0s、视频首帧
    // 0.133s——AAC priming + 视频轨延迟起步）。muxide 的轨道模型不收
    // 「早于首帧视频的音频」，这段头部本就该按 elst 语义裁掉（软解路径
    // 视频 pts 从 0 生成所以从未触发；平台层真机文件直接全灭）。
    let v0 = frames.first().map_or(0.0, |(pts, ..)| *pts);
    let a_start = audio.partition_point(|(pts, _)| *pts < v0);
    let audio = &audio[a_start..];

    // 首样本仍强制视频，做一层与 pts 无关的保险
    let mut v_idx = 0usize;
    let mut a_idx = 0usize;
    let mut prev_dts: Option<f64> = None;
    while v_idx < frames.len() || a_idx < audio.len() {
        let take_video = match (frames.get(v_idx), audio.get(a_idx)) {
            (Some(_), Some(_)) => v_idx == 0 || frames[v_idx].0 <= audio[a_idx].0,
            (Some(_), None) => true,
            _ => false,
        };
        if take_video {
            let (pts, data, is_key) = &frames[v_idx];
            let res = if has_bframes {
                let mut dts = dts_sorted[v_idx];
                // 重复 PTS 的畸形源会把 DTS 顶成并列——按一个 1/90000 刻度顶开
                if let Some(prev) = prev_dts
                    && dts <= prev {
                        dts = prev + 1.0 / 90_000.0;
                    }
                prev_dts = Some(dts);
                muxer.write_video_with_dts(*pts, dts, data, *is_key)
            } else {
                muxer.write_video(*pts, data, *is_key)
            };
            res.map_err(|e| AppError::Media(format!("写入视频样本失败: {e}")))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// B 帧流（解码序 + 非单调 PTS）封装回归：muxide 对这类流走
    /// write_video_with_dts + 关 faststart。曾整段翻车两次——faststart
    /// 搬移假设写入序≈pts 序（样本边界错位），以及 PTS 透传撞上 muxide
    /// 的「写入序严格 PTS 递增」校验。muxer 不解析 H.264 语法，哑 NAL
    /// 即可做字节级核对：解复用回来的每个样本必须与写入帧的 AVCC 形态
    /// 逐字节一致，且样本序=写入序（解码序）。哑帧不可解码，解码器级
    /// 的验证由真实码流的 e2e（ffprobe）承担。
    #[test]
    fn bframe_decode_order_stream_survives_the_mux() {
        // 解码序 + 非单调 PTS（I P B B 型 GOP 重复几轮）；每帧由若干
        // NAL 组成，IDR 帧按真实产物同构带 SPS(7)/PPS(8)
        let display = [0.0f64, 0.3, 0.1, 0.2, 0.6, 0.4, 0.5, 0.9, 0.7, 0.8];
        let idr_nals: Vec<Vec<u8>> = vec![
            [0x65]
                .iter()
                .chain(std::iter::repeat_n(&0u8, 200))
                .copied()
                .collect(),
            vec![0x67, 0xAA],
            vec![0x68, 0xAA],
        ];
        let frames: Vec<(f64, Vec<u8>, bool)> = display
            .iter()
            .enumerate()
            .map(|(i, &t)| {
                let nals: Vec<Vec<u8>> = if i == 0 {
                    idr_nals.clone()
                } else {
                    vec![[i as u8, 0x55, i as u8].to_vec()]
                };
                let mut annexb = Vec::new();
                for nal in &nals {
                    annexb.extend_from_slice(&[0, 0, 0, 1]);
                    annexb.extend_from_slice(nal);
                }
                (t, annexb, i == 0)
            })
            .collect();
        // 期望：muxide 把 Annex-B 转成 AVCC（每 NAL 前置 4 字节大端长度）
        let expected_avcc: Vec<Vec<u8>> = display
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let nals: Vec<Vec<u8>> = if i == 0 {
                    idr_nals.clone()
                } else {
                    vec![[i as u8, 0x55, i as u8].to_vec()]
                };
                let mut avcc = Vec::new();
                for nal in &nals {
                    avcc.extend_from_slice(&(nal.len() as u32).to_be_bytes());
                    avcc.extend_from_slice(nal);
                }
                avcc
            })
            .collect();

        let dir = std::env::temp_dir().join(format!("hg-mux-bframe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("bframe.mp4");
        let opts = TranscodeOptions::default();
        let size = mux_h264(&out, &frames, 192, 108, None, &opts).expect("B 帧流封装应成功");
        assert!(size > 0);

        let demuxed = crate::media::demux::demux_file(&out).expect("产物应能解复用");
        let video = demuxed.video_track().expect("产物应有视频轨");
        assert_eq!(
            video.info.samples.len(),
            frames.len(),
            "样本数必须与写入帧数一致"
        );
        let data = std::fs::read(&out).unwrap();
        for (i, &(at, len)) in video.info.samples.iter().enumerate() {
            let got = &data[at as usize..(at + len) as usize];
            assert_eq!(
                got, &expected_avcc[i],
                "第 {i} 个样本字节与写入帧（AVCC 形态）不一致——样本边界错位"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 真实规模复现：3000 帧 × ~100KB + AAC 音频交织——小样本全过但真机
    /// 产物在 ~302 帧处样本边界炸开，规模/交织相关就靠这条钉住。
    #[test]
    fn bframe_stream_at_realistic_scale_with_audio() {
        let n = 3000usize;
        let mut frames: Vec<(f64, Vec<u8>, bool)> = Vec::with_capacity(n);
        let mut expected: Vec<Vec<u8>> = Vec::with_capacity(n);
        for i in 0..n {
            // 解码序 I P B B（muxide 文档同款）：组内显示序 I B B P。
            // NAL 列表 → Annex-B 作为输入、AVCC（4 字节长度前缀）作为
            // 解复用期望，两份都从同一份 NAL 机械推导，避免自欺。
            let gop = i % 4;
            // 组基 b=4g：解码 I(b) P(b+3) B(b+1) B(b+2) → 显示序 I B B P
            let pts = match gop {
                0 => (i as f64) / 30.0,
                1 => (i as f64 + 2.0) / 30.0,
                2 | 3 => (i as f64 - 1.0) / 30.0,
                _ => unreachable!(),
            };
            let nals: Vec<Vec<u8>> = if gop == 0 {
                // 关键帧中流内前插 SPS/PPS（sample_to_unit 行为）
                vec![
                    [0x65]
                        .iter()
                        .chain(std::iter::repeat_n(&0u8, 100_000))
                        .copied()
                        .collect(),
                    vec![0x67, 0xAA],
                    vec![0x68, 0xAA],
                ]
            } else {
                vec![vec![0x41, (i % 251) as u8]]
            };
            let mut annexb = Vec::new();
            let mut avcc = Vec::new();
            for nal in &nals {
                annexb.extend_from_slice(&[0, 0, 0, 1]);
                annexb.extend_from_slice(nal);
                avcc.extend_from_slice(&(nal.len() as u32).to_be_bytes());
                avcc.extend_from_slice(nal);
            }
            frames.push((pts, annexb, gop == 0));
            expected.push(avcc);
        }
        let audio: Vec<(f64, Vec<u8>)> = (0..n * 2)
            .map(|i| {
                // 合法 ADTS 头（AAC-LC / 44100 / 立体声，帧长 7+400）+ 载荷
                let len = 407u32;
                let mut adts = vec![
                    0xFF,
                    0xF1,
                    0x51,
                    0x80 | ((len >> 11) as u8 & 0x03),
                    ((len >> 3) as u8),
                    (((len & 0x7) as u8) << 5) | 0x1F,
                    0xFC,
                ];
                adts.extend(vec![0xAA; 400]);
                (i as f64 / 44100.0 * 1024.0, adts)
            })
            .collect();
        let dir = std::env::temp_dir().join(format!("hg-mux-scale-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("scale.mp4");
        let _fmt = crate::media::transcode::audio::AudioFormat {
            sample_rate: 44100,
            channels: 2,
        };
        let opts = TranscodeOptions::default();
        let fmt = crate::media::transcode::audio::AudioFormat {
            sample_rate: 44100,
            channels: 2,
        };
        mux_h264(&out, &frames, 192, 108, Some((fmt, &audio)), &opts).expect("规模封装应成功");

        let demuxed = crate::media::demux::demux_file(&out).expect("产物应能解复用");
        let video = demuxed.video_track().expect("应有视频轨");
        assert_eq!(video.info.samples.len(), n, "样本数应一致");
        let data = std::fs::read(&out).unwrap();
        for (i, &(at, len)) in video.info.samples.iter().enumerate() {
            let got = &data[at as usize..(at + len) as usize];
            assert_eq!(got, &expected[i], "第 {i} 个样本边界错位");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
