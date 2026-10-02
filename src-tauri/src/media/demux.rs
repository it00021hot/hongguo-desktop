//! MP4 轨道/样本解复用。
//!
//! 与下载解密阶段共用 [`crate::domain::mp4`] 的 box 解析——同一份文件在两处
//! 解析必须得到同样结果，否则合并出来的文件会和下载产物对不上。

use std::path::Path;

use crate::domain::mp4::sample_table::{collect_tracks, TrackInfo};
use crate::error::{AppError, AppResult};

/// 一条可处理的轨道。
#[derive(Debug, Clone)]
pub struct DemuxedTrack {
    pub info: TrackInfo,
}

/// 解复用结果。
#[derive(Debug, Clone)]
pub struct Demuxed {
    pub tracks: Vec<DemuxedTrack>,
}

impl Demuxed {
    /// 视频轨时长无法从容器直接读出时，返回 0。
    pub fn video_track(&self) -> Option<&DemuxedTrack> {
        self.tracks.iter().find(|t| t.info.is_video)
    }

    /// 第一条音轨。转码时用来把 AAC 帧带进产物。
    pub fn audio_track(&self) -> Option<&DemuxedTrack> {
        self.tracks.iter().find(|t| !t.info.is_video)
    }
}

/// 从内存解复用。
pub fn demux_bytes(data: &[u8]) -> AppResult<Demuxed> {
    // 两条失败路径都只把原因放进返回值，而调用方（合并前的编码一致性检查）
    // 只能报告「第 N 集解析不出来」——具体坏在哪个 box 上就丢了。这里补一条日志，
    // 否则线上遇到坏文件只能靠猜。
    let tracks = collect_tracks(data).map_err(|e| {
        log::error!("[Media] 解析轨道失败: {e}");
        e
    })?;
    if tracks.is_empty() {
        log::error!("[Media] 容器里没有可用轨道");
        return Err(AppError::Media("没有可用轨道".into()));
    }
    Ok(Demuxed {
        tracks: tracks
            .into_iter()
            .map(|info| DemuxedTrack { info })
            .collect(),
    })
}

/// 从文件解复用（只读头部，避免大文件全量载入）。
pub fn demux_file(path: &Path) -> AppResult<Demuxed> {
    let head_len = std::cmp::min(
        std::fs::metadata(path)
            .map_err(|e| AppError::Io(e.to_string()))?
            .len(),
        8 * 1024 * 1024,
    ) as usize;
    let mut buf = vec![0u8; head_len];
    {
        use std::io::Read;
        let mut f = std::fs::File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
        f.read_exact(&mut buf)
            .map_err(|e| AppError::Io(e.to_string()))?;
    }
    demux_bytes(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garbage_errors() {
        assert!(demux_bytes(&[0xffu8; 100]).is_err());
        assert!(demux_bytes(&[]).is_err());
    }

    #[test]
    fn missing_file_errors() {
        assert!(demux_file(Path::new("/definitely/not/here.mp4")).is_err());
    }
}
