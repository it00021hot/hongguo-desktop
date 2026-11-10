use hongguo_common::{AppError, AppResult};
use hongguo_video::r#box::{find_box, parse_boxes};
use hongguo_video::sample_table::{TrackInfo, collect_track};

use crate::DecodedFrame;
use crate::container::{Bytevc2Track, read_moov_at, track_from_moov};
use crate::decode::decode_track_vvdec;

pub struct IndexedBytevc2 {
    video: Bytevc2Track,
    video_atom_types: Vec<u64>,
    pts: Vec<f64>,
    last_pts: f64,
}

impl IndexedBytevc2 {
    pub fn open(
        length: u64,
        read: impl FnMut(u64, u64, &mut Vec<u8>) -> AppResult<()>,
    ) -> AppResult<Self> {
        let (offset, data) = read_moov_at(length, read)?;
        let video = track_from_moov(&data)?;
        let moov = find_box(&data, 0, data.len(), "moov")
            .ok_or_else(|| AppError::Media("找不到 moov box".into()))?;
        let mut video_atom_types = Vec::new();
        let mut has_aac = false;
        for atom in parse_boxes(&data, moov.start, moov.start + moov.size) {
            if !atom.is("trak") {
                continue;
            }
            let Some(track) = collect_track(&data, atom.start, atom.start + atom.size) else {
                continue;
            };
            if track.is_video {
                video_atom_types.push(offset + (atom.start - atom.header_size + 4) as u64);
            } else if track.codec == "mp4a" {
                has_aac = true;
            }
        }
        if !has_aac {
            return Err(AppError::Media("ByteVC2 直播放找不到 AAC 音轨".into()));
        }
        let pts = video.info.sample_pts();
        let last_pts = pts
            .iter()
            .copied()
            .reduce(f64::max)
            .ok_or_else(|| AppError::Media("ByteVC2 缺少显示时间轴".into()))?;
        Ok(Self {
            video,
            video_atom_types,
            pts,
            last_pts,
        })
    }

    /// 把视频轨的 trak 标记换成 free，保留原音轨、edit list 与绝对样本偏移。
    pub fn mask_video_tracks(&self, offset: u64, bytes: &mut [u8]) {
        for &atom_type in &self.video_atom_types {
            mask_atom_type(atom_type, offset, bytes);
        }
    }

    pub fn decode_from(
        &self,
        target: f64,
        read: impl FnMut(u64, u64, &mut Vec<u8>) -> AppResult<()>,
        mut on_frame: impl FnMut(DecodedFrame) -> AppResult<()>,
    ) -> AppResult<()> {
        let target = target.min(self.last_pts);
        let start = seek_sample(&self.video.info, &self.pts, target);
        decode_track_vvdec(&self.video, start, read, &mut |frame| {
            if frame
                .pts
                .ok_or_else(|| AppError::Media("ByteVC2 缺少显示时间戳".into()))?
                + 0.000_5
                >= target
            {
                on_frame(frame)?;
            }
            Ok(())
        })
    }
}

fn mask_atom_type(atom_type: u64, offset: u64, bytes: &mut [u8]) {
    for (relative, replacement) in b"free".iter().enumerate() {
        if let Some(index) = (atom_type + relative as u64).checked_sub(offset)
            && let Some(byte) = bytes.get_mut(index as usize)
        {
            *byte = *replacement;
        }
    }
}

fn seek_sample(track: &TrackInfo, pts: &[f64], target: f64) -> usize {
    if target <= 0.0 {
        return 0;
    }
    let mut best = (0, f64::NEG_INFINITY);
    let mut consider = |index| {
        if let Some(&time) = pts.get(index)
            && time <= target
            && time >= best.1
        {
            best = (index, time);
        }
    };
    if let Some(samples) = &track.stss {
        for sample in samples.iter().filter_map(|sample| sample.checked_sub(1)) {
            consider(sample as usize);
        }
    } else {
        for index in 0..pts.len() {
            consider(index);
        }
    }
    best.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seek_uses_previous_sync_sample_in_decode_order() {
        let mut track = TrackInfo {
            samples: vec![(0, 1); 5],
            stts: vec![(5, 1)],
            stss: Some(vec![1, 3, 5]),
            media_timescale: 1,
            ..Default::default()
        };
        let pts = track.sample_pts();
        assert_eq!(seek_sample(&track, &pts, 0.0), 0);
        assert_eq!(seek_sample(&track, &pts, 3.0), 2);
        assert_eq!(seek_sample(&track, &pts, 4.0), 4);
        track.stss = None;
        assert_eq!(seek_sample(&track, &pts, 3.0), 3);
    }

    #[test]
    fn audio_view_patches_only_the_overlap_with_a_range() {
        let mut before = *b"_tr";
        let mut after = *b"ak_";
        mask_atom_type(10, 9, &mut before);
        mask_atom_type(10, 12, &mut after);
        assert_eq!(&before, b"_fr");
        assert_eq!(&after, b"ee_");
    }
}
