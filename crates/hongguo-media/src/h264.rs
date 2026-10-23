//! H.264（AVC）参数集提取：avcC 记录 → SPS/PPS NAL 数组 + 长度前缀字节数。
//!
//! 与 [`crate::hevc`] 同构（那边喂 hvcC）：解码端两种形态各取所需——
//! VideoToolbox 要裸 NAL 数组、Media Foundation 要整条 avcC 记录
//!（`MF_MT_MPEG_SEQUENCE_HEADER`），所以两个出口都提供。
//!
//! avcC 载荷布局（ISO/IEC 14496-15 5.3.3.1，AVCDecoderConfigurationRecord）：
//!
//! ```text
//! [0]      configurationVersion (1)
//! [1..4]   AVCProfileIndication / profile_compatibility / AVCLevelIndication
//! [4]      lengthSizeMinusOne（低 2 位）
//! [5]      numOfSequenceParameterSets（低 5 位）
//! 之后     逐条 SPS：u16 长度 + 载荷
//! 之后      numOfPictureParameterSets(1) + 逐条 PPS：u16 长度 + 载荷
//! ```

use hongguo_common::{AppError, AppResult};
use hongguo_video::sample_table::TrackInfo;

use crate::hevc::VISUAL_SAMPLE_ENTRY_PAYLOAD;

/// codec 四字符码是否是 H.264（`avc1` 普通轨 / `avc3` 带内参数集轨）。
pub fn is_h264(codec: &str) -> bool {
    matches!(codec, "avc1" | "avc3" | "h264")
}

/// 从头部字节（moov 所在窗口）提取 H.264 参数集：返回（SPS+PPS NAL 数组,
/// 长度前缀字节数）。
pub fn param_nalus_from_head(head: &[u8], track: &TrackInfo) -> AppResult<(Vec<Vec<u8>>, usize)> {
    let avcc = avcc_payload_from_head(head, track)?;
    param_nalus_from_avcc(&avcc)
}

/// avcC 的**原始载荷**（box header 之后、未拆装的整体记录）——Media
/// Foundation 的 `MF_MT_MPEG_SEQUENCE_HEADER` 吃这份形态。
pub fn avcc_payload_from_head(head: &[u8], track: &TrackInfo) -> AppResult<Vec<u8>> {
    if !is_h264(&track.codec) {
        return Err(AppError::Media(format!(
            "不是 H.264 轨（codec = {}）",
            track.codec
        )));
    }
    let (offset, size) = find_avcc_in(head, track)?;
    let start = offset as usize;
    let end = start + size as usize;
    head.get(start..end)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| AppError::Media("avcC 越出头部字节范围（moov 不完整）".into()))
}

/// 解析 avcC 载荷：SPS/PPS NAL 数组 + 长度前缀字节数（1–4）。
pub fn param_nalus_from_avcc(avcc: &[u8]) -> AppResult<(Vec<Vec<u8>>, usize)> {
    if avcc.len() < 6 {
        return Err(AppError::Media("avcC 载荷过短".into()));
    }
    if avcc[0] != 1 {
        return Err(AppError::Media(format!("avcC 版本不认识: {}", avcc[0])));
    }
    let length_size = (avcc[4] & 0x03) as usize + 1;
    let mut nalus: Vec<Vec<u8>> = Vec::new();
    let mut pos = 6usize;

    // SPS 数组
    let num_sps = (avcc[5] & 0x1f) as usize;
    for _ in 0..num_sps {
        let len = read_u16(avcc, pos)?;
        pos += 2;
        let end = pos + len;
        nalus.push(
            avcc.get(pos..end)
                .ok_or_else(|| AppError::Media("avcC 的 SPS 越界".into()))?
                .to_vec(),
        );
        pos = end;
    }

    // PPS 数组
    let num_pps = *avcc
        .get(pos)
        .ok_or_else(|| AppError::Media("avcC 缺 PPS 计数".into()))? as usize;
    pos += 1;
    for _ in 0..num_pps {
        let len = read_u16(avcc, pos)?;
        pos += 2;
        let end = pos + len;
        nalus.push(
            avcc.get(pos..end)
                .ok_or_else(|| AppError::Media("avcC 的 PPS 越界".into()))?
                .to_vec(),
        );
        pos = end;
    }

    if nalus.is_empty() {
        return Err(AppError::Media("avcC 里没有参数集".into()));
    }
    Ok((nalus, length_size))
}

fn read_u16(data: &[u8], pos: usize) -> AppResult<usize> {
    let bytes = data
        .get(pos..pos + 2)
        .ok_or_else(|| AppError::Media("avcC 参数集长度越界".into()))?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]) as usize)
}

/// stsd 里找 avcC（与 [`crate::hevc`] 的 hvcC 查找同款：stbl 位置来自轨道
/// 解析，sample entry 载荷 78 字节之后是扩展区）。
fn find_avcc_in(data: &[u8], track: &TrackInfo) -> AppResult<(u64, u64)> {
    if track.stbl_size == 0 {
        return Err(AppError::Media("轨道没有 stbl".into()));
    }
    let stbl_end = track.stbl_offset + track.stbl_size;
    let stsd = hongguo_video::r#box::find_box(data, track.stbl_offset, stbl_end, "stsd")
        .ok_or_else(|| AppError::Media("找不到 stsd".into()))?;
    for b in hongguo_video::r#box::parse_boxes(data, stsd.start + 8, stsd.start + stsd.size) {
        let entry_end = b.start + b.size;
        let inner = b.start + VISUAL_SAMPLE_ENTRY_PAYLOAD;
        if inner + 8 > entry_end {
            continue;
        }
        if let Some(avcc) = hongguo_video::r#box::find_box(data, inner, entry_end, "avcC") {
            return Ok((avcc.start as u64, avcc.size as u64));
        }
    }
    Err(AppError::Media(format!(
        "stsd 里没有 avcC（format = {}）",
        String::from_utf8_lossy(&data[stsd.start + 12..stsd.start + 16])
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手搓一条 avcC：1 组 SPS + 1 组 PPS，长度前缀 4 字节。
    fn avcc_fixture(sps: &[u8], pps: &[u8]) -> Vec<u8> {
        let mut v = vec![1u8, 0x64, 0x00, 0x1f, 0xff, 0xe1];
        v.extend_from_slice(&(sps.len() as u16).to_be_bytes());
        v.extend_from_slice(sps);
        v.push(1);
        v.extend_from_slice(&(pps.len() as u16).to_be_bytes());
        v.extend_from_slice(pps);
        v
    }

    #[test]
    fn parses_sps_pps_and_length_size() {
        let avcc = avcc_fixture(&[0x67, 0x64, 0x00, 0x1f], &[0x68, 0xeb, 0xec]);
        let (nalus, length_size) = param_nalus_from_avcc(&avcc).expect("解析 avcC");
        assert_eq!(length_size, 4, "lengthSizeMinusOne=3 → 4 字节前缀");
        assert_eq!(nalus.len(), 2, "SPS + PPS");
        assert_eq!(nalus[0], vec![0x67, 0x64, 0x00, 0x1f]);
        assert_eq!(nalus[1], vec![0x68, 0xeb, 0xec]);
    }

    #[test]
    fn rejects_short_and_bad_version() {
        assert!(param_nalus_from_avcc(&[1, 2, 3]).is_err(), "过短拒绝");
        let mut bad = avcc_fixture(&[0x67], &[0x68]);
        bad[0] = 2;
        assert!(param_nalus_from_avcc(&bad).is_err(), "版本不是 1 拒绝");
    }

    #[test]
    fn rejects_truncated_param_set() {
        let mut avcc = avcc_fixture(&[0x67, 0x64], &[0x68]);
        avcc.truncate(8); // 长度声称 2 字节，实际载荷被截断
        assert!(param_nalus_from_avcc(&avcc).is_err());
    }

    #[test]
    fn two_byte_length_prefix_is_reported() {
        let mut avcc = avcc_fixture(&[0x67], &[0x68]);
        avcc[4] = 0xfd; // 低 2 位 = 1 → 2 字节前缀
        let (_, length_size) = param_nalus_from_avcc(&avcc).expect("解析");
        assert_eq!(length_size, 2);
    }
}
