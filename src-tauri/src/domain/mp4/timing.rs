//! 时间轴表：`stts` / `ctts` / `stss` / `mdhd`。
//!
//! [`super::sample_table`] 只负责「样本在哪、多大」——那是解复用与解密要的。
//! 但拼接时还必须知道「样本占多久」，否则合并出来的文件时长只有第 1 集的长度。
//! 四张表放一起是因为它们同属「时间轴」这一件事，拆到 `sample_table` 里会顶破
//! 那个文件的单一职责。

use super::sample_table::TrackInfo;

/// 从 `stts` 读每段的样本数与时长增量。
///
/// 结构：version+flags(4) | entry_count(4) | entries: sample_count(4) sample_delta(4)
///
/// 返回的游程条数与 `stsz` 的样本数不一定是同一套：一条 `stts` 条目代表连续
/// 一段等长样本，合并时要按段拼，不能按样本展开。
pub(super) fn read_stts(data: &[u8], start: usize, size: usize, info: &mut TrackInfo) {
    let count = read_entry_count(data, start, size, 4);
    let mut runs = Vec::with_capacity(count);
    for i in 0..count {
        let at = start + 8 + i * 8;
        if at + 8 > start + size {
            break;
        }
        runs.push((u32_at(data, at), u32_at(data, at + 4)));
    }
    info.stts = runs;
}

/// 从 `ctts` 读合成时间偏移（B 帧重排序的修正量）。
///
/// 结构与 `stts` 同形，区别在末字段的**符号**由 version 决定：
/// version 0 是无符号，version 1 是有符号。读错符号会让 B 帧的显示时间整体偏移，
/// 表现是「画面卡住不动但时间在走」——不报错，只是看着不对。
pub(super) fn read_ctts(data: &[u8], start: usize, size: usize, info: &mut TrackInfo) {
    let signed = data[start] == 1;
    let count = read_entry_count(data, start, size, 4);
    let mut runs = Vec::with_capacity(count);
    for i in 0..count {
        let at = start + 8 + i * 8;
        if at + 8 > start + size {
            break;
        }
        let raw = u32_at(data, at + 4);
        let offset = if signed {
            raw as i32
        } else {
            // version 0 的值是 u32；落到 i32 的负半区说明作者写错了版本号，
            // 当成正数解读比当成负数更接近原意。
            i32::try_from(raw).unwrap_or(i32::MAX)
        };
        runs.push((u32_at(data, at), offset));
    }
    info.ctts = runs;
}

/// 从 `stss` 读同步样本（关键帧）序号，1 起。
///
/// box 不存在 = 全部样本都是同步样本，所以用 `Option` 区分「没有这张表」
/// 和「表是空的」——合并时前者要把整集并进关键帧集合，后者是确实一个都没有。
pub(super) fn read_stss(data: &[u8], start: usize, size: usize, info: &mut TrackInfo) {
    let count = read_entry_count(data, start, size, 4);
    let mut sync = Vec::with_capacity(count);
    for i in 0..count {
        let at = start + 8 + i * 4;
        if at + 4 > start + size {
            break;
        }
        sync.push(u32_at(data, at));
    }
    info.stss = Some(sync);
}

/// 从 `mdhd` 读媒体时基与时长。
///
/// version 0：creation(4) modification(4) timescale(4) duration(4)
/// version 1：creation(8) modification(8) timescale(4) duration(8)
///
/// timescale 为 0 的轨道没法算时长（所有 delta 除下来都是 0），当无时长处理。
pub(super) fn read_mdhd(data: &[u8], start: usize, size: usize, info: &mut TrackInfo) {
    let wide = data[start] == 1;
    let (ts_at, dur_at, dur_len) = if wide {
        (start + 20, start + 24, 8)
    } else {
        (start + 12, start + 16, 4)
    };
    if ts_at + 4 > start + size || dur_at + dur_len > start + size {
        return;
    }
    info.media_timescale = u32_at(data, ts_at);
    info.media_duration = if wide {
        u64_at(data, dur_at)
    } else {
        u64::from(u32_at(data, dur_at))
    };
}

/// entry_count 所在偏移与 entry 宽度决定表里能读几条。
fn read_entry_count(data: &[u8], start: usize, size: usize, entry_size: usize) -> usize {
    if size < 8 {
        return 0;
    }
    let count = u32_at(data, start + 4) as usize;
    // 上限卡在「box 声明的字节数装不下这么多条」：平台偶发的畸形 count
    // 不该让这里按几亿次循环分配。
    count.min(size.saturating_sub(8) / entry_size)
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

fn u64_at(data: &[u8], at: usize) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&data[at..at + 8]);
    u64::from_be_bytes(buf)
}

#[cfg(test)]
#[path = "timing_tests.rs"]
mod tests;
