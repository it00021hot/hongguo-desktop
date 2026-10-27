//! 源音轨读取：声道/采样率、`esds` 原样搬运、裸 AAC 帧。
//!
//! 源音轨在解密后已是**明文 AAC**，直接搬进产物即可——不解码成 PCM 再重编码，
//! 既省掉一整轮编解码，也没有音质损失。
//!
//! MP4 里的 AAC 样本是**不带帧头**的裸数据，容器参数全在 `mp4a` sample entry
//! 与它的 `esds` 里。这里只取三样东西：声道数、采样率、`esds`。
//!
//! `esds` **从源原样搬**：它里面的 `AudioSpecificConfig` 是解码器初始化必需的，
//! 自己按采样率重造一份等于赌源是普通 AAC-LC——源是 HE-AAC 时就会解错。
//! 源确实没有 `esds` 时才退到现造的 AAC-LC。

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use hongguo_common::{AppError, AppResult};
use hongguo_video::r#box::find_box;
use hongguo_video::sample_table::TrackInfo;

/// 一条待封装的音轨。
pub struct AudioTrack {
    /// 采样率，如 44100
    pub sample_rate: u32,
    /// 声道数
    pub channels: u16,
    /// 源音轨的 `esds` box 原始字节（含 box 头）。`None` = 源里没有。
    pub esds: Option<Vec<u8>>,
    /// 逐帧 `(时间戳秒, 裸 AAC 帧)`
    pub samples: Vec<(f64, Vec<u8>)>,
}

/// 收集源音轨：AAC 帧 + 封装参数。
///
/// 没有音轨（或音轨没有样本）时返回 `None`，产物就是纯视频——
/// 与 ffmpeg `-an` 的行为一致。
pub fn collect(input: &Path, demuxed: &crate::demux::Demuxed) -> AppResult<Option<AudioTrack>> {
    let Some(track) = demuxed.audio_track() else {
        return Ok(None);
    };
    if track.info.samples.is_empty() {
        return Ok(None);
    }

    let head = read_head(input, &track.info)?;
    let (channels, sample_rate) = read_entry_params(&head, &track.info)?;
    let esds = read_esds(&head, &track.info);

    // 只读音轨样本所在的区间，不把整个文件读进来：
    // 视频轨动辄几十上百 MB，整文件读一次会白占一份与转码无关的内存。
    let mut samples = Vec::with_capacity(track.info.samples.len());
    let mut pts = 0.0f64;
    let mut file = std::fs::File::open(input).map_err(|e| AppError::Io(e.to_string()))?;
    for &(offset, size) in &track.info.samples {
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| AppError::Io(e.to_string()))?;
        let mut frame = vec![0u8; size as usize];
        file.read_exact(&mut frame)
            .map_err(|e| AppError::Media(format!("读取音轨样本失败: {e}")))?;
        samples.push((pts, frame));
        // AAC 每帧固定 1024 个采样，时间戳按累计秒数给
        pts += 1024.0 / f64::from(sample_rate.max(1));
    }

    Ok(Some(AudioTrack {
        sample_rate,
        channels,
        esds,
        samples,
    }))
}

/// 读出覆盖 `stbl` 的头部字节。
///
/// `stbl` 恒在 moov 里，位置靠前——只读这么多就够解析参数，不必为了几个
/// 字节把上百 MB 的视频读进内存。
fn read_head(path: &Path, track: &TrackInfo) -> AppResult<Vec<u8>> {
    let need = track.stbl_offset + track.stbl_size;
    let len = std::fs::metadata(path)
        .map_err(|e| AppError::Io(e.to_string()))?
        .len() as usize;
    let head_len = need.min(len).min(8 * 1024 * 1024);
    let mut data = vec![0u8; head_len];
    let mut file = std::fs::File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
    file.read_exact(&mut data)
        .map_err(|e| AppError::Io(e.to_string()))?;
    Ok(data)
}

/// 读 `mp4a` sample entry 里的声道数与采样率。
///
/// AudioSampleEntry 的固定字段（ISO/IEC 14496-12）：reserved[6] +
/// data_reference_index(2) + reserved[8] + channelcount(2) + samplesize(2) +
/// pre_defined(2) + reserved(2) + samplerate(4, 16.16 定点)。
fn read_entry_params(data: &[u8], track: &TrackInfo) -> AppResult<(u16, u32)> {
    let entry = sample_entry_start(data, track)?;
    if entry + 28 > data.len() {
        return Err(AppError::Media("音轨的 sample entry 不完整".into()));
    }
    let channels = u16::from_be_bytes([data[entry + 16], data[entry + 17]]);
    let sample_rate = u16::from_be_bytes([data[entry + 24], data[entry + 25]]) as u32;
    Ok((channels, sample_rate))
}

/// 在 sample entry 的扩展区里找 `esds` box，连头带尾原样取出。
///
/// 不按固定布局算位置：音频 entry 的固定体各家不一致（ISO 24 字节、
/// QuickTime 变体到 44），固定字段里的零字节还会让逐盒解析提前误判
/// size=0。在扩展区里**容错扫描** `esds` 四字符码——音频 entry 里这个
/// 四字符码不存在歧义。
fn read_esds(data: &[u8], track: &TrackInfo) -> Option<Vec<u8>> {
    let entry = sample_entry_start(data, track).ok()?;
    let entry_end = sample_entry_end(data, track).ok()?;
    // 固定体的保守下限是 24 字节（ISO 布局实际 28），扩展区从那里起扫
    let from = entry + 24;
    if from + 12 > entry_end {
        return None;
    }
    for i in from..=entry_end - 12 {
        if &data[i..i + 4] == b"esds" {
            let size =
                u32::from_be_bytes([data[i - 4], data[i - 3], data[i - 2], data[i - 1]]) as usize;
            // size 覆盖整个 box 且不越 entry 才认
            if size >= 12 && i - 4 + size <= entry_end {
                return Some(data[i - 4..i - 4 + size].to_vec());
            }
        }
    }
    None
}

/// sample entry 的载荷起点。
///
/// `stsd` 载荷：version+flags(4) + entry_count(4) + entry box 头(8)。
fn sample_entry_start(data: &[u8], track: &TrackInfo) -> AppResult<usize> {
    let stsd = find_box(
        data,
        track.stbl_offset,
        track.stbl_offset + track.stbl_size,
        "stsd",
    )
    .ok_or_else(|| AppError::Media("音轨缺少 stsd".into()))?;
    Ok(stsd.start + 16)
}

/// sample entry 的载荷终点。
fn sample_entry_end(data: &[u8], track: &TrackInfo) -> AppResult<usize> {
    let stsd = find_box(
        data,
        track.stbl_offset,
        track.stbl_offset + track.stbl_size,
        "stsd",
    )
    .ok_or_else(|| AppError::Media("音轨缺少 stsd".into()))?;
    Ok(stsd.start + stsd.size)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一条 `mp4a` 音轨的 `stsd` 载荷（外层是 stbl 里的 stsd box）。
    fn mp4a_stsd(channels: u16, sample_rate: u32, esds: Option<&[u8]>) -> Vec<u8> {
        let mut entry = Vec::new();
        entry.extend([0u8; 6]);
        entry.extend(1u16.to_be_bytes()); // data_reference_index
        entry.extend([0u8; 8]);
        entry.extend(channels.to_be_bytes());
        entry.extend(16u16.to_be_bytes());
        entry.extend(0u16.to_be_bytes());
        entry.extend(0u16.to_be_bytes());
        entry.extend((sample_rate << 16).to_be_bytes());
        if let Some(bytes) = esds {
            entry.extend_from_slice(bytes);
        }
        let mut stsd = Vec::new();
        stsd.extend(0u32.to_be_bytes()); // version + flags
        stsd.extend(1u32.to_be_bytes()); // entry_count
        stsd.extend((8 + entry.len() as u32).to_be_bytes());
        stsd.extend_from_slice(b"mp4a");
        stsd.extend(entry);
        stsd
    }

    /// 把 stsd 载荷包进 `stbl`，再伪造一条指向它的轨道信息。
    fn track_with(stsd_payload: Vec<u8>) -> (Vec<u8>, TrackInfo) {
        let mut stbl = Vec::new();
        stbl.extend((8 + stsd_payload.len() as u32).to_be_bytes());
        stbl.extend_from_slice(b"stsd");
        stbl.extend(stsd_payload);
        let track = TrackInfo {
            stbl_offset: 0,
            stbl_size: stbl.len(),
            ..TrackInfo::default()
        };
        (stbl, track)
    }

    #[test]
    fn entry_params_are_read_from_the_audio_sample_entry() {
        let (stbl, track) = track_with(mp4a_stsd(2, 44100, None));
        let (channels, rate) = read_entry_params(&stbl, &track).expect("应读出参数");
        assert_eq!(channels, 2);
        assert_eq!(rate, 44100);
    }

    #[test]
    fn the_source_esds_is_taken_verbatim() {
        let esds = vec![0u8; 8];
        let esds_box = {
            let mut b = Vec::new();
            b.extend((8 + esds.len() as u32).to_be_bytes());
            b.extend_from_slice(b"esds");
            b.extend_from_slice(&esds);
            b
        };
        let (stbl, track) = track_with(mp4a_stsd(2, 44100, Some(&esds_box)));
        let got = read_esds(&stbl, &track).expect("应找到 esds");
        assert_eq!(got, esds_box);
    }

    #[test]
    fn a_track_without_esds_yields_none() {
        let (stbl, track) = track_with(mp4a_stsd(2, 44100, None));
        assert!(read_esds(&stbl, &track).is_none());
    }
}
