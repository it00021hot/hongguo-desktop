//! 时间轴表：`stts` / `ctts` / `stss` / `mdhd`。
//! [`super::sample_table`] 只负责「样本在哪、多大」；拼接还需要「样本占多久」——四张表同属时间轴一件事，放在一起。

use super::sample_table::TrackInfo;
use hongguo_common::{AppError, AppResult};

#[path = "movie_timeline.rs"]
mod movie;
pub use movie::{EditSegment, MappedTime, MediaPosition, MovieTimeline, TrackTimeline};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditEntry {
    pub segment_duration: u64,
    pub media_time: i64,
    /// Signed 16.16 fixed point playback rate; zero means dwell.
    pub media_rate: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaSampleTime {
    pub dts: i64,
    pub pts: i64,
    pub duration: u32,
}

pub fn movie_timescale(data: &[u8]) -> AppResult<u32> {
    use crate::r#box::find_box;
    let Some(header) = find_box(data, 0, data.len(), "mvhd") else {
        return Ok(0);
    };
    let offset = match data.get(header.start) {
        Some(0) => 12,
        Some(1) => 20,
        _ => return Err(AppError::Media("unsupported mvhd version".into())),
    };
    let bytes = data
        .get(header.start + offset..header.start + offset + 4)
        .filter(|_| offset + 4 <= header.size)
        .ok_or_else(|| AppError::Media("truncated mvhd timescale".into()))?;
    Ok(u32::from_be_bytes(bytes.try_into().map_err(|_| {
        AppError::Media("invalid mvhd timescale".into())
    })?))
}

/// 从 `stts` 读每段的样本数与时长增量：version+flags(4) | entry_count(4) | entries(sample_count 4, sample_delta 4)。
/// 游程条数与 `stsz` 样本数不一定是同一套：一条条目代表连续一段等长样本，合并按段拼、不按样本展开。
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

/// 从 `ctts` 读合成时间偏移（B 帧重排序的修正量）：结构与 `stts` 同形，末字段**符号**由 version 决定（v0 无符号 / v1 有符号）。
/// 读错符号会让 B 帧的显示时间整体偏移——表现是「画面卡住不动但时间在走」，不报错。
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
            // version 0 的值是 u32；落到 i32 负半区说明作者写错版本号，当正数解读更接近原意
            i32::try_from(raw).unwrap_or(i32::MAX)
        };
        runs.push((u32_at(data, at), offset));
    }
    info.ctts = runs;
}

/// 从 `stss` 读同步样本（关键帧）序号（1 起）。
/// box 不存在 = 全部样本都是同步样本，用 `Option` 区分「没有这张表」和「表是空的」。
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

/// 从 `mdhd` 读媒体时基与时长：v0 为 creation(4)/modification(4)/timescale(4)/duration(4)，v1 为 8/8/4/8。
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

/// 从 `elst` 读 edit 起点（首个非空 edit 的 media_time）：v0 条目 12 字节（duration/media_time/rate 各 4），v1 条目 20 字节。
/// media_time = -1（各版本的无符号全 1）是空 edit 占位，跳过。
pub(super) fn read_elst(data: &[u8], start: usize, size: usize, info: &mut TrackInfo) {
    if size < 8 || info.media_timescale == 0 {
        return;
    }
    let version = data[start];
    if version > 1 {
        return;
    }
    let entry = if version == 1 { 20 } else { 12 };
    let count = read_entry_count(data, start, size, entry);
    info.edit_list.clear();
    for i in 0..count {
        let at = start + 8 + i * entry;
        if at + entry > start + size {
            break;
        }
        let media_time = if version == 1 {
            u64_at(data, at + 8) as i64
        } else {
            // media_time 是 SInt32：零扩展会把全 1 读成天文数字正数，空 edit 判定就废了
            i64::from(u32_at(data, at + 4) as i32)
        };
        let segment_duration = if version == 1 {
            u64_at(data, at)
        } else {
            u64::from(u32_at(data, at))
        };
        let rate_at = at + entry - 4;
        info.edit_list.push(EditEntry {
            segment_duration,
            media_time,
            media_rate: u32_at(data, rate_at) as i32,
        });
    }
    info.edit_media_start_secs = info
        .edit_list
        .iter()
        .find(|edit| edit.media_time >= 0)
        .map_or(0.0, |edit| {
            edit.media_time as f64 / f64::from(info.media_timescale)
        });
}

impl TrackInfo {
    /// Raw signed media ticks, in decode order. No edit or per-track origin shift is applied.
    pub fn media_sample_timeline(&self) -> AppResult<Vec<MediaSampleTime>> {
        let samples = self.samples.len() as u64;
        let timing_count: u64 = self.stts.iter().map(|run| u64::from(run.0)).sum();
        let composition_count: u64 = self.ctts.iter().map(|run| u64::from(run.0)).sum();
        if self.media_timescale == 0
            || timing_count != samples
            || (!self.ctts.is_empty() && composition_count != samples)
        {
            return Err(AppError::Media("inconsistent media sample timeline".into()));
        }
        if self.stts.iter().any(|run| run.1 == 0) {
            return Err(AppError::Media("zero sample duration".into()));
        }
        self.sample_times()
    }

    fn sample_times(&self) -> AppResult<Vec<MediaSampleTime>> {
        let total = self.samples.len();
        if self.stts.is_empty() || self.media_timescale == 0 || total == 0 {
            return Ok(Vec::new());
        }
        let mut times = Vec::with_capacity(total);
        let mut cursor = 0i64;
        let mut last_delta = 1u32;
        let mut ctts = self
            .ctts
            .iter()
            .flat_map(|&(c, o)| std::iter::repeat_n(o, c as usize));
        for &(count, delta) in &self.stts {
            last_delta = delta;
            for _ in 0..count {
                if times.len() == total {
                    break;
                }
                let offset = i64::from(ctts.next().unwrap_or(0));
                let pts = cursor
                    .checked_add(offset)
                    .ok_or_else(|| AppError::Media("sample timestamp overflow".into()))?;
                times.push(MediaSampleTime {
                    dts: cursor,
                    pts,
                    duration: delta,
                });
                cursor = cursor
                    .checked_add(i64::from(delta))
                    .ok_or_else(|| AppError::Media("sample timestamp overflow".into()))?;
            }
            if times.len() == total {
                break;
            }
        }
        while times.len() < total {
            times.push(MediaSampleTime {
                dts: cursor,
                pts: cursor,
                duration: last_delta,
            });
            cursor = cursor
                .checked_add(i64::from(last_delta))
                .ok_or_else(|| AppError::Media("sample timestamp overflow".into()))?;
        }
        Ok(times)
    }

    fn timeline(&self) -> (Vec<f64>, Vec<f64>) {
        let times = self.sample_times().unwrap_or_default();
        let timescale = f64::from(self.media_timescale);
        let mut movie_start = 0u64;
        let mut media_rate = 1.0;
        for edit in &self.edit_list {
            if edit.media_time >= 0 {
                media_rate = f64::from(edit.media_rate) / 65_536.0;
                break;
            }
            movie_start = movie_start.saturating_add(edit.segment_duration);
        }
        let movie_start = if self.movie_timescale > 0 {
            movie_start as f64 / f64::from(self.movie_timescale)
        } else {
            0.0
        };
        let map = |ticks: i64| {
            if media_rate == 0.0 {
                movie_start
            } else {
                movie_start + (ticks as f64 / timescale - self.edit_media_start_secs) / media_rate
            }
        };
        let dts = times.iter().map(|time| map(time.dts)).collect();
        let pts = times.iter().map(|time| map(time.pts)).collect();
        (dts, pts)
    }

    /// 每样本的显示时间（秒），保留负时间；多段 edit 的完整映射使用 MovieTimeline。
    /// 按**解码序**索引，带 B 帧时不保证按下标递增；没有 `stts`、时基为 0 或没有样本时返回空 `Vec`（调用方回落合成轴）。
    pub fn sample_pts(&self) -> Vec<f64> {
        self.timeline().1
    }

    /// B 帧重排的最大负偏移（秒）：显示时间最多能比解码时间早多少；无 `ctts`（无 B 帧）时为 0。
    /// 消费方用「已解码 dts 水位 − 本值」当定稿线：早于这条线的帧不可能再有更早的帧解出来。
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

    /// 从样本表推出的平均帧率（fps）；没有 `stts` 或总时长为 0 时返回 `None`，调用方用默认帧率兜底。
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
    // 上限卡在「box 声明字节数装不下这么多条」：畸形 count 不该触发几亿次循环分配
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
