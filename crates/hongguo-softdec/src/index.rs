//! 一集视频的样本索引：建一次，跳转多少次都复用。

use hongguo_bytevc2::{
    Bytevc2Track, decode_track_vvdec, is_bytevc2, read_moov_at, track_from_moov,
};
use hongguo_common::{AppError, AppResult};
use hongguo_video::r#box::{find_box, parse_boxes};
use hongguo_video::sample_table::{TrackInfo, collect_track, collect_tracks};

use crate::DecodedFrame;
use crate::color::FrameColor;
use crate::hevc::{HevcTrack, decode_track_hevc, is_hevc_fourcc};

/// 视频轨用哪个解核。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    /// `hvc1` / `hev1`：标准 HEVC，服务端的 `h265` 与 `bytevc1` 都落在这里。
    Hevc,
    /// `bvc2`：ByteVC2。
    Bytevc2,
}

enum Track {
    Hevc(HevcTrack),
    Bytevc2(Bytevc2Track),
}

impl Track {
    fn info(&self) -> &TrackInfo {
        match self {
            Track::Hevc(track) => &track.info,
            Track::Bytevc2(track) => &track.info,
        }
    }
}

/// 按 `moov` 里第一条视频轨的四字码选解核。认不出的编码在这里就报错——
/// 直播放没有「先试试看」的余地，更不会把它转交给系统解码器或转码。
fn open_track(moov: &[u8]) -> AppResult<Track> {
    let info = collect_tracks(moov)?
        .into_iter()
        .find(|track| track.is_video)
        .ok_or_else(|| AppError::Media("容器没有视频轨".into()))?;
    if is_bytevc2(&info.codec) {
        return track_from_moov(moov).map(Track::Bytevc2);
    }
    if is_hevc_fourcc(&info.codec) {
        return HevcTrack::from_moov(moov, info).map(Track::Hevc);
    }
    let codec = if info.codec.is_empty() {
        "未知"
    } else {
        info.codec.as_str()
    };
    Err(AppError::Media(format!(
        "内置解码器不支持这条视频轨的编码（{codec}）"
    )))
}

pub struct IndexedVideo {
    video: Track,
    video_atom_types: Vec<u64>,
    pts: Vec<f64>,
    last_pts: f64,
}

impl IndexedVideo {
    /// 读 `moov` 建索引。`read(偏移, 长度, 缓冲)` 由调用方供数；`length` 是整个文件的字节数。
    pub fn open(
        length: u64,
        read: impl FnMut(u64, u64, &mut Vec<u8>) -> AppResult<()>,
    ) -> AppResult<Self> {
        let (offset, data) = read_moov_at(length, read)?;
        let video = open_track(&data)?;
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
            return Err(AppError::Media("直播放找不到 AAC 音轨".into()));
        }
        let pts = video.info().sample_pts();
        let last_pts = pts
            .iter()
            .copied()
            .reduce(f64::max)
            .ok_or_else(|| AppError::Media("视频轨缺少显示时间轴".into()))?;
        Ok(Self {
            video,
            video_atom_types,
            pts,
            last_pts,
        })
    }

    pub fn codec(&self) -> VideoCodec {
        match self.video {
            Track::Hevc(_) => VideoCodec::Hevc,
            Track::Bytevc2(_) => VideoCodec::Bytevc2,
        }
    }

    /// 解出来的帧是什么 YUV 口径。HEVC 读自码流的 SPS；ByteVC2 的解码器绑定不给这项信息，
    /// 只能按惯例给（限幅范围，矩阵看分辨率）——那是推定，不是读到的事实。
    pub fn color(&self) -> FrameColor {
        match &self.video {
            Track::Hevc(track) => track.color,
            Track::Bytevc2(track) => FrameColor::assumed(track.info.height as usize, false),
        }
    }

    /// 把视频轨的 trak 标记换成 free，保留原音轨、edit list 与绝对样本偏移。
    pub fn mask_video_tracks(&self, offset: u64, bytes: &mut [u8]) {
        for &atom_type in &self.video_atom_types {
            mask_atom_type(atom_type, offset, bytes);
        }
    }

    /// 从 `target`（秒）前最近的关键帧起解到轨尾，按显示序回调显示时间不早于 `target` 的帧。
    /// 关键帧到 `target` 之间的帧照常解码（后面的帧要拿它们当参考），只是不回调。
    /// 回调返回错误即停止——取消就是这么传进来的。
    pub fn decode_from(
        &self,
        target: f64,
        read: impl FnMut(u64, u64, &mut Vec<u8>) -> AppResult<()>,
        mut on_frame: impl FnMut(DecodedFrame) -> AppResult<()>,
    ) -> AppResult<()> {
        let target = target.min(self.last_pts);
        let start = seek_sample(self.video.info(), &self.pts, target);
        let mut emit = |frame: DecodedFrame| {
            if frame
                .pts
                .ok_or_else(|| AppError::Media("解码帧缺少显示时间戳".into()))?
                + 0.000_5
                >= target
            {
                on_frame(frame)?;
            }
            Ok(())
        };
        match &self.video {
            Track::Hevc(track) => decode_track_hevc(track, start, read, &mut emit),
            Track::Bytevc2(track) => decode_track_vvdec(track, start, read, &mut emit),
        }
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
