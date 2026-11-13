//! HEVC（`hvc1` / `hev1`）解核：`rusty_h265`，纯 Rust、单线程。
//!
//! 服务端的 `h265` 与 `bytevc1` 在容器里都是这两个四字码之一。`bytevc1` 能否被标准 HEVC
//! 解码器逐帧解对，要靠真实样本对拍确认——本模块只按 ITU-T H.265 解，不做任何私有扩展的猜测；
//! 解码器报的每个错都原样上抛，不吞、不换路。

use hongguo_bytevc2::{DecodedFrame, FramePool, presentation_tick};
use hongguo_common::{AppError, AppResult};
use hongguo_media::hevc::{hvcc_payload_from_head, param_nalus_from_hvcc, to_annexb};
use hongguo_video::sample_table::TrackInfo;
use rusty_h265::nal::unescape;
use rusty_h265::ps::parse_sps;
use rusty_h265::{Decoder, Error as HevcError, Frame, Plane};

use crate::color::FrameColor;

const START_CODE: [u8; 4] = [0, 0, 0, 1];

/// 与 [`presentation_tick`] 同一刻度：时间戳以 90 kHz 整数交给解码器，重排后原样带回。
const PRESENTATION_TIMESCALE: f64 = 90_000.0;

/// 明文 MP4 里的 HEVC 四字码。`encv`（没去保护）与其它编码都不算。
pub(crate) fn is_hevc_fourcc(codec: &str) -> bool {
    codec.starts_with("hvc") || codec.starts_with("hev")
}

/// 明文 MP4 里的 HEVC 视频轨。
pub(crate) struct HevcTrack {
    pub(crate) info: TrackInfo,
    /// `hvcC` 里的参数集，已是 Annex-B。`hev1` 的参数集可以只在码流里，这里允许为空。
    params: Vec<u8>,
    /// 样本里 NAL 长度前缀的字节数。
    length_size: usize,
    /// 码流自己声明的 YUV 口径；没声明或读不出时是按分辨率的惯例值。
    pub(crate) color: FrameColor,
}

/// SPS 的 NAL 类型号。
const NAL_SPS: u8 = 33;

/// 从参数集里读 YUV 口径（SPS 的 VUI）。读不到就按惯例：限幅范围，矩阵看分辨率。
fn color_from_params(nalus: &[Vec<u8>], height: usize) -> FrameColor {
    nalus
        .iter()
        .filter(|nalu| nalu.len() > 2 && (nalu[0] >> 1) & 0x3f == NAL_SPS)
        .find_map(|nalu| parse_sps(&unescape(&nalu[2..]).data).ok())
        .and_then(|sps| sps.vui)
        .map(|vui| {
            FrameColor::from_matrix_coeffs(vui.matrix_coeffs, vui.video_full_range_flag, height)
        })
        .unwrap_or_else(|| FrameColor::assumed(height, false))
}

impl HevcTrack {
    pub(crate) fn from_moov(moov: &[u8], info: TrackInfo) -> AppResult<Self> {
        let hvcc = hvcc_payload_from_head(moov, &info)?;
        // hvcC[21] 的低两位是 lengthSizeMinusOne；参数集数组可能为空（hev1），长度前缀必须先单独取。
        let length_size = hvcc
            .get(21)
            .map(|flags| (flags & 0x03) as usize + 1)
            .ok_or_else(|| AppError::Media("hvcC 长度异常".into()))?;
        let nalus = match param_nalus_from_hvcc(&hvcc) {
            Ok((nalus, _)) => nalus,
            Err(_) if info.codec.starts_with("hev") => Vec::new(),
            Err(error) => return Err(error),
        };
        let mut params = Vec::with_capacity(nalus.iter().map(|nalu| nalu.len() + 4).sum());
        for nalu in &nalus {
            params.extend_from_slice(&START_CODE);
            params.extend_from_slice(nalu);
        }
        let color = color_from_params(&nalus, info.height as usize);
        Ok(Self {
            info,
            params,
            length_size,
            color,
        })
    }
}

/// 从解码序第 `start_index` 个样本起解到轨尾，按显示序逐帧回调。
/// `start_index` 须是关键帧；`read(偏移, 长度, 缓冲)` 由调用方供数。
pub(crate) fn decode_track_hevc(
    track: &HevcTrack,
    start_index: usize,
    mut read: impl FnMut(u64, u64, &mut Vec<u8>) -> AppResult<()>,
    on_frame: &mut impl FnMut(DecodedFrame) -> AppResult<()>,
) -> AppResult<()> {
    let timeline = track.info.sample_pts();
    let stamp_pts = timeline.len() == track.info.samples.len() && !timeline.is_empty();
    let mut decoder = Decoder::new();
    let pool = FramePool::new();
    let mut raw = Vec::new();
    let mut fed = false;

    for (index, &(offset, size)) in track.info.samples.iter().enumerate().skip(start_index) {
        read(offset, size, &mut raw)?;
        let sample = to_annexb(&raw, track.length_size);
        if sample.is_empty() {
            continue;
        }
        if !fed {
            fed = true;
            if !track.params.is_empty() {
                decoder.push_annexb(&track.params, None).map_err(failed)?;
            }
        }
        let pts = stamp_pts.then(|| presentation_tick(timeline[index]));
        decoder.push_annexb(&sample, pts).map_err(failed)?;
        drain(&mut decoder, &pool, on_frame)?;
    }
    if !fed {
        return Err(AppError::Media("HEVC 没有可解码的样本".into()));
    }
    decoder.flush();
    drain(&mut decoder, &pool, on_frame)
}

/// 把解码器已经排好显示序的帧全部取走。
fn drain(
    decoder: &mut Decoder,
    pool: &FramePool,
    on_frame: &mut impl FnMut(DecodedFrame) -> AppResult<()>,
) -> AppResult<()> {
    loop {
        match decoder.next_frame() {
            Ok(frame) => on_frame(pack(&frame, pool)?)?,
            Err(HevcError::Again | HevcError::Eof) => return Ok(()),
            Err(error) => return Err(failed(error)),
        }
    }
}

fn failed(error: HevcError) -> AppError {
    match error {
        HevcError::Unsupported(what) => {
            AppError::Media(format!("内置 HEVC 解码器不支持这条码流: {what}"))
        }
        other => AppError::Media(format!("HEVC 解码失败: {other}")),
    }
}

/// 解码器的一帧（编码尺寸、`u16` 样本）→ 裁剪后的 8-bit I420。
fn pack(frame: &Frame, pool: &FramePool) -> AppResult<DecodedFrame> {
    let picture = &frame.picture;
    let with_chroma = match picture.chroma_format_idc {
        0 => false,
        1 => true,
        other => {
            return Err(AppError::Media(format!(
                "HEVC 色度格式暂不支持: chroma_format_idc={other}"
            )));
        }
    };
    let (crop_x, crop_y, crop_w, crop_h) = picture.crop;
    // I420 要偶数边长；奇数裁剪窗丢掉最后一行/列（与 ByteVC2 的打包口径一致）。
    let width = crop_w & !1;
    let height = crop_h & !1;
    if width == 0 || height == 0 {
        return Err(AppError::Media("HEVC 解码分辨率无效".into()));
    }
    let mut y = pool.luma();
    narrow(
        &picture.planes[0],
        (crop_x, crop_y),
        (width, height),
        depth_shift(picture.bit_depth_luma)?,
        &mut y,
    )?;
    let chroma = (width / 2, height / 2);
    let mut u = pool.chroma();
    let mut v = pool.chroma();
    if with_chroma {
        let origin = (crop_x / 2, crop_y / 2);
        let shift = depth_shift(picture.bit_depth_chroma)?;
        narrow(&picture.planes[1], origin, chroma, shift, &mut u)?;
        narrow(&picture.planes[2], origin, chroma, shift, &mut v)?;
    } else {
        for plane in [&mut u, &mut v] {
            plane.clear();
            plane.resize(chroma.0 * chroma.1, 128);
        }
    }
    let pts = frame.pts.map(|ticks| ticks as f64 / PRESENTATION_TIMESCALE);
    Ok(pool.frame(width, height, [y, u, v], pts))
}

fn depth_shift(bit_depth: u8) -> AppResult<u8> {
    match bit_depth {
        8..=12 => Ok(bit_depth - 8),
        other => Err(AppError::Media(format!("HEVC 位深暂不支持: {other}"))),
    }
}

/// 从一个 `u16` 平面里取出 `size` 大小的窗口，截断右移成 8-bit，紧密写进 `out`（先清空，容量复用）。
fn narrow(
    plane: &Plane,
    (x, y): (usize, usize),
    (width, height): (usize, usize),
    shift: u8,
    out: &mut Vec<u8>,
) -> AppResult<()> {
    if x + width > plane.width || y + height > plane.height {
        return Err(AppError::Media("HEVC 帧平面越界".into()));
    }
    out.clear();
    out.reserve(width * height);
    for row in y..y + height {
        // 定长切片 + 迭代器写入：编译器能把这一行向量化（逐元素 push 带容量检查，向量化不了）。
        let src = &plane.row(row)[x..x + width];
        out.extend(src.iter().map(|&sample| (sample >> shift).min(255) as u8));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane(width: usize, height: usize, value: impl Fn(usize, usize) -> u16) -> Plane {
        let mut plane = Plane::new(width, height);
        for row in 0..height {
            for (col, sample) in plane.row_mut(row).iter_mut().enumerate() {
                *sample = value(col, row);
            }
        }
        plane
    }

    #[test]
    fn narrow_crops_the_window_and_packs_rows_tightly() {
        let source = plane(6, 4, |col, row| (row * 10 + col) as u16);
        let mut out = vec![99u8; 3];
        narrow(&source, (1, 1), (4, 2), 0, &mut out).expect("裁剪");
        assert_eq!(out, [11, 12, 13, 14, 21, 22, 23, 24]);
    }

    #[test]
    fn narrow_truncates_high_bit_depth_and_saturates() {
        // 10-bit：1023 → 255，512 → 128，3 → 0；超出 10-bit 的脏值封顶而不是回绕。
        let source = plane(4, 1, |col, _| [1023u16, 512, 3, 4095][col]);
        let mut out = Vec::new();
        narrow(&source, (0, 0), (4, 1), 2, &mut out).expect("收窄");
        assert_eq!(out, [255, 128, 0, 255]);
    }

    #[test]
    fn narrow_rejects_a_window_outside_the_plane() {
        let source = plane(4, 4, |_, _| 0);
        let mut out = Vec::new();
        assert!(narrow(&source, (2, 0), (4, 2), 0, &mut out).is_err());
        assert!(narrow(&source, (0, 3), (2, 2), 0, &mut out).is_err());
    }

    #[test]
    fn depth_shift_accepts_8_to_12_bits_only() {
        assert_eq!(depth_shift(8).expect("8"), 0);
        assert_eq!(depth_shift(10).expect("10"), 2);
        assert_eq!(depth_shift(12).expect("12"), 4);
        assert!(depth_shift(7).is_err());
        assert!(depth_shift(16).is_err());
    }

    #[test]
    fn fourcc_check_is_strict_about_unprotected_hevc() {
        assert!(is_hevc_fourcc("hvc1"));
        assert!(is_hevc_fourcc("hev1"));
        assert!(!is_hevc_fourcc("encv"), "没去保护的轨不能当 HEVC 送解");
        assert!(!is_hevc_fourcc("avc1"));
        assert!(!is_hevc_fourcc("bvc2"));
        assert!(!is_hevc_fourcc(""));
    }
}
