//! 去除 CENC 加密标记：把样本入口（encv/enca…）还原成 `frma` 记录的原始格式并摘掉 `sinf` 等保护 box——
//! 入口仍是加密类型时播放器会去找 CENC 密钥、找不到就拒绝播放（Windows 报 `0xC00D36C4` 就是这么来的）。
//! 摘 box 会让 mdat 整体前移，`stco`/`co64` 必须同步平移同一差值——漏改一处播放器读到错位数据且**不报错**；⚠️ 必须先解密采样数据再调用本模块。

use hongguo_common::{AppError, AppResult};

use super::r#box::{build_box, find_box, parse_boxes};

/// 需要摘掉的保护信息 box。`saiz`/`saio`/`senc` 是采样辅助信息，
/// 对已解密的轨道没有意义，一并去掉以免播放器仍按加密流处理。
const PROTECTION_BOXES: &[&[u8; 4]] = &[b"sinf", b"saiz", b"saio", b"senc"];

/// 加密样本入口类型 → 未加密时的类型。
const ENCRYPTED_ENTRIES: &[&[u8; 4]] = &[b"encv", b"enca", b"encs", b"enct"];

/// VisualSampleEntry 固定载荷（不含 8 字节 box 头）：ISO/IEC 14496-12。
/// 子 box（`hvcC` / `bv2C` / `sinf`…）从 `8 +` 本值起。
const VISUAL_SAMPLE_ENTRY_PAYLOAD: usize = 78;

/// AudioSampleEntry 固定载荷（不含 8 字节 box 头）。
const AUDIO_SAMPLE_ENTRY_PAYLOAD: usize = 28;

/// 把已经解密过采样数据的 MP4 还原成普通可播放的 MP4，返回重写后的完整文件字节（长度会变，通常变短）。
pub fn deprotect(data: &[u8]) -> AppResult<Vec<u8>> {
    let Some(rebuilt) = rebuild_moov_region(data, 0)? else {
        // 没有加密标记说明本来就是明文（整段已解密或非 CENC 封装），原样返回而不是错误
        log::info!("[DeProtect] 没有加密标记，按明文原样保留");
        return Ok(data.to_vec());
    };
    if rebuilt.bytes.len() == rebuilt.end - rebuilt.start {
        return Ok(data.to_vec());
    }
    let mut out = Vec::with_capacity(data.len());
    out.extend_from_slice(&data[..rebuilt.start]);
    out.extend_from_slice(&rebuilt.bytes);
    out.extend_from_slice(&data[rebuilt.end..]);
    Ok(out)
}

/// 重建后的 moov 及其在密文中的位置。
pub struct RebuiltMoov {
    /// 重建后的 moov box（含 8 字节头），stco 已按「原位替换」布局平移
    pub bytes: Vec<u8>,
    /// 旧 moov box 在**完整文件**中的起点（含头）
    pub start: usize,
    /// 旧 moov box 在完整文件中的结束偏移（不含）
    pub end: usize,
}

/// 重建 moov：加密入口还原 + 摘保护 box + stco 平移；`region` 为从旧 moov box 头开始的密文切片，`base` 为其绝对偏移。
/// stco 值是绝对文件偏移，平移阈值必须用绝对值比（只给切片时尾部-moov 的样本会被错误平移）；
/// 「原位替换」指新 moov 顶旧 moov 位置、其后内容整体平移 `新长 − 旧长`；返回 `None` 表示没有加密标记。
pub fn rebuild_moov_region(region: &[u8], base: usize) -> AppResult<Option<RebuiltMoov>> {
    let moov = find_box(region, 0, region.len(), "moov")
        .ok_or_else(|| AppError::Media("文件里没有 moov，不是可解析的 MP4".into()))?;
    let moov_start = moov.start - 8;
    let moov_end = moov.start + moov.size;

    let Some(mut new_moov) = rebuild_video_stsd(region, moov_start, moov_end)? else {
        return Ok(None);
    };
    let delta = new_moov.len() as i64 - (moov_end - moov_start) as i64;
    if delta != 0 {
        // 偏移表在 moov 内部，先在新的 moov 上平移，再拼回文件
        shift_chunk_offsets(&mut new_moov, base + moov_end, delta);
    }
    Ok(Some(RebuiltMoov {
        bytes: new_moov,
        start: base + moov_start,
        end: base + moov_end,
    }))
}

/// 重建 moov：把 `stsd` 里的加密入口换成普通入口；返回 `None` 表示整部片子里没有加密标记。
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

/// `container` 的直接子 box，返回 `(子 box 完整字节, 类型)`；`container` **含自身 8 字节头**（本模块的递归就是这么切的）。
/// 子 box 从偏移 8 开始——写成 0 会把容器自己当成第一个子 box，递归随即原地踏步。
fn children(container: &[u8]) -> Vec<(&[u8], [u8; 4])> {
    let mut v: Vec<(&[u8], [u8; 4])> = parse_boxes(container, 8, container.len())
        .into_iter()
        .map(|b| (&container[b.start - 8..b.start + b.size], b.kind))
        .collect();
    v.shrink_to_fit();
    v
}

/// 重建单个 trak：改写其中的 stsd。音视频都要处理——平台流里音轨同样是 `enca`，只改视频轨会残留一半加密标记。
fn rebuild_trak(trak: &[u8], changed: &mut bool) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(trak.len());
    for (child, kind) in children(trak) {
        if &kind == b"mdia" {
            out.extend_from_slice(&rebuild_mdia(child, changed));
        } else {
            out.extend_from_slice(child);
        }
    }
    // 连 box 头一起返回：调用方把它当**完整子 box** 拼回去——只还内容每层少 8 字节，结构静默错位不报错
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
            // CENC 采样辅助信息解密后已无意义，留着会让播放器继续按加密流处理
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

/// 样本入口子 box 起点（含 8 字节头）。音视频固定字段长度不同，从 16 扫会把
/// width/height 等当成 box 头，既读丢 `frma`（回落成错误的 `hvc1`），也剥不掉 `sinf`。
fn sample_entry_children_at(entry: &[u8]) -> usize {
    match &entry[4..8] {
        b"enca" => 8 + AUDIO_SAMPLE_ENTRY_PAYLOAD,
        _ => 8 + VISUAL_SAMPLE_ENTRY_PAYLOAD,
    }
}

/// 加密入口 → 普通入口：改类型、摘掉保护信息。
fn plain_entry(entry: &[u8]) -> Vec<u8> {
    let children_at = sample_entry_children_at(entry);
    if entry.len() < children_at {
        return entry.to_vec();
    }
    // 类型取 `sinf/frma` 里记录的原始格式；取不到就用编码名兜底
    let original = original_format(entry).unwrap_or_else(|| match &entry[4..8] {
        b"enca" => *b"mp4a",
        _ => *b"hvc1",
    });

    let mut children: Vec<u8> = Vec::with_capacity(entry.len().saturating_sub(children_at));
    for b in parse_boxes(entry, children_at, entry.len()) {
        if PROTECTION_BOXES.contains(&&b.kind) {
            continue;
        }
        let start = b.start - 8;
        children.extend_from_slice(&entry[start..b.start + b.size]);
    }

    let mut head = entry[..children_at].to_vec();
    head[4..8].copy_from_slice(&original);
    let total = (head.len() + children.len()) as u32;
    head[..4].copy_from_slice(&total.to_be_bytes());
    head.extend_from_slice(&children);
    head
}

/// `sinf` → `frma` 的载荷前 4 字节就是加密前的原始样本格式。
fn original_format(entry: &[u8]) -> Option<[u8; 4]> {
    let children_at = sample_entry_children_at(entry);
    if entry.len() < children_at {
        return None;
    }
    let sinf = parse_boxes(entry, children_at, entry.len())
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
/// `stco` / `co64` 位于 `stbl` **内部**，要沿 moov 的层级往下走；漏改一处播放器就会读到错位数据而且不报错。
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
        // 只平移指向 moov 之后的偏移；宁可偏移不完美也不能把指向 box 内部的偏移改坏
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
