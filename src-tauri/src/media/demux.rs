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
    let file_size = std::fs::metadata(path)
        .map_err(|e| AppError::Io(e.to_string()))?
        .len();
    let head_len = std::cmp::min(file_size, 8 * 1024 * 1024) as usize;
    let mut buf = vec![0u8; head_len];
    {
        use std::io::Read;
        let mut f = std::fs::File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
        f.read_exact(&mut buf)
            .map_err(|e| AppError::Io(e.to_string()))?;
    }
    match demux_bytes(&buf) {
        Ok(d) => Ok(d),
        // faststart 关闭的产物（B 帧流）moov 在文件尾，头部 8MB 只有
        // ftyp+mdat——按顶层 box 跳读定位 moov，整箱读回再解。
        Err(e) if (head_len as u64) < file_size => {
            let moov = read_tail_moov(path, file_size).map_err(|_| e)?;
            demux_bytes(&moov)
        }
        Err(e) => Err(e),
    }
}

/// 顶层 box 逐个跳读定位 `moov`，整箱读回（moov 内是纯表，样本数据
/// 由 stco 的绝对偏移另行读取，与 moov 所在位置无关）。
fn read_tail_moov(path: &Path, file_size: u64) -> AppResult<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
    let mut pos = 0u64;
    while pos + 8 <= file_size {
        f.seek(SeekFrom::Start(pos))
            .map_err(|e| AppError::Io(e.to_string()))?;
        let mut header = [0u8; 8];
        f.read_exact(&mut header)
            .map_err(|e| AppError::Io(e.to_string()))?;
        let size32 = u32::from_be_bytes(header[0..4].try_into().expect("u32 四字节"));
        let (box_size, header_len) = match size32 {
            1 => {
                let mut ext = [0u8; 8];
                f.read_exact(&mut ext)
                    .map_err(|e| AppError::Io(e.to_string()))?;
                (u64::from_be_bytes(ext), 16)
            }
            0 => (file_size - pos, 8), // size 0 = 延伸到文件尾
            n => (u64::from(n), 8),
        };
        if box_size < header_len || pos + box_size > file_size {
            break; // 畸形 box，别跟着跳进坑
        }
        if &header[4..8] == b"moov" {
            let mut moov = vec![0u8; box_size as usize];
            f.seek(SeekFrom::Start(pos))
                .map_err(|e| AppError::Io(e.to_string()))?;
            f.read_exact(&mut moov)
                .map_err(|e| AppError::Io(e.to_string()))?;
            return Ok(moov);
        }
        pos += box_size;
    }
    Err(AppError::Decrypt("找不到 moov box".into()))
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

    #[test]
    fn tail_moov_is_found_when_faststart_is_off() {
        // B 帧流关 faststart 后 moov 在 mdat 之后：单个 9MB 样本把 mdat
        // 顶过 8MB 头部读窗，moov 落到头部窗外——必须能定位尾部 moov
        use crate::domain::mp4::fixtures::{mp4_with_samples, TrackPlan};
        let dir = std::env::temp_dir().join(format!("hg-demux-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("tail-moov.mp4");
        let big = vec![7u8; 9 * 1024 * 1024];
        std::fs::write(&p, mp4_with_samples(&[TrackPlan::video(vec![big])])).unwrap();
        let d = demux_file(&p).expect("尾部 moov 必须能定位并解析");
        let v = d.video_track().expect("应有视频轨");
        assert_eq!(v.info.samples.len(), 1, "样本表应完整");
        assert_eq!(v.info.samples[0].1, 9 * 1024 * 1024_u64, "样本长度应一致");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
