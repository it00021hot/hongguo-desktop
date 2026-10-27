//! `esds` → AudioSpecificConfig（AAC 解码器的 extradata）。
//!
//! 音频样本入口（`mp4a`）里的 `esds` box 是一串 MPEG-4 描述符：
//!
//! ```text
//! ES_Descriptor(0x03)
//! └─ DecoderConfigDescriptor(0x04)
//!    └─ DecoderSpecificInfo(0x05)   ← AudioSpecificConfig，裸 AAC 解码必需
//! ```
//!
//! 裸 AAC 解码器不走 MP4 demuxer 时，没有这串字节就无法初始化——
//! 解出来的是噪声或直接拒开。描述符长度是变长编码（每字节 7 位，
//! 高位续传），遍历必须按这个读。

use crate::r#box::parse_boxes;
use hongguo_common::{AppError, AppResult};

/// 从 `esds` box 载荷提取 DecoderSpecificInfo（AudioSpecificConfig）。
///
/// 载荷开头是 FullBox 的 `version+flags(4)`，其后才是描述符串。0x05 嵌在
/// 0x04 体内、0x04 又嵌在 0x03 体内——逐层下潜，不在一层找。
pub fn decoder_specific_info(esds_payload: &[u8]) -> AppResult<Vec<u8>> {
    if esds_payload.len() < 4 {
        return Err(AppError::Decrypt("esds 载荷过短".into()));
    }
    let end = esds_payload.len();
    let mut pos = 4usize; // version + flags
    while let Some((tag, body_start, body_end)) = next_descriptor(esds_payload, pos, end) {
        match tag {
            // ES_Descriptor：ES_ID(2) + stream priority/flags(1)，flags 置位
            // 时跟可选字段（streamDependence 2 / OCR 2 / reserved 1）
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

/// 在描述符串 `[pos, end)` 里找指定 tag 的描述符，回其载荷区间。
///
/// `fixed_head` 是调用方要跳过的固定字段长度（如 0x04 的 13 字节头）。
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
///
/// 音频 sample entry 的固定体长度各家不一致（ISO 24 字节、QuickTime 变体到
/// 44），固定字段里的零字节还会让逐盒解析提前误判 size=0——不按布局算，
/// 在 entry 扩展区里**容错扫描** `esds` fourcc：命中处向前读 size 字段并
/// 校验（≥8 且不越 entry），音频 entry 里这个四字符码不存在歧义。
pub fn decoder_config_from_stsd(data: &[u8], start: usize, size: usize) -> Option<Vec<u8>> {
    // stsd 每轨只装一个 sample entry：取第一个，找不到就是没有
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
            // size 覆盖整个 box 且不越 entry 才认
            if size >= 12 && i - 4 + size <= entry_end {
                let payload = data.get(i + 4..i - 4 + size)?;
                return decoder_specific_info(payload).ok();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手工拼一个真实形态的 esds：ASC 两字节（AAC-LC 44100 双声道的典型值）。
    #[test]
    fn extracts_specific_info_from_nested_descriptors() {
        let asc = [0x12, 0x10];
        // DecoderConfigDescriptor 体：objType(1)+streamType(1)+bufferSizeDB(3)+maxBr(4)+avgBr(4)
        let dcd_body: Vec<u8> = [
            [0x40u8, 0x15].as_slice(),
            &[0, 0, 0],
            &[0, 0, 1, 0],
            &[0, 0, 1, 0],
        ]
        .concat();
        // 0x04 的长度含整个 0x05 子描述符（tag 1 + len 1 + 载荷 2）
        let dcd: Vec<u8> = [[0x04].as_slice(), &[(dcd_body.len() + 2 + asc.len()) as u8]]
            .concat()
            .into_iter()
            .chain(dcd_body.iter().copied())
            .chain([0x05, asc.len() as u8])
            .chain(asc.iter().copied())
            .collect();
        let es_body: Vec<u8> = [
            [0x03].as_slice(),
            &[dcd.len() as u8 + 3],
            &[0x00, 0x01, 0x00],
        ]
        .concat()
        .into_iter()
        .chain(dcd.iter().copied())
        .collect();
        let mut payload = vec![0, 0, 0, 0]; // version + flags
        payload.extend_from_slice(&es_body);

        let out = decoder_specific_info(&payload).expect("应解出 ASC");
        assert_eq!(out, asc);
    }

    /// 变长长度（两字节形态）也能读。
    #[test]
    fn reads_extended_length_varints() {
        // tag 0x05，长度 200（0x81 0x48：((1)<<7)|0x48 = 200）
        let mut data = vec![0x05, 0x81, 0x48];
        data.extend(std::iter::repeat_n(0xabu8, 200));
        let (tag, s, e) = next_descriptor(&data, 0, data.len()).expect("应读到描述符");
        assert_eq!(tag, 0x05);
        assert_eq!((s, e), (3, 203));
    }

    /// stsd 全量路径：mp4a entry 里的 esds（套在真实布局里）。
    #[test]
    fn finds_esds_inside_stsd_entry() {
        let asc = [0x11, 0x90];
        let mut entry = Vec::new();
        entry.extend_from_slice(&[0u8; 6]); // SampleEntry reserved
        entry.extend_from_slice(&[0, 1]); // data_reference_index
        entry.extend_from_slice(&[0, 0, 0, 0]); // reserved
        entry.extend_from_slice(&[0, 2]); // channels
        entry.extend_from_slice(&[0, 16]); // sample size
        entry.extend_from_slice(&[0, 0]); // pre_defined
        entry.extend_from_slice(&[0, 0]); // reserved
        entry.extend_from_slice(&[0, 0, 17, 113]); // samplerate 16.16（44100）
        // esds box：version+flags(4) + tag 0x03 + len + ES_ID(2)+flags(1)
        //   + tag 0x04 + len + 13 字节固定头 + tag 0x05 + len + asc
        let dcd_len = 13 + 2 + asc.len() as u8;
        let inner: Vec<u8> = [
            [0x03u8].as_slice(),
            &[3 + 1 + 1 + dcd_len],
            &[0x00, 0x01, 0x00],
            &[0x04],
            &[dcd_len],
            &[0x40, 0x15, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0],
        ]
        .concat()
        .into_iter()
        .chain([0x05, asc.len() as u8])
        .chain(asc.iter().copied())
        .collect();
        // box size = 头(8) + version/flags(4) + 描述符串
        let esds_size = (8 + 4 + inner.len()) as u32;
        entry.extend_from_slice(&esds_size.to_be_bytes());
        entry.extend_from_slice(b"esds");
        entry.extend_from_slice(&[0, 0, 0, 0]);
        entry.extend_from_slice(&inner);

        let mut stsd = Vec::new();
        stsd.extend_from_slice(&[0, 0, 0, 0]); // version + flags
        stsd.extend_from_slice(&[0, 0, 0, 1]); // entry_count
        stsd.extend_from_slice(&((8 + entry.len()) as u32).to_be_bytes());
        stsd.extend_from_slice(b"mp4a");
        stsd.extend_from_slice(&entry);

        let out = decoder_config_from_stsd(&stsd, 0, stsd.len()).expect("应找到 esds");
        assert_eq!(out, asc);
    }
}
