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

/// 从 `elst` 读 edit 起点（首个非空 edit 的 media_time）。
///
/// 结构：version+flags(4) | entry_count(4) | entries。
/// version 0 条目 12 字节：segment_duration(4) media_time(4) rate(4)；
/// version 1 条目 20 字节：duration(8) media_time(8) rate(4)。
/// media_time = -1（各版本的无符号全 1）是空 edit 占位，跳过。
pub(super) fn read_elst(data: &[u8], start: usize, size: usize, info: &mut TrackInfo) {
    if size < 8 || info.media_timescale == 0 {
        return;
    }
    let version = data[start];
    let entry = if version == 1 { 20 } else { 12 };
    let count = read_entry_count(data, start, size, entry);
    for i in 0..count {
        let at = start + 8 + i * entry;
        if at + entry > start + size {
            break;
        }
        let media_time = if version == 1 {
            u64_at(data, at + 8) as i64
        } else {
            // media_time 是 SInt32：-1（全 1）是空 edit 占位，零扩展会读成
            // 天文数字的正数，空 edit 判定就废了
            i64::from(u32_at(data, at + 4) as i32)
        };
        if media_time < 0 {
            continue; // 空 edit：只占 presentation 时长，不指媒体位置
        }
        info.edit_media_start_secs = media_time as f64 / f64::from(info.media_timescale);
        break; // 首个非空 edit 即锚点，多 edit 流按首个锚定
    }
}

impl TrackInfo {
    /// 每样本的**解码**时间与**显示**时间（秒），同一套展开一次算齐。
    ///
    /// 返回 `(dts, pts)`，两者都按**解码序**（样本序）索引、同量平移过
    /// （负偏移时最早的显示时间落到 0 之前，整体抬到 0 起步）。带 B 帧的
    /// 码流里 **pts 不保证递增**——「解码器输出（显示）序下递增」才是不变量，
    /// 由消费方保证；**dts 恒递增**，是消费方做重排时的安全水位。
    fn timeline(&self) -> (Vec<f64>, Vec<f64>) {
        let total = self.samples.len();
        if self.stts.is_empty() || self.media_timescale == 0 || total == 0 {
            return (Vec::new(), Vec::new());
        }
        let timescale = f64::from(self.media_timescale);

        // 展开到与样本数等长：表短于样本数时按最后一段 delta 延续，
        // 表长于样本数时多出的段丢弃——两表描述本该是同一批样本，
        // 不一致只出现在畸形文件上，能覆盖多少就覆盖多少。
        let mut dts = Vec::with_capacity(total);
        let mut pts = Vec::with_capacity(total);
        let mut cursor = 0u64;
        let mut last_delta = 1u32;
        let mut ctts = self
            .ctts
            .iter()
            .flat_map(|&(c, o)| std::iter::repeat_n(o, c as usize));
        for &(count, delta) in &self.stts {
            last_delta = delta.max(1);
            for _ in 0..count {
                if dts.len() == total {
                    break;
                }
                let offset = i64::from(ctts.next().unwrap_or(0));
                dts.push(cursor as f64 / timescale);
                pts.push((cursor as i64 + offset) as f64 / timescale);
                cursor += u64::from(delta);
            }
            if dts.len() == total {
                break;
            }
        }
        while dts.len() < total {
            let t = cursor as f64 / timescale;
            dts.push(t);
            pts.push(t);
            cursor += u64::from(last_delta);
        }

        // 负偏移（version 1 ctts）会让最早的显示时间落在 0 之前，而音轨与
        // 封装层都从 0 起步：整体平移，等量平移不破坏任何相对时序。dts 跟着
        // 同量平移——两个轴必须对齐，否则「显示时间 - 解码时间」的差会失真。
        if let Some(min) = pts.iter().cloned().reduce(f64::min)
            && min < 0.0
        {
            for p in &mut pts {
                *p -= min;
            }
            for d in &mut dts {
                *d -= min;
            }
        }

        // elst：presentation 时间轴 = 媒体时间轴 − edit 起点（AAC 编码器
        // priming 由容器的 edit list 表达）。减出负值是刻意的——起点前的
        // 样本（编码器前置静音）在 presentation 轴上就该落在 0 之前，由
        // 消费方按「pts ≥ 起点」过滤。dts 同量平移，保持两轴对齐。
        if self.edit_media_start_secs > 0.0 {
            for p in &mut pts {
                *p -= self.edit_media_start_secs;
            }
            for d in &mut dts {
                *d -= self.edit_media_start_secs;
            }
        }
        (dts, pts)
    }

    /// 每样本的**显示**时间戳（秒），由 `stts`/`ctts` 游程展开，整体平移到 0 起点。
    ///
    /// 数组按**解码序**（样本序）索引；带 B 帧的码流里显示序 ≠ 解码序，
    /// 所以返回值**不保证**按数组下标递增——「解码器输出（显示）序下递增」
    /// 才是不变量，由消费方（转码的编码输出侧）保证。组成偏移
    /// （`ctts`）直接加在解码时间上。没有 `stts`、时基为 0 或没有样本时返回
    /// 空 `Vec`：容器没给时间轴，调用方回落到「样本号 / 默认帧率」的合成轴。
    pub fn sample_pts(&self) -> Vec<f64> {
        self.timeline().1
    }

    /// B 帧重排的最大负偏移（秒）：显示时间最多能比解码时间早多少。
    ///
    /// 无 `ctts`（无 B 帧）时为 0。消费方用「已解码 dts 水位 - 本值」当
    /// 定稿线：显示时间早于这条线的帧，不可能再有更早的帧解出来。
    pub fn max_reorder_secs(&self) -> f64 {
        if self.media_timescale == 0 {
            return 0.0;
        }
        let most_negative = self
            .ctts
            .iter()
            .map(|&(_, offset)| i64::from(offset).min(0))
            .min()
            .unwrap_or(0);
        (-most_negative) as f64 / f64::from(self.media_timescale)
    }

    /// 从样本表推出的平均帧率（fps）。
    ///
    /// 没有 `stts` 或总时长为 0 时返回 `None`，调用方用默认帧率兜底。
    pub fn average_framerate(&self) -> Option<f64> {
        if self.stts.is_empty() || self.media_timescale == 0 {
            return None;
        }
        let deltas: u64 = self
            .stts
            .iter()
            .map(|&(c, d)| u64::from(c) * u64::from(d))
            .sum();
        if deltas == 0 {
            return None;
        }
        let secs = deltas as f64 / f64::from(self.media_timescale);
        // sample_count 由 stsz 填写；未填的合成结构按 samples 长度算
        let frames = if self.sample_count > 0 {
            self.sample_count.min(self.samples.len() as u32)
        } else {
            self.samples.len() as u32
        };
        if frames == 0 {
            return None;
        }
        let fps = f64::from(frames) / secs;
        (fps > 0.0 && fps.is_finite()).then_some(fps)
    }
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
