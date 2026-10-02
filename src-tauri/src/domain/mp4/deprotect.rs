//! 去除 CENC 加密标记。
//!
//! 解密采样数据只做了一半：视频轨的样本入口仍然是 `encv`，`sinf`/`schm`/`tenc`
//! 也都还在。按 MP4 规范这就等于「声明自己是一段加密视频」，播放器会去找
//! CENC 密钥、找不到就拒绝播放（Windows 报 `0xC00D36C4` 就是这么来的）。
//!
//! 这里把入口类型还原成 `frma` 里记录的原始格式（通常 `hvc1`）并摘掉 `sinf`。
//! 摘 box 会让 `moov` 变短，`mdat` 整体前移，所以 `stco`/`co64` 里的
//! 每个 chunk 偏移都必须同步减去同一个差值——漏改一处，播放器就会读到
//! 错位数据而且**不报错**，只会花屏或卡死。
//!
//! ⚠️ 顺序要求：必须先解密采样数据、再调用本模块；反过来会把密钥信息一起抹掉。

use crate::error::{AppError, AppResult};

use super::r#box::{build_box, find_box, parse_boxes};

/// 需要摘掉的保护信息 box。`saiz`/`saio`/`senc` 是采样辅助信息，
/// 对已解密的轨道没有意义，一并去掉以免播放器仍按加密流处理。
const PROTECTION_BOXES: &[&[u8; 4]] = &[b"sinf", b"saiz", b"saio", b"senc"];

/// 加密样本入口类型 → 未加密时的类型。
const ENCRYPTED_ENTRIES: &[&[u8; 4]] = &[b"encv", b"enca", b"encs", b"enct"];

/// 把已经解密过采样数据的 MP4 还原成普通可播放的 MP4。
///
/// 返回重写后的完整文件字节。长度会变（通常变短）。
pub fn deprotect(data: &[u8]) -> AppResult<Vec<u8>> {
    let moov = find_box(data, 0, data.len(), "moov")
        .ok_or_else(|| AppError::Media("文件里没有 moov，不是可解析的 MP4".into()))?;
    let moov_start = moov.start - 8; // 载荷起点减去 8 字节头
    let moov_end = moov.start + moov.size;

    let Some(new_moov) = rebuild_video_stsd(data, moov_start, moov_end)? else {
        // 没有加密标记说明本来就是明文，原样返回——不是错误。
        // 整段已解密的流、或非 CENC 封装的流都会走到这里。
        log::info!("[DeProtect] 没有加密标记，按明文原样保留");
        return Ok(data.to_vec());
    };
    let delta = new_moov.len() as i64 - (moov_end - moov_start) as i64;
    if delta == 0 {
        return Ok(data.to_vec());
    }
    // 偏移表在 moov 内部，先在新的 moov 上平移，再拼回文件
    let mut new_moov = new_moov;
    shift_chunk_offsets(&mut new_moov, moov_end, delta);

    let mut out = Vec::with_capacity(data.len());
    out.extend_from_slice(&data[..moov_start]);
    out.extend_from_slice(&new_moov);
    // moov 之后的内容（mdat 等）原样跟上
    out.extend_from_slice(&data[moov_end..]);
    Ok(out)
}

/// 重建 moov：把视频轨 `stsd` 里的加密入口换成普通入口。
///
/// 返回 `None` 表示整部片子里没有加密标记，无需重建。
fn rebuild_video_stsd(
    data: &[u8],
    moov_start: usize,
    moov_end: usize,
) -> AppResult<Option<Vec<u8>>> {
    let container = &data[moov_start..moov_end];
    let mut out: Vec<u8> = Vec::with_capacity(moov_end - moov_start - 8);
    let mut changed = false;

    for (child, kind) in children(container) {
        if &kind == b"trak" {
            out.extend_from_slice(&rebuild_trak(child, &mut changed));
        } else {
            out.extend_from_slice(child);
        }
    }

    if !changed {
        return Ok(None);
    }
    Ok(Some(build_box(b"moov", &out)))
}

/// `container` 的直接子 box，返回 `(子 box 完整字节, 类型)`。
///
/// ⚠️ `container` **含自身的 8 字节头**（本模块的递归就是这么切的）。
/// 子 box 从偏移 8 开始——起点写成 0 会把容器自己当成第一个子 box，
/// 递归随即原地踏步，这个坑踩过一次。
fn children(container: &[u8]) -> Vec<(&[u8], [u8; 4])> {
    let mut v: Vec<(&[u8], [u8; 4])> = parse_boxes(container, 8, container.len())
        .into_iter()
        .map(|b| (&container[b.start - 8..b.start + b.size], b.kind))
        .collect();
    v.shrink_to_fit();
    v
}

/// 重建单个 trak：改写其中的 stsd。
///
/// 音视频都要处理——实测平台流里音轨同样是 `enca`，只改视频轨的话
/// 产物里会残留一半的加密标记，播放器照样拒绝。
fn rebuild_trak(trak: &[u8], changed: &mut bool) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(trak.len());
    for (child, kind) in children(trak) {
        if &kind == b"mdia" {
            out.extend_from_slice(&rebuild_mdia(child, changed));
        } else {
            out.extend_from_slice(child);
        }
    }
    // 连 box 头一起返回：调用方把它当**完整子 box** 拼回去。
    // 只还内容的话每层都少 8 字节，结构会静默错位——不报错，只是花屏。
    build_box(b"trak", &out)
}

fn rebuild_mdia(mdia: &[u8], changed: &mut bool) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(mdia.len());
    for (child, kind) in children(mdia) {
        if &kind == b"minf" {
            out.extend_from_slice(&rebuild_minf(child, changed));
        } else {
            out.extend_from_slice(child);
        }
    }
    build_box(b"mdia", &out)
}

fn rebuild_minf(minf: &[u8], changed: &mut bool) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(minf.len());
    for (child, kind) in children(minf) {
        if &kind == b"stbl" {
            out.extend_from_slice(&rebuild_stbl(child, changed));
        } else {
            out.extend_from_slice(child);
        }
    }
    build_box(b"minf", &out)
}

fn rebuild_stbl(stbl: &[u8], changed: &mut bool) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(stbl.len());
    for (child, kind) in children(stbl) {
        if &kind == b"stsd" {
            out.extend_from_slice(&rebuild_stsd(child, changed));
        } else if PROTECTION_BOXES.contains(&&kind) {
            // CENC 的采样辅助信息在 stbl 里是平级 box，解密后已无意义，
            // 留着会让播放器继续按加密流处理
            *changed = true;
        } else {
            out.extend_from_slice(child);
        }
    }
    build_box(b"stbl", &out)
}

/// `stsd` = `[4 版本/标志][4 入口数][入口…]`，入口 = `[4 大小][4 类型][6 保留][2 data_ref][子 box…]`
fn rebuild_stsd(stsd: &[u8], changed: &mut bool) -> Vec<u8> {
    if stsd.len() < 16 {
        return stsd.to_vec();
    }
    let entry_count = u32::from_be_bytes([stsd[12], stsd[13], stsd[14], stsd[15]]) as usize;

    let mut entries: Vec<u8> = Vec::with_capacity(stsd.len());
    entries.extend_from_slice(&stsd[8..16]); // 版本/标志 + 入口数

    let mut pos = 16usize;
    for _ in 0..entry_count {
        if pos + 16 > stsd.len() {
            break;
        }
        let size =
            u32::from_be_bytes([stsd[pos], stsd[pos + 1], stsd[pos + 2], stsd[pos + 3]]) as usize;
        let end = if size == 0 { stsd.len() } else { pos + size };
        if size < 16 || end > stsd.len() {
            break;
        }
        let kind = [stsd[pos + 4], stsd[pos + 5], stsd[pos + 6], stsd[pos + 7]];
        let entry = &stsd[pos..end];

        if ENCRYPTED_ENTRIES.contains(&&kind) {
            entries.extend_from_slice(&plain_entry(entry));
            *changed = true;
        } else {
            entries.extend_from_slice(entry);
        }
        pos = end;
    }

    build_box(b"stsd", &entries)
}

/// 加密入口 → 普通入口：改类型、摘掉保护信息。
fn plain_entry(entry: &[u8]) -> Vec<u8> {
    // 类型取 `sinf/frma` 里记录的原始格式；取不到就用编码名兜底
    let original = original_format(entry).unwrap_or_else(|| {
        // encv 几乎总是 HEVC；其余按类型推断
        match &entry[4..8] {
            b"enca" => *b"mp4a",
            _ => *b"hvc1",
        }
    });

    let mut children: Vec<u8> = Vec::with_capacity(entry.len());
    for b in parse_boxes(entry, 16, entry.len()) {
        if PROTECTION_BOXES.contains(&&b.kind) {
            continue;
        }
        let start = b.start - 8;
        children.extend_from_slice(&entry[start..b.start + b.size]);
    }

    let mut head: Vec<u8> = Vec::with_capacity(16);
    head.extend_from_slice(&[0u8; 4]); // 入口大小占位
    head.extend_from_slice(&original);
    head.extend_from_slice(&entry[8..16]); // 6 字节保留 + 2 字节 data_ref

    let total = (head.len() + children.len()) as u32;
    head[..4].copy_from_slice(&total.to_be_bytes());
    head.extend_from_slice(&children);
    head
}

/// `sinf` → `frma` 的载荷前 4 字节就是加密前的原始样本格式。
fn original_format(entry: &[u8]) -> Option<[u8; 4]> {
    let sinf = parse_boxes(entry, 16, entry.len())
        .into_iter()
        .find(|b| b.kind == *b"sinf")?;
    let frma = parse_boxes(entry, sinf.start, sinf.start + sinf.size)
        .into_iter()
        .find(|b| b.kind == *b"frma")?;
    if frma.size < 4 {
        return None;
    }
    Some([
        entry[frma.start],
        entry[frma.start + 1],
        entry[frma.start + 2],
        entry[frma.start + 3],
    ])
}

/// moov 长度变化后，把所有指向 moov 之后的 chunk 偏移平移 `delta`。
///
/// `stco` / `co64` 位于 `stbl` **内部**，所以要沿着 moov 的层级往下走，
/// 而不是去 moov 外面找。漏改一处播放器就会读到错位数据而且不报错。
fn shift_chunk_offsets(moov: &mut [u8], threshold: usize, delta: i64) {
    fn walk(moov: &mut [u8], start: usize, end: usize, threshold: usize, delta: i64) {
        for b in parse_boxes(moov, start, end) {
            match &b.kind {
                b"stco" => patch_offsets(moov, b.start, b.size, 4, threshold, delta),
                b"co64" => patch_offsets(moov, b.start, b.size, 8, threshold, delta),
                b"trak" | b"mdia" | b"minf" | b"stbl" => {
                    walk(moov, b.start, b.start + b.size, threshold, delta)
                }
                _ => {}
            }
        }
    }
    walk(moov, 8, moov.len(), threshold, delta);
}

/// `stco` = `[4 版本/标志][4 条数][条目…]`；`co64` 条目是 8 字节。
fn patch_offsets(
    moov: &mut [u8],
    payload_start: usize,
    payload_size: usize,
    entry_bytes: usize,
    threshold: usize,
    delta: i64,
) {
    if payload_size < 8 {
        return;
    }
    let count = u32::from_be_bytes([
        moov[payload_start + 4],
        moov[payload_start + 5],
        moov[payload_start + 6],
        moov[payload_start + 7],
    ]) as usize;

    for i in 0..count {
        let at = payload_start + 8 + i * entry_bytes;
        if at + entry_bytes > payload_start + payload_size || at + entry_bytes > moov.len() {
            return;
        }
        let value: u64 = if entry_bytes == 4 {
            u32::from_be_bytes([moov[at], moov[at + 1], moov[at + 2], moov[at + 3]]) as u64
        } else {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&moov[at..at + 8]);
            u64::from_be_bytes(buf)
        };
        // 只平移指向 moov 之后的偏移；落在 moov 内部的（不该有）保持不动，
        // 宁可偏移不完美也不能把指向 box 内部的偏移改坏
        if value < threshold as u64 {
            continue;
        }
        let shifted = (value as i64 + delta).max(0) as u64;
        if entry_bytes == 4 {
            moov[at..at + 4].copy_from_slice(&(shifted as u32).to_be_bytes());
        } else {
            moov[at..at + 8].copy_from_slice(&shifted.to_be_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::mp4::sample_table::collect_tracks;

    /// 拼接若干 box 的字节（`&Vec<u8>` 不能直接 `concat`，统一走这里）。
    fn cat(parts: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for p in parts {
            out.extend_from_slice(p);
        }
        out
    }

    /// 造一个最小的加密 MP4：`moov(vide trak(stbl(stsd(encv+sinf), stco), ...)) + mdat`
    fn encrypted_mp4(sample: &[u8]) -> Vec<u8> {
        let frma = build_box(b"frma", b"hvc1");
        let schm = build_box(b"schm", &cat(&[&[0u8; 4], b"cenc", &[0u8; 4]]));
        let schi = build_box(b"schi", &build_box(b"tenc", &[0u8; 4]));
        let sinf = build_box(b"sinf", &cat(&[&frma, &schm, &schi]));
        let hvcc = build_box(b"hvcC", &[0u8; 8]);
        let btrt = build_box(b"btrt", &[0u8; 12]);

        // stsd 条目：大小占位 + encv + 6 保留 + 2 data_ref + hvcC + btrt + sinf
        let mut entry: Vec<u8> = Vec::new();
        entry.extend_from_slice(&[0u8; 4]);
        entry.extend_from_slice(b"encv");
        entry.extend_from_slice(&[0u8; 6]);
        entry.extend_from_slice(&[0u8; 2]);
        entry.extend_from_slice(&hvcc);
        entry.extend_from_slice(&btrt);
        entry.extend_from_slice(&sinf);
        let entry_size = entry.len() as u32;
        entry[..4].copy_from_slice(&entry_size.to_be_bytes());

        let stsd = build_box(b"stsd", &cat(&[&[0u8; 4], &1u32.to_be_bytes(), &entry]));
        let stsz = build_box(
            b"stsz",
            &cat(&[
                &[0u8; 4],
                &(sample.len() as u32).to_be_bytes(),
                &1u32.to_be_bytes(),
                &(sample.len() as u32).to_be_bytes(),
            ]),
        );
        let stsc = build_box(
            b"stsc",
            &cat(&[
                &[0u8; 4],
                &1u32.to_be_bytes(),
                &1u32.to_be_bytes(),
                &1u32.to_be_bytes(),
                &1u32.to_be_bytes(),
            ]),
        );
        let stco = build_box(
            b"stco",
            &cat(&[&[0u8; 4], &1u32.to_be_bytes(), &0u32.to_be_bytes()]), // 偏移稍后回填
        );

        let stbl = build_box(b"stbl", &cat(&[&stsd, &stsz, &stsc, &stco]));
        let vmhd = build_box(b"vmhd", &[0u8; 12]);
        let dref = build_box(
            b"dref",
            &cat(&[
                &[0u8; 4],
                &1u32.to_be_bytes(),
                &12u32.to_be_bytes(),
                b"url ",
                &[0u8; 4],
            ]),
        );
        let dinf = build_box(b"dinf", &dref);
        let minf = build_box(b"minf", &cat(&[&vmhd, &dinf, &stbl]));
        let hdlr = build_box(b"hdlr", &cat(&[&[0u8; 8], b"vide", &[0u8; 12]]));
        let mdhd = build_box(b"mdhd", &[0u8; 24]);
        let mdia = build_box(b"mdia", &cat(&[&mdhd, &hdlr, &minf]));
        let tkhd = build_box(
            b"tkhd",
            &cat(&[&[0u8; 12], &1u32.to_be_bytes(), &[0u8; 40]]),
        );
        let trak = build_box(b"trak", &cat(&[&tkhd, &mdia]));
        let moov = build_box(b"moov", &trak);
        let mdat = build_box(b"mdat", sample);

        // stco 偏移在文件中的位置：**搜出来**，不要手算。
        // 手算过一次，偏了 8 字节，把 box 类型覆盖成乱码，排查时极具迷惑性。
        // find_box 会递归下钻；parse_boxes 只看顶层，找不到 moov 里的 stco。
        let ftyp = build_box(b"ftyp", b"isom\x00\x00\x02\x00isomiso2");
        let mut data = ftyp;
        let sample_at = (data.len() + moov.len() + 8) as u32;
        data.extend_from_slice(&moov);
        data.extend_from_slice(&mdat);

        let stco = find_box(&data, 0, data.len(), "stco").expect("构造的样本里应有 stco");
        // 载荷是 [版本/标志 4][条数 4][条目…]，第一个条目在 +8
        let entry_at = stco.start + 8;
        data[entry_at..entry_at + 4].copy_from_slice(&sample_at.to_be_bytes());
        data
    }

    #[test]
    fn turns_encv_into_hvc1_and_drops_sinf() {
        let mp4 = encrypted_mp4(&[0x11u8; 64]);
        let out = deprotect(&mp4).expect("应能去除加密标记");

        let has = |tag: &[u8; 4]| out.windows(4).any(|w| w == tag);
        assert!(has(b"hvc1"), "入口应还原成 hvc1");
        assert!(!has(b"encv"), "不该再出现 encv");
        assert!(!has(b"sinf"), "不该再出现 sinf");
        assert!(!has(b"tenc"), "不该再出现 tenc");
        assert!(!has(b"cenc"), "不该再出现 cenc 方案名");
        // 非保护 box 要保留
        assert!(has(b"hvcC"));
        assert!(has(b"btrt"));
        assert!(has(b"stco"));
    }

    #[test]
    fn chunk_offsets_still_point_at_the_sample() {
        let sample = [0x77u8; 128];
        let mp4 = encrypted_mp4(&sample);
        let out = deprotect(&mp4).unwrap();

        // 重写后 moov 变短，mdat 前移；stco 必须同步平移，
        // 否则这里读到的就不是样本数据了
        let tracks = collect_tracks(&out).expect("产物应可解析");
        let (offset, size) = tracks[0].samples[0];
        assert_eq!(size as usize, sample.len());
        let offset = offset as usize;
        assert_eq!(
            &out[offset..offset + 4],
            &sample[..4],
            "stco 偏移应仍指向样本数据"
        );
    }

    #[test]
    fn plain_file_passes_through_untouched() {
        // 没有加密标记 = 本来就是明文，原样返回而不是报错
        let ftyp = build_box(b"ftyp", b"isom");
        let moov = build_box(b"moov", &build_box(b"mvhd", &[0u8; 100]));
        let mut data = ftyp;
        data.extend_from_slice(&moov);
        let out = deprotect(&data).expect("明文文件应原样通过");
        assert_eq!(out, data, "明文文件不应被改写");
    }

    #[test]
    fn garbage_input_errors() {
        assert!(deprotect(&[0xffu8; 64]).is_err());
    }
}
