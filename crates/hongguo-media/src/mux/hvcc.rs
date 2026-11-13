use hongguo_common::{AppError, AppResult};
use hongguo_video::r#box::build_box;

const VPS: u8 = 32;
const PPS: u8 = 34;

#[derive(Debug, Clone)]
pub struct HevcConfig {
    header: [u8; 23],
    arrays: [Vec<Vec<u8>>; 3],
}

pub fn extract(data: &[u8]) -> AppResult<HevcConfig> {
    let mut arrays: [Vec<Vec<u8>>; 3] = Default::default();
    for nal in split(data) {
        let kind = nal_type(nal)?;
        if !(VPS..=PPS).contains(&kind) {
            continue;
        }
        if nal.len() < 3 || nal.len() > u16::MAX as usize {
            return Err(invalid("HEVC 参数集长度无效"));
        }
        let sets = &mut arrays[usize::from(kind - VPS)];
        if !sets.iter().any(|set| set == nal) {
            if sets.len() == usize::from(u16::MAX) {
                return Err(invalid("HEVC 参数集数量超出容器上限"));
            }
            sets.push(nal.to_vec());
        }
    }
    if arrays.iter().any(Vec::is_empty) {
        return Err(invalid("HEVC 视频缺少 VPS/SPS/PPS"));
    }
    let header = sps_header(&arrays[1][0])?;
    for sps in &arrays[1][1..] {
        if sps_header(sps)? != header {
            return Err(invalid("HEVC SPS 配置不一致"));
        }
    }
    Ok(HevcConfig { header, arrays })
}

pub fn hvcc_box(config: &HevcConfig) -> Vec<u8> {
    let mut payload = config.header.to_vec();
    for (index, sets) in config.arrays.iter().enumerate() {
        payload.push(0x80 | (VPS + index as u8));
        payload.extend((sets.len() as u16).to_be_bytes());
        for nal in sets {
            payload.extend((nal.len() as u16).to_be_bytes());
            payload.extend(nal);
        }
    }
    build_box(b"hvcC", &payload)
}

pub fn to_sample(data: &[u8]) -> AppResult<Vec<u8>> {
    let mut sample = Vec::with_capacity(data.len());
    let mut has_picture = false;
    for nal in split(data) {
        let kind = nal_type(nal)?;
        if (VPS..=PPS).contains(&kind) {
            continue;
        }
        has_picture |= kind < VPS;
        let length = u32::try_from(nal.len()).map_err(|_| invalid("HEVC NAL 超过容器上限"))?;
        sample.extend(length.to_be_bytes());
        sample.extend(nal);
    }
    if !has_picture {
        return Err(invalid("HEVC 样本缺少图像 NAL"));
    }
    Ok(sample)
}

fn nal_type(nal: &[u8]) -> AppResult<u8> {
    if nal.len() < 2 || nal[0] & 0x80 != 0 || nal[1] & 7 == 0 {
        return Err(invalid("HEVC NAL 头无效"));
    }
    Ok((nal[0] >> 1) & 0x3f)
}

pub(super) fn split(data: &[u8]) -> Vec<&[u8]> {
    let mut nals = Vec::new();
    let mut cursor = 0;
    while let Some((at, length)) = start_code(data, cursor) {
        let start = at + length;
        let end = start_code(data, start).map_or(data.len(), |(next, _)| next);
        let mut trimmed = end;
        while trimmed > start && data[trimmed - 1] == 0 {
            trimmed -= 1;
        }
        if trimmed > start {
            nals.push(&data[start..trimmed]);
        }
        cursor = end;
    }
    if nals.is_empty() && !data.is_empty() {
        nals.push(data);
    }
    nals
}

fn start_code(data: &[u8], from: usize) -> Option<(usize, usize)> {
    for index in from..data.len().saturating_sub(2) {
        if data[index..].starts_with(&[0, 0, 0, 1]) {
            return Some((index, 4));
        }
        if data[index..].starts_with(&[0, 0, 1]) {
            return Some((index, 3));
        }
    }
    None
}

fn sps_header(sps: &[u8]) -> AppResult<[u8; 23]> {
    let rbsp = unescape(&sps[2..]);
    let mut bits = Bits {
        data: &rbsp,
        position: 0,
    };
    bits.read(4)?;
    let layers = bits.read(3)? as u8;
    let nested = bits.read(1)? as u8;
    if layers > 6 {
        return Err(invalid("HEVC 时域层数无效"));
    }
    let mut header = [0_u8; 23];
    header[0] = 1;
    for byte in &mut header[1..13] {
        *byte = bits.read(8)? as u8;
    }
    let mut present = Vec::new();
    for _ in 0..layers {
        present.push((bits.read(1)? != 0, bits.read(1)? != 0));
    }
    if layers > 0 {
        bits.read(usize::from(8 - layers) * 2)?;
    }
    for (profile, level) in present {
        if profile {
            bits.read(64)?;
            bits.read(24)?;
        }
        if level {
            bits.read(8)?;
        }
    }
    bits.ue()?;
    let chroma = bits.ue()?;
    if chroma > 3 {
        return Err(invalid("HEVC 色度格式无效"));
    }
    if chroma == 3 {
        bits.read(1)?;
    }
    bits.ue()?;
    bits.ue()?;
    if bits.read(1)? != 0 {
        for _ in 0..4 {
            bits.ue()?;
        }
    }
    let luma = bits.ue()?;
    let chroma_depth = bits.ue()?;
    if luma > 7 || chroma_depth > 7 {
        return Err(invalid("HEVC 位深超出 hvcC 范围"));
    }
    header[13..15].copy_from_slice(&0xf000_u16.to_be_bytes());
    header[15] = 0xfc;
    header[16] = 0xfc | chroma as u8;
    header[17] = 0xf8 | luma as u8;
    header[18] = 0xf8 | chroma_depth as u8;
    header[21] = ((layers + 1) << 3) | (nested << 2) | 3;
    header[22] = 3;
    Ok(header)
}

fn unescape(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    let mut zeros = 0;
    for &byte in data {
        if zeros >= 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        result.push(byte);
        zeros = if byte == 0 { zeros + 1 } else { 0 };
    }
    result
}

struct Bits<'a> {
    data: &'a [u8],
    position: usize,
}

impl Bits<'_> {
    fn read(&mut self, count: usize) -> AppResult<u64> {
        if count > 64 || self.position + count > self.data.len() * 8 {
            return Err(invalid("HEVC SPS 截断"));
        }
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1)
                | u64::from((self.data[self.position / 8] >> (7 - self.position % 8)) & 1);
            self.position += 1;
        }
        Ok(value)
    }

    fn ue(&mut self) -> AppResult<u64> {
        let mut leading = 0;
        while self.read(1)? == 0 {
            leading += 1;
            if leading > 31 {
                return Err(invalid("HEVC Exp-Golomb 超出范围"));
            }
        }
        Ok((1_u64 << leading) - 1 + self.read(leading)?)
    }
}

fn invalid(message: &str) -> AppError {
    AppError::Media(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mux::fixtures;

    #[test]
    fn hvcc_records_real_main_profile_and_complete_parameter_arrays() {
        let config = extract(&fixtures::first()).unwrap();
        let bytes = hvcc_box(&config);
        let payload = &bytes[8..];
        assert_eq!(payload[0], 1);
        assert_eq!(payload[1] & 31, 1);
        assert_eq!(payload[12], 30);
        assert_eq!(payload[16] & 3, 1);
        assert_eq!(payload[17] & 7, 0);
        assert_eq!(payload[18] & 7, 0);
        assert_eq!(payload[21] & 3, 3);
        let (sets, length) = crate::hevc::param_nalus_from_hvcc(payload).unwrap();
        assert_eq!(length, 4);
        assert_eq!(sets, [fixtures::VPS, fixtures::SPS, fixtures::PPS]);
        assert_eq!(payload[23], 0x80 | VPS);
    }

    #[test]
    fn sample_removes_parameter_sets_but_preserves_picture_and_sei() {
        let data = fixtures::annexb(&[
            fixtures::VPS,
            fixtures::SPS,
            fixtures::PPS,
            &[0x4e, 1, 0x80],
            fixtures::IDR,
        ]);
        assert_eq!(
            to_sample(&data).unwrap(),
            [
                0, 0, 0, 3, 0x4e, 1, 0x80, 0, 0, 0, 5, 0x28, 1, 0xaf, 0x1d, 0x80
            ]
        );
        assert_eq!(
            to_sample(&[0, 0, 1, 0x02, 1, 0x80]).unwrap(),
            [0, 0, 0, 3, 0x02, 1, 0x80]
        );
    }

    #[test]
    fn missing_oversized_and_truncated_parameter_sets_fail() {
        assert!(extract(&[]).is_err());
        assert!(extract(&fixtures::annexb(&[fixtures::VPS, fixtures::SPS])).is_err());
        assert!(
            extract(&fixtures::annexb(&[
                fixtures::VPS,
                &[0x42, 1, 1],
                fixtures::PPS
            ]))
            .is_err()
        );
        let mut huge = vec![0x80; usize::from(u16::MAX) + 1];
        huge[0..2].copy_from_slice(&[0x44, 1]);
        assert!(extract(&fixtures::annexb(&[fixtures::VPS, fixtures::SPS, &huge])).is_err());
        assert!(
            to_sample(&fixtures::annexb(&[
                fixtures::VPS,
                fixtures::SPS,
                fixtures::PPS
            ]))
            .is_err()
        );
        assert!(extract(&[0, 0, 1, 0x67, 0x64, 0, 0x1f]).is_err());
        assert!(to_sample(&[0, 0, 1, 0x65, 0x88]).is_err());
    }

    #[test]
    fn duplicate_parameter_sets_are_stored_once() {
        let data = fixtures::annexb(&[
            fixtures::VPS,
            fixtures::SPS,
            fixtures::PPS,
            fixtures::VPS,
            fixtures::SPS,
            fixtures::PPS,
        ]);
        let config = extract(&data).unwrap();
        assert_eq!(
            config.arrays.iter().map(Vec::len).collect::<Vec<_>>(),
            [1, 1, 1]
        );
    }

    #[test]
    fn actual_main10_sps_preserves_profile_and_both_bit_depths() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../hongguo-softdec/tests/fixtures/hevc-main10-128x72-hev1-aac.mp4");
        let demux = crate::demux::demux_file(&path).unwrap();
        let (sets, _) =
            crate::hevc::read_parameter_set_nalus(&path, &demux.video_track().unwrap().info)
                .unwrap();
        let refs: Vec<_> = sets.iter().map(Vec::as_slice).collect();
        let bytes = hvcc_box(&extract(&fixtures::annexb(&refs)).unwrap());
        assert_eq!(bytes[8 + 1] & 31, 2);
        assert_eq!(bytes[8 + 17] & 7, 2);
        assert_eq!(bytes[8 + 18] & 7, 2);
    }

    #[test]
    fn all_hevc_irap_types_are_sync_and_avc_is_rejected() {
        for kind in 0..64 {
            assert_eq!(
                crate::hevc::is_random_access_nal(&[kind << 1, 1]),
                (16..=23).contains(&kind)
            );
        }
        assert!(!crate::hevc::is_random_access_nal(&[0x26, 0]));
        assert!(!crate::hevc::is_random_access_nal(&[0x65, 0x88]));
    }
}
