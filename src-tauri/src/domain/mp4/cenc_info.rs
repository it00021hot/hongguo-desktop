//! CENC 辅助信息：`saiz`（每样本附加信息大小）与 `senc`（逐样本 IV）。
//!
//! 与 [`super::sample_table`] 分文件：那边是「样本在哪、多大」，
//! 这边是「怎么解密」，两套字段互不相干，放一起反而看不清。

use super::sample_table::TrackInfo;

/// 从 `saiz` 读每个样本的附加信息大小。
pub(super) fn read_saiz(data: &[u8], start: usize, size: usize, info: &mut TrackInfo) {
    if size < 8 {
        return;
    }
    // saiz 载荷布局：version/flags(4) | default_sample_info_size(1) | sample_count(4) | sizes...
    if size < 9 {
        return;
    }
    let default_size = data[start + 4];
    let count = u32::from_be_bytes([
        data[start + 5],
        data[start + 6],
        data[start + 7],
        data[start + 8],
    ]);
    if count == 0 {
        return;
    }
    let n = count as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let at = start + 9 + i;
        if at >= start + size {
            out.push(default_size);
        } else {
            out.push(data[at]);
        }
    }
    info.aux_sizes = out;
}

/// 从 `senc` 读每个样本的 IV。
///
/// ⚠️ **IV 是 8 字节**，步长也是 8。平台封装时如此（现版 JS 按 `i * 8` 取），
///    按 CENC 规范的 16 字节读会整体错位——不报错，但解出来全是噪声。
pub(super) fn read_senc(data: &[u8], start: usize, size: usize, info: &mut TrackInfo) {
    if size < 8 {
        return;
    }
    // version+flags 已占 4 字节，sample_count 紧随其后。
    // 现版 JS 同样不按 version 分支，这里保持一致。
    let count_at = start + 4;
    if count_at + 4 > start + size {
        return;
    }
    let count = u32::from_be_bytes([
        data[count_at],
        data[count_at + 1],
        data[count_at + 2],
        data[count_at + 3],
    ]) as usize;

    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = count_at + 4 + i * 8;
        if at + 8 > start + size {
            break;
        }
        let mut iv = [0u8; 8];
        iv.copy_from_slice(&data[at..at + 8]);
        out.push(iv);
    }
    if !out.is_empty() {
        info.sample_ivs = out;
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::domain::mp4::r#box::build_box;

    /// 构造 FullBox：`version(1) + flags(3)` 由本函数写入，
    /// `payload` 只放紧随其后的字段（如 sample_size / count / 数据）。
    fn full_box(kind: &str, version: u8, flags: u8, payload: &[u8]) -> Vec<u8> {
        let mut body = vec![version, flags, 0, 0];
        body.extend_from_slice(payload);
        let k: [u8; 4] = kind.as_bytes().try_into().unwrap_or([0; 4]);
        build_box(&k, &body)
    }

    #[test]
    fn reads_saiz_sizes() {
        // saiz 载荷：flags 由 full_box 写入，接着 default_sample_info_size(1) | count(4) | sizes
        let mut payload = vec![0u8]; // default_sample_info_size = 0
        payload.extend_from_slice(&2u32.to_be_bytes()); // sample_count
        payload.push(8);
        payload.push(16);
        let saiz = full_box("saiz", 0, 0, &payload);

        let mut info = TrackInfo::default();
        read_saiz(&saiz, 8, saiz.len() - 8, &mut info);
        assert_eq!(info.aux_sizes, vec![8, 16]);
    }

    #[test]
    fn reads_senc_ivs() {
        // ⚠️ IV 是 **8 字节**，步长也是 8（平台封装如此，现版 JS 同样按 `i * 8` 取）。
        // 按 CENC 规范的 16 字节读会整体错位，解密出来全是噪声且不报错。
        let mut payload = 2u32.to_be_bytes().to_vec(); // sample_count
        payload.extend_from_slice(&[0x11u8; 8]);
        payload.extend_from_slice(&[0x22u8; 8]);
        let senc = full_box("senc", 0, 0, &payload);

        let mut info = TrackInfo::default();
        read_senc(&senc, 8, senc.len() - 8, &mut info);
        assert_eq!(info.sample_ivs.len(), 2);
        assert_eq!(info.sample_ivs[0], [0x11u8; 8]);
        assert_eq!(info.sample_ivs[1], [0x22u8; 8]);
    }

    #[test]
    fn senc_truncation_stops_without_panic() {
        // 声明 4 个 IV 但只给 2 个，必须停在给定的数据上而不是越界读
        let mut payload = 4u32.to_be_bytes().to_vec();
        payload.extend_from_slice(&[0x33u8; 8]);
        payload.extend_from_slice(&[0x44u8; 8]);
        let senc = full_box("senc", 0, 0, &payload);

        let mut info = TrackInfo::default();
        read_senc(&senc, 8, senc.len() - 8, &mut info);
        assert_eq!(info.sample_ivs.len(), 2);
    }

    #[test]
    fn truncated_boxes_do_not_panic() {
        let mut info = TrackInfo::default();
        let data = vec![0u8; 4];

        read_saiz(&data, 0, 4, &mut info);
        read_senc(&data, 0, 4, &mut info);
        assert_eq!(info.sample_count, 0);
    }
}
