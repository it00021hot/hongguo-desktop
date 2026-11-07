//! 配置记录 → Annex-B：对齐官方 `convertByteVC2Extradata`（`vc2_dec.cpp`）。
//!
//! 布局与标准 `vvcC` 同族但更松：字节 1 的 bit0 = PTL 是否在场，bit1..3 = `lengthSizeMinusOne`
//! （官方用 3 bit；librempeg 的 CBS 拆包侧固定按 4 字节长度前缀读样本）。

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
    let mut pos = 2usize;

    if flags & 1 != 0 {
        pos = skip_ptl(extradata, pos)?;
    }

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

/// 跳过 PTL 段（官方路径：在 bit0 置位时走到 arrays 之前）。
/// 实现按 librempeg `cbs_h266_split_fragment` 的 VVCC PTL 跳读口径收敛，避免猜字段。
fn skip_ptl(data: &[u8], mut pos: usize) -> AppResult<usize> {
    // num_sublayers 等：至少还要 3 字节才能安全读完常见头
    if pos + 3 > data.len() {
        return Err(AppError::Media("bytevc2 PTL 截断".into()));
    }
    let ol = u16::from_be_bytes([data[pos], data[pos + 1]]);
    let num_sublayers = ((ol >> 4) & 7) as usize;
    pos += 2;
    pos += 1; // 对齐官方/librempeg 再跳 1

    if pos >= data.len() {
        return Err(AppError::Media("bytevc2 PTL constraint 截断".into()));
    }
    let num_bytes_constraint_info = (data[pos] & 0x3f) as usize;
    pos += 1;
    pos = pos
        .checked_add(2 + num_bytes_constraint_info)
        .ok_or_else(|| AppError::Media("bytevc2 PTL 溢出".into()))?;
    if pos > data.len() {
        return Err(AppError::Media("bytevc2 PTL constraint 越界".into()));
    }

    if num_sublayers > 1 {
        if pos >= data.len() {
            return Err(AppError::Media("bytevc2 PTL sublayer 截断".into()));
        }
        let b = data[pos];
        pos += 1;
        let mut present = 0usize;
        for i in 0..(num_sublayers - 1) {
            if (b >> (7 - i)) & 1 != 0 {
                present += 1;
            }
        }
        pos = pos
            .checked_add(present)
            .ok_or_else(|| AppError::Media("bytevc2 PTL sublayer 溢出".into()))?;
        if pos > data.len() {
            return Err(AppError::Media("bytevc2 PTL sublayer 越界".into()));
        }
    }

    // num_sub_profiles (1) + 4*n
    if pos >= data.len() {
        return Err(AppError::Media("bytevc2 PTL profiles 截断".into()));
    }
    let num_sub_profiles = data[pos] as usize;
    pos += 1;
    pos = pos
        .checked_add(num_sub_profiles.saturating_mul(4))
        .ok_or_else(|| AppError::Media("bytevc2 PTL profiles 溢出".into()))?;
    if pos > data.len() {
        return Err(AppError::Media("bytevc2 PTL profiles 越界".into()));
    }

    // 官方随后还有一段对齐到 arrays；剩余未知字节按「找到 arrays 计数」太脆。
    // librempeg 在非 header 路径不走这里；官方 convert 在 bit0=0 时直接 arrays。
    // bit0=1 的样本用完再收紧——先要求调用方给到 arrays 起点已对齐的缓冲。
    Ok(pos)
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
}
