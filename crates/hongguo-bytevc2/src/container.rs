//! 从明文 MP4 取出 `bv2C` 配置记录与视频样本表。
//! 样本偏移是 `stco` 的文件绝对偏移；`moov` 在头或尾都能定位。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use hongguo_common::{AppError, AppResult};
use hongguo_video::r#box::parse_boxes;
use hongguo_video::sample_table::{TrackInfo, collect_tracks};

use crate::codec::{CONFIG_BOX, is_bytevc2};

/// ISO/IEC 14496-15 VisualSampleEntry 固定字段长度（box 载荷起点之后）。
const VISUAL_SAMPLE_ENTRY_PAYLOAD: usize = 78;

/// 明文 MP4 里的 ByteVC2 视频轨。
pub struct Bytevc2Track {
    pub info: TrackInfo,
    /// `bv2C` 载荷（box 头之后）。
    pub config: Vec<u8>,
}

/// 读出 ByteVC2 轨。不是 `bvc2` 时返回错误，调用方不要把它送去 HEVC 解码。
pub fn open_track(path: &Path) -> AppResult<Bytevc2Track> {
    let moov = read_moov(path)?;
    let tracks = collect_tracks(&moov)?;
    let info = tracks
        .into_iter()
        .find(|t| t.is_video)
        .ok_or_else(|| AppError::Media("ByteVC2 容器没有视频轨".into()))?;
    if !is_bytevc2(&info.codec) {
        return Err(AppError::Media(format!(
            "不是 ByteVC2 轨（codec = {}）",
            info.codec
        )));
    }
    let config = config_in_moov(&moov)?;
    Ok(Bytevc2Track { info, config })
}

fn read_moov(path: &Path) -> AppResult<Vec<u8>> {
    let mut file = File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
    let file_size = file
        .metadata()
        .map_err(|e| AppError::Io(e.to_string()))?
        .len();
    let mut pos = 0u64;
    while pos + 8 <= file_size {
        file.seek(SeekFrom::Start(pos))
            .map_err(|e| AppError::Io(e.to_string()))?;
        let mut header = [0u8; 16];
        file.read_exact(&mut header[..8])
            .map_err(|e| AppError::Io(e.to_string()))?;
        let size32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let (box_size, header_len) = match size32 {
            1 => {
                file.read_exact(&mut header[8..16])
                    .map_err(|e| AppError::Io(e.to_string()))?;
                (
                    u64::from_be_bytes(header[8..16].try_into().expect("8 字节")),
                    16u64,
                )
            }
            0 => (file_size - pos, 8u64),
            n => (u64::from(n), 8u64),
        };
        if box_size < header_len || pos + box_size > file_size {
            return Err(AppError::Media("MP4 box 长度越界".into()));
        }
        if &header[4..8] == b"moov" {
            let mut buf = vec![0u8; box_size as usize];
            file.seek(SeekFrom::Start(pos))
                .map_err(|e| AppError::Io(e.to_string()))?;
            file.read_exact(&mut buf)
                .map_err(|e| AppError::Io(e.to_string()))?;
            return Ok(buf);
        }
        pos += box_size;
    }
    Err(AppError::Media("找不到 moov".into()))
}

fn config_in_moov(moov: &[u8]) -> AppResult<Vec<u8>> {
    let payload = box_payload_start(moov).ok_or_else(|| AppError::Media("moov 头损坏".into()))?;
    find_config(moov, payload, moov.len())
        .ok_or_else(|| AppError::Media(format!("stsd 里没有 {CONFIG_BOX}")))
}

fn find_config(data: &[u8], start: usize, end: usize) -> Option<Vec<u8>> {
    for b in parse_boxes(data, start, end) {
        let child_end = b.start + b.size;
        if b.is(CONFIG_BOX) {
            return data.get(b.start..child_end).map(<[u8]>::to_vec);
        }
        if b.is("stsd") {
            let entries = b.start + 8;
            if entries < child_end
                && let Some(found) = find_config(data, entries, child_end)
            {
                return Some(found);
            }
            continue;
        }
        if b.is("bvc2") || b.is("encv") {
            let inner = b.start + VISUAL_SAMPLE_ENTRY_PAYLOAD;
            if inner < child_end
                && let Some(found) = find_config(data, inner, child_end)
            {
                return Some(found);
            }
            continue;
        }
        if is_container(&b)
            && let Some(found) = find_config(data, b.start, child_end)
        {
            return Some(found);
        }
    }
    None
}

fn is_container(b: &hongguo_video::r#box::BoxHeader) -> bool {
    b.is("moov") || b.is("trak") || b.is("mdia") || b.is("minf") || b.is("stbl")
}

fn box_payload_start(data: &[u8]) -> Option<usize> {
    if data.len() < 8 {
        return None;
    }
    let size32 = u32::from_be_bytes(data[0..4].try_into().expect("4 字节"));
    Some(if size32 == 1 { 16 } else { 8 })
}

/// 按样本表从文件读一块样本。
pub fn read_sample(path: &Path, offset: u64, size: u64) -> AppResult<Vec<u8>> {
    if size == 0 {
        return Ok(Vec::new());
    }
    let mut file = File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| AppError::Io(e.to_string()))?;
    let mut buf = vec![0u8; size as usize];
    file.read_exact(&mut buf)
        .map_err(|e| AppError::Io(e.to_string()))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_video::r#box::build_box;

    #[test]
    fn finds_bv2c_inside_sample_entry() {
        let nal = [0x00u8, 0x01, 0x02, 0x03];
        let config_payload = {
            let mut v = vec![0u8, 0b0000_0110, 1, 0];
            v.extend_from_slice(&1u16.to_be_bytes());
            v.extend_from_slice(&(nal.len() as u16).to_be_bytes());
            v.extend_from_slice(&nal);
            while v.len() <= 0x16 {
                v.push(0);
            }
            v
        };
        let bv2c = build_box(b"bv2C", &config_payload);
        let mut entry_payload = vec![0u8; VISUAL_SAMPLE_ENTRY_PAYLOAD];
        entry_payload.extend_from_slice(&bv2c);
        let entry = build_box(b"bvc2", &entry_payload);
        let mut stsd_payload = vec![0u8; 8];
        stsd_payload[7] = 1;
        stsd_payload.extend_from_slice(&entry);
        let stsd = build_box(b"stsd", &stsd_payload);
        let stbl = build_box(b"stbl", &stsd);
        let minf = build_box(b"minf", &stbl);
        let mdia = build_box(b"mdia", &minf);
        let trak = build_box(b"trak", &mdia);
        let moov = build_box(b"moov", &trak);
        let found = config_in_moov(&moov).expect("应找到 bv2C");
        assert_eq!(found, config_payload);
    }

    #[test]
    fn missing_config_is_an_error() {
        let moov = build_box(b"moov", &build_box(b"trak", &[]));
        assert!(config_in_moov(&moov).is_err());
    }
}
