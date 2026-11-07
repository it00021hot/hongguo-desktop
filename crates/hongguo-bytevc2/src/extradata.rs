//! 配置记录 → Annex-B：对齐官方 `convertByteVC2Extradata`。
//!
//! 字节 1 的 bit0 = PTL 是否在场，bit1..3 = `lengthSizeMinusOne`。PTL 在场时按官方字节游标跳到
//! `num_arrays`，不是 ISO `vvcC` 的位域。

use hongguo_common::{AppError, AppResult};

const MIN_EXTRADATA: usize = 0x17; // 官方：`x2 > 0x16`
const START_CODE: [u8; 4] = [0, 0, 0, 1];

/// 从配置记录读出 NAL 长度前缀字节数（样本侧 `sample_to_annexb` 用）。
pub fn length_size_from_extradata(extradata: &[u8]) -> AppResult<usize> {
    if extradata.len() < 2 {
        return Err(AppError::Media("bytevc2 extradata 过短".into()));
    }
    let flags = extradata[1];
    Ok(((flags >> 1) & 0x07) as usize + 1)
}

/// 配置记录 → Annex-B 参数集（每条 NAL 前加起始码）。
pub fn annexb_from_extradata(extradata: &[u8]) -> AppResult<Vec<u8>> {
    if extradata.len() < MIN_EXTRADATA {
        return Err(AppError::Media("bytevc2 extradata 过短".into()));
    }

    let flags = extradata[1];
    let mut pos = if flags & 1 != 0 {
        arrays_after_ptl(extradata)?
    } else {
        2
    };

    if pos >= extradata.len() {
        return Err(AppError::Media("bytevc2 extradata 缺 num_arrays".into()));
    }
    let num_arrays = extradata[pos] as usize;
    pos += 1;

    let mut out = Vec::new();
    for _ in 0..num_arrays {
        if pos + 3 > extradata.len() {
            return Err(AppError::Media("bytevc2 extradata 数组头截断".into()));
        }
        let num_nalus = u16::from_be_bytes([extradata[pos + 1], extradata[pos + 2]]) as usize;
        pos += 3;
        for _ in 0..num_nalus {
            if pos + 2 > extradata.len() {
                return Err(AppError::Media("bytevc2 extradata NAL 长度截断".into()));
            }
            let nalu_len = u16::from_be_bytes([extradata[pos], extradata[pos + 1]]) as usize;
            pos += 2;
            if pos + nalu_len > extradata.len() {
                return Err(AppError::Media("bytevc2 extradata NAL 载荷越界".into()));
            }
            out.extend_from_slice(&START_CODE);
            out.extend_from_slice(&extradata[pos..pos + nalu_len]);
            pos += nalu_len;
        }
    }

    if out.is_empty() {
        return Err(AppError::Media("bytevc2 extradata 无参数集".into()));
    }
    Ok(out)
}

/// PTL 在场时，`num_arrays` 的下标。游标与 `libttmplayer` `convertByteVC2Extradata` 一致。
fn arrays_after_ptl(data: &[u8]) -> AppResult<usize> {
    let base = match data.get(4).copied() {
        Some(n) if n > 1 => n as usize,
        _ => 1,
    } - 1;
    let flag_at = base
        .checked_add(8)
        .ok_or_else(|| AppError::Media("bytevc2 PTL 溢出".into()))?;
    let mut bits = *data
        .get(flag_at)
        .ok_or_else(|| AppError::Media("bytevc2 PTL 截断".into()))?;
    let mut cursor = base
        .checked_add(9)
        .ok_or_else(|| AppError::Media("bytevc2 PTL 溢出".into()))?;
    for _ in 0..8 {
        if bits & 0x80 != 0 {
            cursor = cursor
                .checked_add(1)
                .ok_or_else(|| AppError::Media("bytevc2 PTL 溢出".into()))?;
        }
        bits = bits.wrapping_shl(1);
    }
    let profiles = *data
        .get(cursor)
        .ok_or_else(|| AppError::Media("bytevc2 PTL profiles 截断".into()))?
        as usize;
    let advance = if profiles == 0 {
        1
    } else {
        profiles
            .checked_mul(4)
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| AppError::Media("bytevc2 PTL profiles 溢出".into()))?
    };
    cursor = cursor
        .checked_add(advance)
        .and_then(|n| n.checked_add(8))
        .ok_or_else(|| AppError::Media("bytevc2 PTL 溢出".into()))?;
    if cursor >= data.len() {
        return Err(AppError::Media("bytevc2 extradata 缺 num_arrays".into()));
    }
    Ok(cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 无 PTL：`flags` bit0=0，lengthSizeMinusOne=3 → 长度前缀 4；一个数组一条 NAL。
    /// 官方要求整段 `> 0x16`，数组从 offset 2 起。
    fn minimal_extradata(nal: &[u8]) -> Vec<u8> {
        let mut v = vec![0u8, 0b0000_0110]; // [0]=cfg, [1]=flags
        v.push(1); // num_arrays @2
        v.push(0); // array completeness | nal type
        v.extend_from_slice(&(1u16).to_be_bytes());
        v.extend_from_slice(&(nal.len() as u16).to_be_bytes());
        v.extend_from_slice(nal);
        while v.len() <= 0x16 {
            v.push(0);
        }
        v
    }

    #[test]
    fn length_size_reads_flags() {
        let ed = minimal_extradata(&[0x00, 0x01, 0x02]);
        assert_eq!(length_size_from_extradata(&ed).unwrap(), 4);
    }

    #[test]
    fn annexb_wraps_parameter_sets() {
        let nal = [0x40, 0x01, 0xaa, 0xbb];
        let ed = minimal_extradata(&nal);
        let out = annexb_from_extradata(&ed).unwrap();
        assert_eq!(&out[..4], &[0, 0, 0, 1]);
        assert_eq!(&out[4..], &nal);
    }

    #[test]
    fn rejects_short_buffer() {
        assert!(annexb_from_extradata(&[0; 8]).is_err());
    }

    /// `flags=0x07`：PTL 在场，`data[4]=1`、存在位图为 0、无 sub-profile。
    /// `num_arrays` 在固定 8 字节尾巴之后。
    #[test]
    fn ptl_present_lands_on_arrays() {
        let mut raw = vec![0u8; 26];
        raw[0] = 1;
        raw[1] = 0x07;
        raw[4] = 1;
        raw[18] = 1;
        raw[19] = 0x8f;
        raw[20] = 0;
        raw[21] = 1;
        raw[22] = 0;
        raw[23] = 2;
        raw[24] = 0x00;
        raw[25] = 0x79;
        let out = annexb_from_extradata(&raw).unwrap();
        assert_eq!(out, [0, 0, 0, 1, 0x00, 0x79]);
        assert_eq!(length_size_from_extradata(&raw).unwrap(), 4);
    }

    #[test]
    fn official_bv2c_yields_parameter_sets() {
        let raw = include_bytes!("official_bv2c.bin");
        assert_eq!(raw[1], 0x07);
        let out = annexb_from_extradata(raw).unwrap();
        assert!(out.starts_with(&[0, 0, 0, 1]));
        assert!(out.len() > 4, "官方样本必须抽出参数集");
        assert_eq!(&out[4..6], &[0x00, 0x79]);
    }
}
