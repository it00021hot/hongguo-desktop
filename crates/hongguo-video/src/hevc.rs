//! HEVC 参数集读取：`hvcC` → Annex-B。MP4 里 HEVC 参数集存在 `hvcC` box（长度前缀格式），解码器要的是 Annex-B（起始码分隔）——
//! 少这一步解码器拿不到 SPS/PPS/VPS，**首个 IDR 帧必然解不出来**且不报错，只表现为一直黑屏。
//! `hvcC` 结构（ISO/IEC 14496-15）：configurationVersion…lengthSizeMinusOne(1) + numOfArrays(1) + arrays[]（array_completeness | NAL_unit_type | numNalus | nalus）；解码只关心 lengthSizeMinusOne 与 arrays。

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::r#box::find_box;
use crate::sample_table::TrackInfo;
use hongguo_common::{AppError, AppResult};

/// VisualSampleEntry 载荷长度（ISO/IEC 14496-15）：reserved(6)+data_ref_idx(2)+pre_defined(2)+reserved(2)+pre_defined[3](12)+width/height(4)+res(8)+reserved(4)+frame_count(2)+compressorname(32)+depth(2)+pre_defined(2)。
/// 之后的扩展区是 hvcC。
const VISUAL_SAMPLE_ENTRY_PAYLOAD: usize = 78;

/// Annex-B 起始码。
const START_CODE: [u8; 4] = [0, 0, 0, 1];

/// 是否是 HEVC 轨：`hev1` / `hvc1`，以及解密后仍带 `encv` 标记的加密轨。
/// 判定只写这一份：平台后端前置校验与 [`read_parameter_set_nalus`] 必须给出同一答案，否则出现自相矛盾的失败路径。
pub fn is_hevc(codec: &str) -> bool {
    codec.starts_with("hev") || codec.starts_with("hvc") || codec == "encv"
}

pub fn is_random_access_nal(nal: &[u8]) -> bool {
    nal.len() >= 2
        && nal[0] & 0x80 == 0
        && nal[1] & 7 != 0
        && (16..=23).contains(&((nal[0] >> 1) & 0x3f))
}

/// 从 MP4 读出 HEVC 参数集，每条一个 NAL（无起始码）。
/// 平台解码器（VideoToolbox 的 `CMVideoFormatDescriptionCreateFrom…ParameterSets`）要的就是这种形态，走 Annex-B 反而要再剥一次起始码。
pub fn read_parameter_set_nalus(
    path: &Path,
    track: &TrackInfo,
) -> AppResult<(Vec<Vec<u8>>, usize)> {
    // CENC 解密后仍可能保留 encv；原始格式位于 frma，参数集仍从 hvcC 读取。
    if !is_hevc(&track.codec) {
        return Err(AppError::Media(format!(
            "不是 HEVC 轨（codec = {}）",
            track.codec
        )));
    }

    let sample_entry = find_hvc_c(path, track)?;
    let hvcc = read_box_payload(path, sample_entry.0, sample_entry.1)?;
    param_nalus_from_hvcc(&hvcc)
}

/// 解析 hvcC **载荷**（box header 之后的部分）：参数集 NAL 数组 + 长度前缀字节数。
pub fn param_nalus_from_hvcc(hvcc: &[u8]) -> AppResult<(Vec<Vec<u8>>, usize)> {
    // hvcC 固定头 23 字节，其后才是参数集数组
    if hvcc.len() < 23 {
        return Err(AppError::Media("hvcC 长度异常".into()));
    }

    // hvcC 载荷布局（0-based，ISO/IEC 14496-15）：[21] = constantFrameRate(2)|numTemporalLayers(3)|temporalIdNested(1)|lengthSizeMinusOne(2)，
    // [22] = numOfArrays，[23..] = arrays；固定头 22 字节 + numOfArrays 1 字节 = 最小 23 字节
    let length_size = (hvcc[21] & 0x03) as usize + 1;
    let num_arrays = hvcc[22] as usize;

    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut pos = 23usize;

    for _ in 0..num_arrays {
        if pos + 3 > hvcc.len() {
            break;
        }
        // array 头：array_completeness(1) + reserved(1) + NAL_unit_type(6) = 1 字节，随后 2 字节 numNalus
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

/// 把一个 MP4 样本（长度前缀 NAL 串）转成 Annex-B；`length_size` 来自 hvcC 的 `lengthSizeMinusOne + 1`，通常是 4。
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
    find_hvc_c_in(&head, track)
}

/// 从已读取的文件头定位 hvcC。
fn find_hvc_c_in(data: &[u8], track: &TrackInfo) -> AppResult<(u64, u64)> {
    // stbl 的位置与大小在解析轨道时（`collect_track`）已记在 track 上，直接用——不要在它自己的载荷范围里再找一次 stbl
    if track.stbl_size == 0 {
        return Err(AppError::Media("轨道没有 stbl".into()));
    }
    let stbl_end = track.stbl_offset + track.stbl_size;
    let stsd = find_box(data, track.stbl_offset, stbl_end, "stsd")
        .ok_or_else(|| AppError::Media("找不到 stsd".into()))?;

    // stsd 是 FullBox：载荷开头 8 字节是 version+flags(4) 与 entry_count(4)，之后才是 sample entry 列表（`b.start` 是载荷起点）。
    // sample entry 扩展区（hvcC / avcC / sinf）位于 VisualSampleEntry 载荷 78 字节之后；加密轨 format 是 `encv`，但这部分布局与普通轨一致。
    for b in crate::r#box::parse_boxes(data, stsd.start + 8, stsd.start + stsd.size) {
        let entry_end = b.start + b.size;
        let inner = b.start + VISUAL_SAMPLE_ENTRY_PAYLOAD;
        if inner + 8 > entry_end {
            continue;
        }
        if let Some(hvcc) = find_box(data, inner, entry_end, "hvcC") {
            return Ok((hvcc.start as u64, hvcc.size as u64));
        }
    }

    Err(AppError::Media(format!(
        "stsd 里没有 hvcC（format = {}）",
        String::from_utf8_lossy(&data[stsd.start + 12..stsd.start + 16])
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
