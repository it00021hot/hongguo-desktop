use super::*;
use std::io::{Read, SeekFrom};

const NON_SYNC_BIT: u32 = 0x0001_0000;

pub(super) fn prefix_is_independently_decodable(path: &Path) -> bool {
    read_fragment_video(path)
        .ok()
        .is_some_and(|video| video.first_is_sync && !video.pts.is_empty())
}

/// 按片段顺序读出视频显示时间。损坏的箱返回错误。
pub(super) fn read_fragment_video(path: &Path) -> AppResult<FragmentVideo> {
    let mut file = File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
    let len = file
        .metadata()
        .map_err(|e| AppError::Io(e.to_string()))?
        .len();
    let mut pos = 0u64;
    let mut pts = Vec::new();
    let mut first_is_sync = false;
    let mut saw_sample = false;
    let mut fragment_bases = Vec::new();
    while pos + 8 <= len {
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
            0 => (len - pos, 8u64),
            n => (u64::from(n), 8u64),
        };
        if box_size < header_len || pos + box_size > len {
            return Err(AppError::Media("片段箱长度越界".into()));
        }
        if &header[4..8] == b"moof" {
            let mut payload = vec![0u8; (box_size - header_len) as usize];
            file.read_exact(&mut payload)
                .map_err(|e| AppError::Io(e.to_string()))?;
            let (base, sync) = collect_video(&payload, &mut pts)?;
            if let Some(base) = base {
                fragment_bases.push(base);
            }
            if !saw_sample {
                first_is_sync = sync;
                saw_sample = !pts.is_empty();
            }
        }
        pos += box_size;
    }
    Ok(FragmentVideo {
        pts,
        first_is_sync,
        fragment_bases,
    })
}

fn collect_video(payload: &[u8], pts: &mut Vec<f64>) -> AppResult<(Option<u64>, bool)> {
    let mut base = None;
    let mut first_sync = false;
    let mut saw = false;
    for traf in parse_boxes(payload, 0, payload.len())
        .into_iter()
        .filter(|b| b.is("traf"))
    {
        let body = &payload[traf.start..traf.start + traf.size];
        let children = parse_boxes(body, 0, body.len());
        let Some(tfhd) = children.iter().find(|b| b.is("tfhd")) else {
            continue;
        };
        let tfhd_body = &body[tfhd.start..tfhd.start + tfhd.size];
        if tfhd_body.len() < 8 {
            return Err(AppError::Media("tfhd 过短".into()));
        }
        let track_id = u32::from_be_bytes(tfhd_body[4..8].try_into().expect("4 字节"));
        if track_id != VIDEO_TRACK {
            continue;
        }
        let Some(tfdt) = children.iter().find(|b| b.is("tfdt")) else {
            return Err(AppError::Media("视频片段缺少 tfdt".into()));
        };
        let tfdt_body = &body[tfdt.start..tfdt.start + tfdt.size];
        let dts_base = read_tfdt(tfdt_body)?;
        let Some(trun) = children.iter().find(|b| b.is("trun")) else {
            return Err(AppError::Media("视频片段缺少 trun".into()));
        };
        let trun_body = &body[trun.start..trun.start + trun.size];
        let sync = append_trun(trun_body, dts_base, pts)?;
        if !saw {
            first_sync = sync;
            saw = true;
        }
        base = Some(dts_base);
    }
    Ok((base, first_sync))
}

fn read_tfdt(payload: &[u8]) -> AppResult<u64> {
    if payload.is_empty() {
        return Err(AppError::Media("tfdt 过短".into()));
    }
    if payload[0] == 1 {
        if payload.len() < 12 {
            return Err(AppError::Media("tfdt 过短".into()));
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&payload[4..12]);
        return Ok(u64::from_be_bytes(buf));
    }
    if payload.len() < 8 {
        return Err(AppError::Media("tfdt 过短".into()));
    }
    Ok(u32::from_be_bytes(payload[4..8].try_into().expect("4 字节")) as u64)
}

fn append_trun(payload: &[u8], base: u64, pts: &mut Vec<f64>) -> AppResult<bool> {
    if payload.len() < 8 {
        return Err(AppError::Media("trun 过短".into()));
    }
    let flags = u32::from_be_bytes([0, payload[1], payload[2], payload[3]]);
    let count = u32::from_be_bytes(payload[4..8].try_into().expect("4 字节")) as usize;
    let mut off = 8usize;
    if flags & 0x1 != 0 {
        off += 4;
    }
    if flags & 0x4 != 0 {
        off += 4;
    }
    let has_dur = flags & 0x100 != 0;
    let has_size = flags & 0x200 != 0;
    let has_flags = flags & 0x400 != 0;
    let has_cto = flags & 0x800 != 0;
    let mut dts = base;
    let mut first_sync = false;
    for index in 0..count {
        let dur = if has_dur {
            let v = read_u32(payload, off)?;
            off += 4;
            v
        } else {
            0
        };
        if has_size {
            off += 4;
        }
        let sample_flags = if has_flags {
            let v = read_u32(payload, off)?;
            off += 4;
            v
        } else {
            0
        };
        let cto = if has_cto {
            let v = read_i32(payload, off)?;
            off += 4;
            v
        } else {
            0
        };
        if index == 0 {
            first_sync = sample_flags & NON_SYNC_BIT == 0;
        }
        let stamp = dts as i64 + i64::from(cto);
        pts.push(stamp as f64 / f64::from(MEDIA_TIMESCALE));
        dts += u64::from(dur);
    }
    Ok(first_sync)
}

pub(super) fn read_u32(data: &[u8], at: usize) -> AppResult<u32> {
    let end = at + 4;
    let bytes: [u8; 4] = data
        .get(at..end)
        .ok_or_else(|| AppError::Media("trun 字段越界".into()))?
        .try_into()
        .expect("4 字节");
    Ok(u32::from_be_bytes(bytes))
}

fn read_i32(data: &[u8], at: usize) -> AppResult<i32> {
    Ok(read_u32(data, at)? as i32)
}
