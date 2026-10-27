//! 参数集与码流切分：输出类型序列头（`MF_MT_MPEG_SEQUENCE_HEADER`）里的
//! SPS/PPS 提取与 Annex-B 规范化，Annex-B 起始码 / AVCC 长度前缀两种
//! 形态的 NALU 切分。

use windows::Win32::Media::MediaFoundation::*;

/// 输出类型的 `MF_MT_MPEG_SEQUENCE_HEADER` 里取 SPS/PPS（Annex-B）。
/// 序列头可能是起始码形态，也可能是 4 字节长度前缀形态，两种都解析。
pub(super) fn read_type_param_sets(t: &IMFMediaType) -> Option<Vec<u8>> {
    let len = unsafe { t.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER) }.ok()? as usize;
    if len == 0 || len > 1024 * 1024 {
        return None;
    }
    let mut buf = vec![0u8; len];
    unsafe { t.GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut buf, None) }.ok()?;
    let ps = param_sets_annexb(&buf);
    (!ps.is_empty()).then_some(ps)
}

pub(super) fn param_sets_annexb_or_none(raw: &[u8]) -> Option<Vec<u8>> {
    let ps = param_sets_annexb(raw);
    (!ps.is_empty()).then_some(ps)
}

/// 从一段字节（Annex-B 或 AVCC 形态）里抽 SPS（NAL 7）/PPS（NAL 8），
/// 重新拼成规范 Annex-B。
pub(super) fn param_sets_annexb(blob: &[u8]) -> Vec<u8> {
    let mut nalus = split_annexb(blob);
    if nalus.is_empty() {
        nalus = split_avcc(blob);
    }
    let mut out = Vec::new();
    for n in nalus {
        if !n.is_empty() && matches!(n[0] & 0x1f, 7 | 8) {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(n);
        }
    }
    out
}

fn split_annexb(mut b: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let Some((_, data)) = find_start_code(b) else {
        return out;
    };
    b = &b[data..];
    loop {
        match find_start_code(b) {
            Some((s, e)) => {
                out.push(&b[..s]);
                b = &b[e..];
            }
            None => {
                out.push(b);
                break;
            }
        }
    }
    out.retain(|n| !n.is_empty());
    out
}

/// 返回 (起始码起点, NAL 数据起点)。
fn find_start_code(b: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i + 3 < b.len() {
        if b[i] == 0 && b[i + 1] == 0 {
            if b[i + 2] == 1 {
                return Some((i, i + 3));
            }
            if i + 3 < b.len() && b[i + 2] == 0 && b[i + 3] == 1 {
                return Some((i, i + 4));
            }
        }
        i += 1;
    }
    None
}

/// mf/playback.rs 的 `annexb_from_length_prefixed` 是变长前缀（1-4 字节）+
/// 任何解析失败回退原文的解码输入形态；本函数是 moov/参数集侧的定长
/// 4 字节截断语义——两处边界行为有意不同，合并前先对齐两处实测语义。
fn split_avcc(b: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 4 <= b.len() {
        let len = u32::from_be_bytes([b[pos], b[pos + 1], b[pos + 2], b[pos + 3]]) as usize;
        pos += 4;
        if len == 0 || pos + len > b.len() {
            break;
        }
        out.push(&b[pos..pos + len]);
        pos += len;
    }
    out
}
