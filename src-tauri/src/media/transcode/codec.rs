//! HEVC 解码 → YUV，YUV → H.264 编码。
//!
//! 与 `transcode.rs`（编排）和 `transcode/mux.rs`（封装）分文件：
//! 三步各占一个文件，才看得出转码到底卡在哪一步。

use std::path::Path;

use crate::error::{AppError, AppResult};

use super::{read_range, TranscodeOptions, YuvFrame};

pub(super) fn decode_hevc(
    input: &Path,
    prefix: &[u8],
    length_size: usize,
    samples: &[(u64, u64)],
) -> AppResult<(usize, usize, Vec<YuvFrame>)> {
    use rusty_h265::Decoder;

    let mut decoder = Decoder::new();

    if !prefix.is_empty() {
        decoder
            .push_annexb(prefix, None)
            .map_err(|e| AppError::Media(format!("HEVC 参数集解码失败: {e}")))?;
    }

    let mut frames: Vec<YuvFrame> = Vec::new();
    let mut width = 0usize;
    let mut height = 0usize;

    let drain =
        |decoder: &mut Decoder, frames: &mut Vec<YuvFrame>, w: &mut usize, h: &mut usize| {
            // next_frame 返回 Err 表示「暂时没有更多帧」，是流控信号不是错误
            while let Ok(frame) = decoder.next_frame() {
                if *w == 0 {
                    *w = frame.width;
                    *h = frame.height;
                }
                frames.push(extract_yuv(&frame));
            }
        };

    for (i, &(offset, size)) in samples.iter().enumerate() {
        let raw = read_range(input, offset, size)?;
        if raw.is_empty() {
            continue;
        }
        // MP4 样本是「长度前缀 + NAL」，解码器只认 Annex-B
        let data = crate::media::hevc::to_annexb(&raw, length_size);
        if data.is_empty() {
            continue;
        }

        decoder
            .push_annexb(&data, Some(i as i64))
            .map_err(|e| AppError::Media(format!("样本 {i} 解码失败: {e}")))?;

        drain(&mut decoder, &mut frames, &mut width, &mut height);
    }

    decoder.flush();
    drain(&mut decoder, &mut frames, &mut width, &mut height);

    if width == 0 || height == 0 {
        return Err(AppError::Media("无法确定视频分辨率".into()));
    }

    Ok((width, height, frames))
}

/// 从解码帧取出 YUV 4:2:0 8-bit（按裁剪窗口取）。
///
/// 短剧平台是 Main profile 8-bit 4:2:0，直接取低字节；
/// 10-bit 需要右移 2 位。
fn extract_yuv(frame: &rusty_h265::Frame) -> YuvFrame {
    let p = &frame.picture;
    let (crop_x, crop_y, crop_w, crop_h) = p.crop;
    let shift = p.bit_depth_luma.saturating_sub(8);

    let w = crop_w;
    let h = crop_h;
    let cw = w / 2;
    let ch = h / 2;

    let mut y = vec![0u8; w * h];
    for row in 0..h {
        let src = p.planes[0].row(crop_y + row);
        let dst = &mut y[row * w..row * w + w];
        for (i, px) in dst.iter_mut().enumerate() {
            *px = (src[crop_x + i] >> shift) as u8;
        }
    }

    let mut u = vec![0u8; cw * ch];
    let mut v = vec![0u8; cw * ch];
    for row in 0..ch {
        let su = p.planes[1].row(crop_y / 2 + row);
        let sv = p.planes[2].row(crop_y / 2 + row);
        let du = &mut u[row * cw..row * cw + cw];
        let dv = &mut v[row * cw..row * cw + cw];
        for i in 0..cw {
            du[i] = (su[crop_x / 2 + i] >> shift) as u8;
            dv[i] = (sv[crop_x / 2 + i] >> shift) as u8;
        }
    }

    (y, u, v, w, cw)
}

/// H.264 编码。
pub(super) fn encode_h264(
    frames: &[YuvFrame],
    width: usize,
    height: usize,
    options: &TranscodeOptions,
) -> AppResult<Vec<(f64, Vec<u8>, bool)>> {
    use rusty_h264::{Encoder, EncoderConfig, YuvFrame};

    // 编码器要求宽高为偶数
    let w = width & !1;
    let h = height & !1;

    let mut cfg = EncoderConfig::new(w, h);
    cfg.qp = options.clamped_qp();
    cfg.gop_size = options.gop_size.max(1);
    cfg.framerate = options.framerate.max(1.0);
    // 关掉 mb-tree 前瞻：默认 40 帧缓冲会让前几帧 encode() 返回空，
    // 必须等 flush 才吐数据。逐帧处理场景不需要前瞻，关掉更简单也更快。
    cfg.lookahead = 0;
    cfg.scenecut = 0;

    let mut encoder =
        Encoder::new(cfg).map_err(|e| AppError::Media(format!("H.264 编码器初始化失败: {e}")))?;

    let mut out: Vec<(f64, Vec<u8>, bool)> = Vec::with_capacity(frames.len());
    for (i, (y, u, v, _sy, _sc)) in frames.iter().enumerate() {
        // 帧尺寸与编码器不匹配时（解码分辨率变化）跳过并记日志
        if y.len() < w * h || u.len() < (w / 2) * (h / 2) || v.len() < (w / 2) * (h / 2) {
            log::warn!("[Transcode] 第 {i} 帧尺寸不匹配，跳过");
            continue;
        }

        let frame = YuvFrame {
            width: w,
            height: h,
            y: y[..w * h].to_vec(),
            u: u[..(w / 2) * (h / 2)].to_vec(),
            v: v[..(w / 2) * (h / 2)].to_vec(),
        };

        let pts = i as f64 / f64::from(options.framerate.max(1.0));
        // lookahead 关闭时每帧都有输出；仍要跳过空块
        let au = encoder.encode(&frame);
        if !au.is_empty() {
            let is_keyframe =
                i == 0 || (i as u64).is_multiple_of(u64::from(options.gop_size.max(1)));
            out.push((pts, au, is_keyframe));
        }
    }

    // 排空编码器内部缓冲的尾帧
    let tail = encoder.flush();
    if !tail.is_empty() {
        let pts = frames.len() as f64 / f64::from(options.framerate.max(1.0));
        out.push((pts, tail, false));
    }

    if out.is_empty() {
        return Err(AppError::Media("H.264 编码没有产出任何帧".into()));
    }
    Ok(out)
}
