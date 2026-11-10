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

/// 按样本表顺序读样本。文件只打开一次，样本缓冲由调用方复用——
/// 一集有几千个样本，逐样本 `open` + 新分配在解码循环里是纯开销。
pub struct SampleReader {
    file: File,
}

impl SampleReader {
    pub fn open(path: &Path) -> AppResult<Self> {
        let file = File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
        Ok(Self { file })
    }

    /// 把一块样本读进 `buf`（先清空）。`size == 0` 时 `buf` 为空。
    pub fn read_into(&mut self, offset: u64, size: u64, buf: &mut Vec<u8>) -> AppResult<()> {
        buf.clear();
        if size == 0 {
            return Ok(());
        }
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|e| AppError::Io(e.to_string()))?;
        buf.resize(size as usize, 0);
        self.file
            .read_exact(buf)
            .map_err(|e| AppError::Io(e.to_string()))?;
        Ok(())
    }
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
    fn sample_reader_reuses_one_buffer_across_reads() {
        let dir = std::env::temp_dir().join(format!("hg-bvc2-reader-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("临时目录");
        let path = dir.join("samples.bin");
        std::fs::write(&path, [1u8, 2, 3, 4, 5, 6, 7, 8]).expect("写样本");
        let mut reader = SampleReader::open(&path).expect("打开");
        let mut buf = Vec::new();
        reader.read_into(4, 3, &mut buf).expect("后段");
        assert_eq!(buf, [5, 6, 7]);
        reader.read_into(0, 2, &mut buf).expect("回读前段");
        assert_eq!(buf, [1, 2]);
        reader.read_into(0, 0, &mut buf).expect("空样本");
        assert!(buf.is_empty());
        assert!(
            reader.read_into(6, 4, &mut buf).is_err(),
            "越过文件尾应报错"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_config_is_an_error() {
        let moov = build_box(b"moov", &build_box(b"trak", &[]));
        assert!(config_in_moov(&moov).is_err());
    }
}
