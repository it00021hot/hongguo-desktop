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
    track_from_moov(&moov)
}

/// 从已读入内存的 `moov` 取 ByteVC2 轨（在线流没有文件可开，直播放索引用这个形态）。
pub fn track_from_moov(moov: &[u8]) -> AppResult<Bytevc2Track> {
    let tracks = collect_tracks(moov)?;
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
    let config = config_in_moov(moov)?;
    Ok(Bytevc2Track { info, config })
}

fn read_moov(path: &Path) -> AppResult<Vec<u8>> {
    let mut reader = SampleReader::open(path)?;
    let file_size = reader
        .file
        .metadata()
        .map_err(|e| AppError::Io(e.to_string()))?
        .len();
    read_moov_at(file_size, |offset, size, bytes| {
        reader.read_into(offset, size, bytes)
    })
    .map(|(_, moov)| moov)
}

/// 用回调逐个读顶层 box 头，找到 `moov` 后整块读入，返回 `(moov 的文件偏移, moov 字节)`。
/// `moov` 在文件头或尾都能找到；不依赖数据来源，也不依赖编码。
pub fn read_moov_at(
    file_size: u64,
    mut read: impl FnMut(u64, u64, &mut Vec<u8>) -> AppResult<()>,
) -> AppResult<(u64, Vec<u8>)> {
    let mut pos = 0u64;
    let mut header = Vec::with_capacity(16);
    while file_size.saturating_sub(pos) >= 8 {
        read(pos, 8, &mut header)?;
        if header.len() != 8 {
            return Err(AppError::Media("MP4 box 头不完整".into()));
        }
        let size32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let is_moov = &header[4..8] == b"moov";
        let (box_size, header_len) = match size32 {
            1 => {
                if file_size - pos < 16 {
                    return Err(AppError::Media("MP4 扩展头长度越界".into()));
                }
                read(pos + 8, 8, &mut header)?;
                if header.len() != 8 {
                    return Err(AppError::Media("MP4 扩展头不完整".into()));
                }
                (
                    u64::from_be_bytes(header[..8].try_into().expect("8 字节")),
                    16u64,
                )
            }
            0 => (file_size - pos, 8u64),
            n => (u64::from(n), 8u64),
        };
        if box_size < header_len || box_size > file_size - pos {
            return Err(AppError::Media("MP4 box 长度越界".into()));
        }
        if is_moov {
            read(pos, box_size, &mut header)?;
            if header.len() as u64 != box_size {
                return Err(AppError::Media("MP4 moov 不完整".into()));
            }
            return Ok((pos, header));
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
