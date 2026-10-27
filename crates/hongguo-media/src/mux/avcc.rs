//! Annex-B → AVCC 转换与 `avcC` 构造。
//!
//! MP4 里的 H.264 样本是**长度前缀**形态（AVCC）：每个 NAL 前置 4 字节大端长度。
//! 平台编码器给的是 Annex-B（起始码分隔），两者之间必须转换——少了这一步，
//! 播放器按 Annex-B 去解析 AVCC 样本，整条轨解不出画面，而且不会报错，
//! 只表现为一直黑屏。
//!
//! 参数集（SPS/PPS）**不剥离**：它们跟着关键帧一起进 `mdat`，播放器按 `avcC`
//! 里的那份解。带内参数集与 `avcC` 并存是合规的（`avc3` 的做法）。

use hongguo_video::r#box::build_box;

/// NAL 类型：序列参数集。
const NAL_SPS: u8 = 7;
/// NAL 类型：图像参数集。
const NAL_PPS: u8 = 8;

/// H.264 参数集（构造 `avcC` 用）。
#[derive(Debug, Clone)]
pub struct AvcConfig {
    /// 序列参数集
    pub sps: Vec<u8>,
    /// 图像参数集
    pub pps: Vec<u8>,
}

/// Annex-B → AVCC：每个 NAL 前置 4 字节大端长度。
///
/// 没有起始码时整段当成一个 NAL——空样本会让 `stsz` 写 0 长度，
/// 播放器读到就停，表现为「只出一帧」。
pub fn to_avcc(data: &[u8]) -> Vec<u8> {
    let nals = split(data);
    if nals.is_empty() {
        let mut out = Vec::with_capacity(data.len() + 4);
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(data);
        return out;
    }
    let mut out = Vec::with_capacity(data.len() + 4 * nals.len());
    for nal in nals {
        if nal.is_empty() {
            continue;
        }
        out.extend_from_slice(&(nal.len() as u32).to_be_bytes());
        out.extend_from_slice(nal);
    }
    out
}

/// 拆出 Annex-B 里的各个 NAL（不含起始码）。
fn split(data: &[u8]) -> Vec<&[u8]> {
    let mut nals = Vec::new();
    let mut cursor = 0usize;
    while let Some((at, len)) = find_start_code(data, cursor) {
        let start = at + len;
        let end = find_start_code(data, start).map_or(data.len(), |(next, _)| next);
        nals.push(&data[start..end]);
        cursor = end;
    }
    nals
}

/// 找下一个起始码：返回（位置, 长度）。
///
/// 4 字节形态（`00 00 00 01`）优先判定——否则它会被当成「3 字节起始码 +
/// 一个前导零」，NAL 的边界就错了一位。
fn find_start_code(data: &[u8], from: usize) -> Option<(usize, usize)> {
    if data.len() < 3 || from >= data.len() {
        return None;
    }
    let mut i = from;
    while i + 3 <= data.len() {
        if i + 4 <= data.len()
            && data[i] == 0
            && data[i + 1] == 0
            && data[i + 2] == 0
            && data[i + 3] == 1
        {
            return Some((i, 4));
        }
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            return Some((i, 3));
        }
        i += 1;
    }
    None
}

/// 从码流里取 SPS / PPS。
///
/// 参数集必须来自**第一帧**：播放器要靠 `avcC` 里的参数集解出第一个 IDR，
/// 而第一帧就是那个 IDR。缺 SPS 时 `avcC` 填不出 profile/level，缺 PPS 时
/// 根本解不出画面——两种都直接判失败，不拿默认值凑。
pub fn extract(data: &[u8]) -> Option<AvcConfig> {
    let mut sps: Option<Vec<u8>> = None;
    let mut pps: Option<Vec<u8>> = None;
    for nal in split(data) {
        if nal.is_empty() {
            continue;
        }
        match nal[0] & 0x1f {
            NAL_SPS if sps.is_none() => sps = Some(nal.to_vec()),
            NAL_PPS if pps.is_none() => pps = Some(nal.to_vec()),
            _ => {}
        }
        if sps.is_some() && pps.is_some() {
            break;
        }
    }
    // SPS 至少要有 NAL 头 + profile_idc + constraint_flags + level_idc 四字节
    let sps = sps?;
    if sps.len() < 4 {
        return None;
    }
    Some(AvcConfig { sps, pps: pps? })
}

/// 构造 `avcC` box。
///
/// profile / compatibility / level 取 SPS 的第 2~4 字节（NAL 头之后的三字节），
/// 那是 `profile_idc` / `constraint_flags` / `level_idc`，与 SPS 里的值一一对应。
pub fn avcc_box(config: &AvcConfig) -> Vec<u8> {
    let mut p = vec![
        1,             // configurationVersion
        config.sps[1], // AVCProfileIndication
        config.sps[2], // profile_compatibility
        config.sps[3], // AVCLevelIndication
        0xff,          // 6 bit reserved + lengthSizeMinusOne = 3（4 字节长度前缀）
        0xe1,          // 3 bit reserved + numOfSequenceParameterSets = 1
    ];
    p.extend_from_slice(&(config.sps.len() as u16).to_be_bytes());
    p.extend_from_slice(&config.sps);
    p.push(1); // numOfPictureParameterSets
    p.extend_from_slice(&(config.pps.len() as u16).to_be_bytes());
    p.extend_from_slice(&config.pps);
    build_box(b"avcC", &p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn annexb(nals: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for nal in nals {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(nal);
        }
        out
    }

    #[test]
    fn annexb_becomes_length_prefixed() {
        let data = annexb(&[&[0x67, 0x42], &[0x68, 0xce]]);
        let avcc = to_avcc(&data);
        assert_eq!(&avcc[0..4], &[0, 0, 0, 2]);
        assert_eq!(&avcc[4..6], &[0x67, 0x42]);
        assert_eq!(&avcc[6..10], &[0, 0, 0, 2]);
        assert_eq!(&avcc[10..12], &[0x68, 0xce]);
    }

    #[test]
    fn three_byte_start_codes_are_recognized() {
        let mut data = vec![0, 0, 1, 0x65, 0x11];
        data.extend_from_slice(&[0, 0, 1, 0x41, 0x22]);
        let nals = split(&data);
        assert_eq!(nals.len(), 2);
        assert_eq!(nals[0], &[0x65, 0x11]);
        assert_eq!(nals[1], &[0x41, 0x22]);
    }

    #[test]
    fn a_frame_without_start_codes_is_one_nal() {
        let avcc = to_avcc(&[0x41, 0x99]);
        assert_eq!(avcc, vec![0, 0, 0, 2, 0x41, 0x99]);
    }

    #[test]
    fn sps_and_pps_are_pulled_out() {
        let data = annexb(&[&[0x67, 0x64, 0x00, 0x1f], &[0x68, 0xeb], &[0x65, 0x88]]);
        let cfg = extract(&data).expect("应取到参数集");
        assert_eq!(cfg.sps, vec![0x67, 0x64, 0x00, 0x1f]);
        assert_eq!(cfg.pps, vec![0x68, 0xeb]);
    }

    #[test]
    fn missing_parameter_sets_are_rejected() {
        assert!(
            extract(&annexb(&[&[0x65, 0x88]])).is_none(),
            "只有 IDR 不够"
        );
        assert!(
            extract(&annexb(&[&[0x67, 0x64, 0x00, 0x1f]])).is_none(),
            "缺 PPS"
        );
        assert!(extract(&annexb(&[&[0x67, 0x64]])).is_none(), "SPS 太短");
    }

    #[test]
    fn avcc_records_the_sps_profile_and_level() {
        let cfg = AvcConfig {
            sps: vec![0x67, 0x64, 0x00, 0x1f],
            pps: vec![0x68, 0xeb],
        };
        let box_bytes = avcc_box(&cfg);
        // box 头 8 字节，之后是 configurationVersion + profile + compat + level
        assert_eq!(&box_bytes[8..12], &[1, 0x64, 0x00, 0x1f]);
        assert_eq!(box_bytes[12], 0xff, "4 字节长度前缀");
        assert_eq!(box_bytes[13], 0xe1, "1 组 SPS");
    }
}
