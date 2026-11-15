//! `esds` → AudioSpecificConfig（AAC 解码器的 extradata）。
//! 描述符串：ES_Descriptor(0x03) ⊃ DecoderConfigDescriptor(0x04) ⊃ DecoderSpecificInfo(0x05)；
//! 裸 AAC 解码器不走 MP4 demuxer 时没有这串字节无法初始化。描述符长度是变长编码（每字节 7 位、高位续传），遍历必须按这个读。

use crate::r#box::parse_boxes;
use hongguo_common::{AppError, AppResult};

/// 从 `esds` box 载荷提取 DecoderSpecificInfo（AudioSpecificConfig）：载荷开头是 FullBox 的 `version+flags(4)`。
/// 0x05 嵌在 0x04 体内、0x04 又嵌在 0x03 体内——逐层下潜，不在一层找。
pub fn decoder_specific_info(esds_payload: &[u8]) -> AppResult<Vec<u8>> {
    if esds_payload.len() < 4 {
        return Err(AppError::Decrypt("esds 载荷过短".into()));
    }
    let end = esds_payload.len();
    let mut pos = 4usize; // version + flags
    while let Some((tag, body_start, body_end)) = next_descriptor(esds_payload, pos, end) {
        match tag {
            // ES_Descriptor：ES_ID(2) + stream priority/flags(1)，flags 置位时跟可选字段（streamDependence 2 / OCR 2 / reserved 1）
            0x03 => {
                let mut p = body_start + 3;
                if p <= body_end {
                    let flags = esds_payload[body_start + 2];
                    if flags & 0x80 != 0 {
                        p += 2;
                    }
                    if flags & 0x40 != 0 {
                        p += 2;
                    }
                    if flags & 0x20 != 0 {
                        p += 1;
                    }
                    if let Some(asc) = find_in_body(esds_payload, p, body_end, 0x04, 13)
                        .and_then(|(s, e)| find_in_body(esds_payload, s, e, 0x05, 0))
                    {
                        return Ok(esds_payload[asc.0..asc.1].to_vec());
                    }
                }
            }
            // 不规范的封装把 DecoderConfigDescriptor 放在顶层：照样认
            0x04 => {
                if let Some(asc) = find_in_body(esds_payload, body_start + 13, body_end, 0x05, 0) {
                    return Ok(esds_payload[asc.0..asc.1].to_vec());
                }
            }
            _ => {}
        }
        pos = body_end;
    }
    Err(AppError::Decrypt(
        "esds 里没有 DecoderSpecificInfo（DecoderConfigDescriptor 之下）".into(),
    ))
}

/// 在描述符串 `[pos, end)` 里找指定 tag 的描述符，回其载荷区间；`fixed_head` 是调用方要跳过的固定字段长度（如 0x04 的 13 字节头）。
fn find_in_body(
    data: &[u8],
    mut pos: usize,
    end: usize,
    tag: u8,
    fixed_head: usize,
) -> Option<(usize, usize)> {
    while let Some((t, body_start, body_end)) = next_descriptor(data, pos, end) {
        if t == tag {
            return Some((body_start + fixed_head, body_end));
        }
        pos = body_end;
    }
    None
}

/// 读一个描述符：`(tag, 载荷起, 载荷止)`。长度是 0x80 续传的变长编码。
fn next_descriptor(data: &[u8], pos: usize, end: usize) -> Option<(u8, usize, usize)> {
    if pos + 2 > end {
        return None;
    }
    let tag = data[pos];
    let mut len = 0usize;
    let mut cursor = pos + 1;
    loop {
        if cursor >= end {
            return None;
        }
        let byte = data[cursor];
        cursor += 1;
        len = (len << 7) | (byte & 0x7f) as usize;
        if byte & 0x80 == 0 {
            break;
        }
    }
    let body_end = cursor + len;
    if body_end > end {
        return None;
    }
    Some((tag, cursor, body_end))
}

/// 从轨道所在的 stsd 载荷里找到 `esds` 并提取 AudioSpecificConfig。
/// 音频 sample entry 固定体长度各家不一（ISO 24 / QuickTime 变体到 44），固定字段的零字节还会让逐盒解析误判 size=0——
/// 所以不按布局算，在 entry 扩展区**容错扫描** `esds` fourcc：命中处向前读 size 并校验（≥8 且不越 entry）。
pub fn decoder_config_from_stsd(data: &[u8], start: usize, size: usize) -> Option<Vec<u8>> {
    let b = parse_boxes(data, start + 8, start + size)
        .into_iter()
        .next()?;
    let entry_end = b.start + b.size;
    // 扩展区从固定体的保守下限（ISO 24）起扫
    let from = b.start + 24;
    if from + 12 > entry_end {
        return None;
    }
    for i in from..=entry_end - 12 {
        if &data[i..i + 4] == b"esds" {
            let size =
                u32::from_be_bytes([data[i - 4], data[i - 3], data[i - 2], data[i - 1]]) as usize;
            if size >= 12 && i - 4 + size <= entry_end {
                let payload = data.get(i + 4..i - 4 + size)?;
                return decoder_specific_info(payload).ok();
            }
        }
    }
    None
}
