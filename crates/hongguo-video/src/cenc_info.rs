//! CENC 辅助信息：`saiz`（每样本附加信息大小）与 `senc`（逐样本 IV）。
//! [`super::sample_table`] 管「样本在哪、多大」，这里管「怎么解密」，两套字段分文件。

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
    // 畸形 count 防御（口径同 timing::read_entry_count）：saiz 每条 1 字节，卡到 box 容量内
    let n = (count as usize).min(size.saturating_sub(9)).min(data.len());
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
/// ⚠️ **IV 是 8 字节、步长 8**（平台封装如此，现版 JS 按 `i * 8` 取）；按 CENC 规范的 16 字节读会整体错位——不报错但全是噪声。
pub(super) fn read_senc(data: &[u8], start: usize, size: usize, info: &mut TrackInfo) {
    if size < 8 {
        return;
    }
    // version+flags 占 4 字节、sample_count 紧随其后；现版 JS 同样不按 version 分支，保持一致
    let count_at = start + 4;
    if count_at + 4 > start + size {
        return;
    }
    // 畸形 count 防御（口径同 timing::read_entry_count）：每条 IV 8 字节，卡到 box 容量内
    let count = (u32::from_be_bytes([
        data[count_at],
        data[count_at + 1],
        data[count_at + 2],
        data[count_at + 3],
    ]) as usize)
        .min(size.saturating_sub(8) / 8)
        .min(data.len());

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
