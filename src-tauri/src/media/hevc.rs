//! HEVC 参数集读取：`hvcC` → Annex-B。
//!
//! MP4 里 HEVC 参数集存在 `hvcC` box（长度前缀格式），而解码器要的是
//! Annex-B（起始码分隔）。少这一步，解码器拿不到 SPS/PPS/VPS，
//! **首个 IDR 帧必然解不出来**，而且不会报错——只表现为一直黑屏。
//!
//! `hvcC` 结构（ISO/IEC 14496-15）：
//!
//! ```text
//! configurationVersion(1) | profile byte(1) | 4×reserved(4)
//! | minSpatialSegmentation(2) | parallelismType(1) | chromaFormat(1)
//! | bitDepthLuma(1) | bitDepthChroma(1) | avgFrameRate(2)
//! | constantFrameRate(1) | numTemporalLayers(1) | temporalIdNested(1)
//! | lengthSizeMinusOne(1)
//! | numOfArrays(1)
//! | arrays[numOfArrays]: array_completeness(1) | NAL_unit_type(1)
//!                    | numNalus(2) | nalus[numNalus]
//! ```
//!
//! 解码只关心 `lengthSizeMinusOne` 与 `arrays` 里的参数集。

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::domain::mp4::r#box::find_box;
use crate::domain::mp4::sample_table::TrackInfo;
use crate::error::{AppError, AppResult};

/// VisualSampleEntry 载荷长度（ISO/IEC 14496-15）：
/// reserved(6) + data_ref_idx(2) + pre_defined(2) + reserved(2) + pre_defined[3](12)
/// + width(2) + height(2) + horiz/vert res(8) + reserved(4) + frame_count(2)
/// + compressorname(32) + depth(2) + pre_defined(2)
const VISUAL_SAMPLE_ENTRY_PAYLOAD: usize = 78;

/// Annex-B 起始码。
const START_CODE: [u8; 4] = [0, 0, 0, 1];

/// 是否是 HEVC 轨：`hev1` / `hvc1`，以及解密后仍带 `encv` 标记的加密轨。
///
/// 判定只写这一份。解码前置校验（`codec_probe::ensure_softdecode_supported`）
/// 和 [`read_parameter_sets`] 必须给出同一个答案，否则会出现「校验说不是 HEVC、
/// 解析却仍当 HEVC 处理」这种自相矛盾的失败路径。
pub fn is_hevc(codec: &str) -> bool {
    codec.starts_with("hev") || codec.starts_with("hvc") || codec == "encv"
}

/// 从 MP4 读出 HEVC 参数集，拼成 Annex-B 前缀。
///
/// 返回 `(Annex-B 前缀, 样本长度前缀字节数)`。
///
/// ⚠️ `length_size` 必须一起返回：MP4 里每个样本是「长度前缀 + NAL」，
///    解码器要的是 Annex-B。转换在 [`to_annexb`]，漏掉它解出来的是黑屏。
///
/// 返回空 vec 表示轨道里没有 `hvcC`（调用方应直接报错而不是继续解码）。
pub fn read_parameter_sets(path: &Path, track: &TrackInfo) -> AppResult<(Vec<u8>, usize)> {
    let (nalus, length_size) = read_parameter_set_nalus(path, track)?;
    let mut out = Vec::new();
    for nalu in &nalus {
        out.extend_from_slice(&START_CODE);
        out.extend_from_slice(nalu);
    }
    Ok((out, length_size))
}

/// [`read_parameter_sets`] 的裸形态：每条参数集一个 NAL（无起始码）。
///
/// 平台解码器（VideoToolbox 的 `CMVideoFormatDescriptionCreateFrom…ParameterSets`）
/// 要的就是这种「裸 NAL + 各自长度」的形态，走 Annex-B 反而要再剥一次起始码。
pub fn read_parameter_set_nalus(
    path: &Path,
    track: &TrackInfo,
) -> AppResult<(Vec<Vec<u8>>, usize)> {
    // ⚠️ CENC 加密后 stsd 的 format 是 `encv`，**不是** `hvc1`——解密只覆盖
    //    样本字节，不改这个字段（现版 JS 同样如此，改了反而会与 App 不一致）。
    //    真正的原始格式藏在 `frma` 里，但短剧的视频轨只有 HEVC 一种可能，
    //    所以这里把 `encv` 一并当作 HEVC 处理。
    if !is_hevc(&track.codec) {
        return Err(AppError::Media(format!(
            "不是 HEVC 轨（codec = {}）",
            track.codec
        )));
    }

    // 找 hvcC：它在 stsd 的 sample entry 里
    let sample_entry = find_hvc_c(path, track)?;
    let hvcc = read_box_payload(path, sample_entry.0, sample_entry.1)?;

    // hvcC 固定头 23 字节，其后才是参数集数组
    if hvcc.len() < 23 {
        return Err(AppError::Media("hvcC 长度异常".into()));
    }

    // hvcC 载荷布局（0-based，ISO/IEC 14496-15）：
    //   [21] = constantFrameRate(2) | numTemporalLayers(3)
    //          | temporalIdNested(1) | lengthSizeMinusOne(2)
    //   [22] = numOfArrays
    //   [23..] = arrays
    // 固定头 22 字节 + numOfArrays 1 字节 = 最小 23 字节。
    let length_size = (hvcc[21] & 0x03) as usize + 1;
    let num_arrays = hvcc[22] as usize;

    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut pos = 23usize;

    for _ in 0..num_arrays {
        if pos + 3 > hvcc.len() {
            break;
        }
        // array 头：array_completeness(1) + reserved(1) + NAL_unit_type(6) = 1 字节，
        // 随后 2 字节 numNalus
        let num_nalus = u16::from_be_bytes([hvcc[pos + 1], hvcc[pos + 2]]) as usize;
        pos += 3;

        for _ in 0..num_nalus {
            if pos + 2 > hvcc.len() {
                break;
            }
            let nalu_len = u16::from_be_bytes([hvcc[pos], hvcc[pos + 1]]) as usize;
            pos += 2;

            if pos + nalu_len > hvcc.len() {
                break;
            }
            out.push(hvcc[pos..pos + nalu_len].to_vec());
            pos += nalu_len;
        }
    }

    if out.is_empty() {
        return Err(AppError::Media("hvcC 里没有参数集".into()));
    }
    Ok((out, length_size))
}

/// 把一个 MP4 样本（长度前缀 NAL 串）转成 Annex-B。
///
/// `length_size` 来自 hvcC 的 `lengthSizeMinusOne + 1`，通常是 4。
/// 遇到长度为 0 或越界的 NAL 就停止——截断的样本不该把后面的字节当 NAL 读。
pub fn to_annexb(sample: &[u8], length_size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(sample.len() + 16);
    let mut pos = 0usize;
    while pos + length_size <= sample.len() {
        let mut len = 0usize;
        for i in 0..length_size {
            len = (len << 8) | sample[pos + i] as usize;
        }
        pos += length_size;
        if len == 0 || pos + len > sample.len() {
            break;
        }
        out.extend_from_slice(&START_CODE);
        out.extend_from_slice(&sample[pos..pos + len]);
        pos += len;
    }
    out
}

/// 定位 `hvcC` box，返回 `(载荷偏移, 载荷长度)`。
fn find_hvc_c(path: &Path, track: &TrackInfo) -> AppResult<(u64, u64)> {
    let mut head = Vec::new();
    {
        let meta = std::fs::metadata(path).map_err(|e| AppError::Io(e.to_string()))?;
        let len = std::cmp::min(meta.len(), 8 * 1024 * 1024) as usize;
        let mut f = std::fs::File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
        head.resize(len, 0);
        f.read_exact(&mut head)
            .map_err(|e| AppError::Io(e.to_string()))?;
    }

    // stbl 的位置与大小在解析轨道时（`collect_track`）就记在 track 上了，
    // 直接用——**不要在它自己的载荷范围里再找一次 stbl**，那里没有它自己。
    if track.stbl_size == 0 {
        return Err(AppError::Media("轨道没有 stbl".into()));
    }
    let stbl_end = track.stbl_offset + track.stbl_size;
    let stsd = find_box(&head, track.stbl_offset, stbl_end, "stsd")
        .ok_or_else(|| AppError::Media("找不到 stsd".into()))?;

    // stsd 是 FullBox：载荷开头 8 字节是 version+flags(4) 与 entry_count(4)，
    // 之后才是 sample entry 列表。`b.start` 是**载荷**起点，不是 box 起点。
    //
    // sample entry 的扩展区（hvcC / avcC / sinf）位于 VisualSampleEntry 载荷的
    // 78 字节之后——加密轨的 format 是 `encv`，但这部分布局与普通轨一致。
    for b in crate::domain::mp4::r#box::parse_boxes(&head, stsd.start + 8, stsd.start + stsd.size) {
        let entry_end = b.start + b.size;
        let inner = b.start + VISUAL_SAMPLE_ENTRY_PAYLOAD;
        if inner + 8 > entry_end {
            continue;
        }
        if let Some(hvcc) = find_box(&head, inner, entry_end, "hvcC") {
            return Ok((hvcc.start as u64, hvcc.size as u64));
        }
    }

    Err(AppError::Media(format!(
        "stsd 里没有 hvcC（format = {}）",
        String::from_utf8_lossy(&head[stsd.start + 12..stsd.start + 16])
    )))
}

/// 按偏移与长度读一段数据。
fn read_box_payload(path: &Path, offset: u64, size: u64) -> AppResult<Vec<u8>> {
    let mut f = std::fs::File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
    f.seek(SeekFrom::Start(offset))
        .map_err(|e| AppError::Io(e.to_string()))?;
    let mut buf = vec![0u8; size as usize];
    f.read_exact(&mut buf)
        .map_err(|e| AppError::Media(format!("读取 hvcC 失败: {e}")))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_annexb_converts_length_prefixed() {
        let sample = vec![0x00, 0x00, 0x03, 0xAA, 0xBB, 0xCC];
        assert_eq!(to_annexb(&sample, 3), vec![0, 0, 0, 1, 0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn to_annexb_handles_multiple_nals() {
        let sample = vec![
            0x00, 0x00, 0x02, 0x11, 0x22, //
            0x00, 0x00, 0x01, 0x33,
        ];
        let out = to_annexb(&sample, 3);
        assert_eq!(out, vec![0, 0, 0, 1, 0x11, 0x22, 0, 0, 0, 1, 0x33]);
    }

    #[test]
    fn to_annexb_stops_on_bad_length() {
        // 声明长度 100 但数据不足，应停止而不是 panic
        let sample = vec![0x00, 0x00, 0x64, 0x11, 0x22];
        let out = to_annexb(&sample, 3);
        assert!(out.is_empty());
    }

    #[test]
    fn to_annexb_handles_empty() {
        assert!(to_annexb(&[], 4).is_empty());
    }

    #[test]
    fn to_annexb_with_four_byte_lengths() {
        let sample = vec![0x00, 0x00, 0x00, 0x02, 0xAB, 0xCD];
        let out = to_annexb(&sample, 4);
        assert_eq!(out, vec![0, 0, 0, 1, 0xAB, 0xCD]);
    }

    #[test]
    fn is_hevc_accepts_every_transport_form() {
        for codec in ["hvc1", "hev1", "encv"] {
            assert!(is_hevc(codec), "{codec} 应被当作 HEVC");
        }
    }

    #[test]
    fn is_hevc_rejects_other_codecs() {
        for codec in ["avc1", "mp4a", "vvc1", ""] {
            assert!(!is_hevc(codec), "{codec} 不该被当作 HEVC");
        }
    }
}
