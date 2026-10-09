//! HEIC 静图纯 Rust 解码：HEIF 容器 → rusty_h265 → JPEG。
//!
//! 封面转码能力阶梯的最后一级（平台 WIC/VT → ffmpeg → 本级），ffmpeg 与
//! 平台解码器都没有的机器靠它保证封面可用——2026-10-09 真机实测：无
//! ffmpeg 机器上封面曾全军覆没（协议层静默 404）。
//!
//! 「HEIC = HEVC 进 HEIF 容器」：`meta` box 里 `pitm` 选主 item、`iinf`
//! 声明类型（hvc1）、`ipco` 存属性（hvcC 参数集）、`iloc` 给出 mdat 里的
//! 字节区间——与 MP4 的 stsd/sample 机制同源，所以参数集解析与视频管线
//! 共用 [`crate::media::hevc::param_nalus_from_hvcc`]，解码复用转码同款
//! rusty_h265 用法。
//!
//! 已知边界（对红果 CDN 样本实测，2026-10-09）：单 `hvc1` item、数据在
//! `mdat`（construction_method 0）。Apple 相机那种 grid 拼贴 HEIC 目前
//! 不支持——遇到时明确报错而不是解出错图；CDN 现网没有这种形态。

use rusty_h265::Decoder;

use crate::error::{AppError, AppResult};
use crate::media::hevc::{param_nalus_from_hvcc, to_annexb, START_CODE};

/// 解出主 item 并编码为 JPEG（质量 85）。
pub fn decode_primary_to_jpeg(data: &[u8]) -> AppResult<Vec<u8>> {
    let item = PrimaryHvc1::locate(data)?;
    let (param_nalus, length_size) = param_nalus_from_hvcc(item.hvcc)?;

    // hvc1 条目与 MP4 样本同构：长度前缀 NAL 串，参数集只在 hvcC 里
    let mut annexb = Vec::with_capacity(
        item.data.len() + 16 + params_len(&param_nalus),
    );
    for nalu in &param_nalus {
        annexb.extend_from_slice(&START_CODE);
        annexb.extend_from_slice(nalu);
    }
    annexb.extend_from_slice(&to_annexb(item.data, length_size));

    let mut decoder = Decoder::new();
    decoder
        .push_annexb(&annexb, Some(0))
        .map_err(|e| AppError::Media(format!("HEIC HEVC 解码失败: {e}")))?;
    decoder.flush();

    // 静图只有一个 IDR 帧；解码器攒齐参数集后才吐帧，循环取到第一帧即止
    let mut frame = None;
    while frame.is_none() {
        match decoder.next_frame() {
            Ok(f) => frame = Some(f),
            Err(_) => break,
        }
    }
    let frame = frame.ok_or_else(|| AppError::Media("HEVC 解码没有产出任何帧".into()))?;

    let (y, u, v, w, h) = extract_yuv420(&frame);
    let rgb = yuv420_to_rgb(&y, &u, &v, w, h);
    encode_jpeg(rgb, w, h)
}

fn params_len(param_nalus: &[Vec<u8>]) -> usize {
    param_nalus.iter().map(|n| n.len() + START_CODE.len()).sum()
}

// ---------------------------------------------------------------- HEIF 解析

/// 主 item（hvc1）的解码所需材料：hvcC 载荷与图像数据字节区间。
struct PrimaryHvc1<'a> {
    hvcc: &'a [u8],
    data: &'a [u8],
}

impl<'a> PrimaryHvc1<'a> {
    /// 从完整文件字节里定位主 item。
    fn locate(data: &'a [u8]) -> AppResult<Self> {
        let mut ftyp_ok = false;
        let mut meta: Option<(usize, usize)> = None;
        for b in boxes_in(data, 0, data.len()) {
            match &b.typ {
                // mif1 是 HEIF 的基品牌，heic/heix 是具体图像形态
                b"ftyp" => {
                    ftyp_ok = has_image_brand(&data[b.payload.clone()]);
                }
                b"meta" => meta = Some((b.payload.start, b.payload.end)),
                _ => {}
            }
        }
        if !ftyp_ok {
            return Err(AppError::Media("不是 HEIF 容器（ftyp 品牌不符）".into()));
        }
        let Some((start, end)) = meta else {
            return Err(AppError::Media("HEIF 缺少 meta box".into()));
        };
        // meta 是 FullBox：version+flags 4 字节后才是子 box
        Self::locate_in_meta(data, start + 4, end)
    }

    fn locate_in_meta(data: &'a [u8], start: usize, end: usize) -> AppResult<Self> {
        let mut primary_id: u32 = 0;
        let mut hvcc: Option<&'a [u8]> = None;
        let mut loc: Option<(u64, u64)> = None; // (绝对偏移, 长度)

        for b in boxes_in(data, start, end) {
            match &b.typ {
                b"pitm" => primary_id = parse_pitm(&data[b.payload.clone()])?,
                b"iloc" => loc = parse_iloc(&data[b.payload.clone()], primary_id)?,
                b"iprp" => {
                    // 属性容器：iprp（普通容器 box）→ ipco → hvcC。多属性时
                    // 取第一个 hvcC——CDN 样本每 item 一份（ipma 一对一关联）
                    for child in boxes_in(data, b.payload.start, b.payload.end) {
                        if child.typ == *b"ipco" {
                            hvcc = find_hvcc(data, child.payload.start, child.payload.end);
                        }
                    }
                }
                _ => {}
            }
        }

        let hvcc = hvcc.ok_or_else(|| AppError::Media("HEIF 缺少 hvcC 属性".into()))?;
        let (offset, length) =
            loc.ok_or_else(|| AppError::Media("HEIF 缺少主 item 的 iloc 条目".into()))?;
        let end = offset
            .checked_add(length)
            .filter(|&e| e <= data.len() as u64)
            .ok_or_else(|| AppError::Media("HEIF item 数据区间越界".into()))?;
        let payload = &data[offset as usize..end as usize];
        if payload.is_empty() {
            return Err(AppError::Media("HEIF 主 item 数据为空".into()));
        }
        Ok(Self { hvcc, data: payload })
    }
}

/// 一个 ISO-BMFF box：类型 + 载荷区间（header 之后，不含 FullBox 的 4 字节）。
struct BoxRef {
    typ: [u8; 4],
    payload: std::ops::Range<usize>,
}

/// 按 ISO/IEC 14496-12 走 `[start, end)` 内的兄弟 box 序列（含 largesize）。
fn boxes_in(data: &[u8], start: usize, end: usize) -> impl Iterator<Item = BoxRef> + '_ {
    let mut pos = start;
    std::iter::from_fn(move || {
        if pos + 8 > end {
            return None;
        }
        let mut size = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
            as u64;
        let mut header = 8usize;
        if size == 1 {
            // largesize：8 字节扩展
            if pos + 16 > end {
                return None;
            }
            size = u64::from_be_bytes([
                data[pos + 8],
                data[pos + 9],
                data[pos + 10],
                data[pos + 11],
                data[pos + 12],
                data[pos + 13],
                data[pos + 14],
                data[pos + 15],
            ]);
            header = 16;
        } else if size == 0 {
            // 到文件尾
            size = (end - pos) as u64;
        }
        if (size as usize) < header || pos + size as usize > end {
            return None;
        }
        let typ = [
            data[pos + 4],
            data[pos + 5],
            data[pos + 6],
            data[pos + 7],
        ];
        let b = BoxRef {
            typ,
            payload: pos + header..pos + size as usize,
        };
        pos += size as usize;
        Some(b)
    })
}

/// ftyp 的主要/兼容品牌里是否含 HEIF 静图品牌。
fn has_image_brand(payload: &[u8]) -> bool {
    payload
        .as_chunks::<4>()
        .0
        .iter()
        .any(|b| matches!(b, b"heic" | b"heix" | b"mif1" | b"heim" | b"heis" | b"msf1"))
}

/// pitm（FullBox）：主 item ID。version 0 是 u16，其余 u32。
fn parse_pitm(payload: &[u8]) -> AppResult<u32> {
    if payload.is_empty() {
        return Err(AppError::Media("pitm 为空".into()));
    }
    if payload[0] == 0 {
        read_u16(payload, 4)
            .map(u32::from)
            .ok_or_else(|| AppError::Media("pitm 长度不足".into()))
    } else {
        read_u32(payload, 4).ok_or_else(|| AppError::Media("pitm 长度不足".into()))
    }
}

/// ipco 子序列里找第一个 hvcC 的载荷。
fn find_hvcc(data: &[u8], start: usize, end: usize) -> Option<&[u8]> {
    boxes_in(data, start, end)
        .find(|b| b.typ == *b"hvcC")
        .map(|b| &data[b.payload])
}

/// iloc（ISO/IEC 14496-12）：主 item 的数据区间。只认 construction_method 0
/// （数据在文件偏移处）；idat 内嵌（method 1）现网没有，遇到明确报错。
fn parse_iloc(payload: &[u8], primary_id: u32) -> AppResult<Option<(u64, u64)>> {
    if payload.len() < 6 {
        return Err(AppError::Media("iloc 长度不足".into()));
    }
    let version = payload[0];
    let offset_size = (payload[4] >> 4) as usize;
    let length_size = (payload[4] & 0x0F) as usize;
    let base_offset_size = (payload[5] >> 4) as usize;
    let index_size = if version >= 1 { (payload[5] & 0x0F) as usize } else { 0 };

    let mut c = Cursor { buf: payload, pos: 6 };
    let item_count = if version < 2 { c.u16()? as u64 } else { c.u32()? as u64 };

    for _ in 0..item_count {
        let id = if version < 2 { c.u16()? as u64 } else { c.u32()? as u64 };
        // v1/v2 在 item_ID 后直接是 construction_method——中间没有 reserved
        // （真机样本钉过：base=808 正好是 mdat 数据起点，少读一个 u16 全对齐）
        let construction_method = if version >= 1 { c.u16()? } else { 0 };
        let _data_ref_index = c.u16()?;
        let base_offset = c.bytes(base_offset_size)?;
        let extent_count = c.u16()? as u64;

        for _ in 0..extent_count {
            if index_size > 0 {
                c.bytes(index_size)?;
            }
            let offset = c.bytes(offset_size)?;
            let length = c.bytes(length_size)?;
            if id == primary_id as u64 {
                if construction_method != 0 {
                    return Err(AppError::Media(
                        "HEIF item 数据在 idat 内嵌（construction_method != 0），暂不支持".into(),
                    ));
                }
                // base_offset 与 extent_offset 是相加关系，不是拼接
                let absolute = base_offset.checked_add(offset).ok_or_else(|| {
                    AppError::Media("HEIF iloc 偏移溢出".into())
                })?;
                return Ok(Some((absolute, length)));
            }
        }
    }
    Ok(None)
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn u16(&mut self) -> AppResult<u16> {
        Ok(self.bytes(2)? as u16)
    }
    fn u32(&mut self) -> AppResult<u32> {
        Ok(self.bytes(4)? as u32)
    }
    /// 读 n 字节并按大端拼成 u64（iloc 的各字段宽度就是动态的）
    fn bytes(&mut self, n: usize) -> AppResult<u64> {
        if self.pos + n > self.buf.len() {
            return Err(AppError::Media("iloc 字段越界".into()));
        }
        let mut v = 0u64;
        for b in &self.buf[self.pos..self.pos + n] {
            v = (v << 8) | *b as u64;
        }
        self.pos += n;
        Ok(v)
    }
}

// ---------------------------------------------------------------- 帧处理

/// rusty_h265 帧 → 8bit 4:2:0 平面（与 transcode/codec 的 extract_yuv 同一
/// 口径：crop 之后的可见区，10bit 右移取高字节）。
fn extract_yuv420(frame: &rusty_h265::Frame) -> (Vec<u8>, Vec<u8>, Vec<u8>, usize, usize) {
    let p = &frame.picture;
    let (crop_x, crop_y, crop_w, crop_h) = p.crop;
    let shift = p.bit_depth_luma.saturating_sub(8);

    let (w, h) = (crop_w, crop_h);
    let (cw, ch) = (w / 2, h / 2);

    let mut y = vec![0u8; w * h];
    for row in 0..h {
        let src = p.planes[0].row(crop_y + row);
        let dst = &mut y[row * w..row * w + w];
        for (i, px) in dst.iter_mut().enumerate() {
            *px = (src[crop_x + i] >> shift) as u8;
        }
    }
    let mut u = vec![0u8; cw * ch];
    let mut v = vec![0u8; cw * ch];
    for row in 0..ch {
        let su = p.planes[1].row(crop_y / 2 + row);
        let sv = p.planes[2].row(crop_y / 2 + row);
        for (i, (du, dv)) in u[row * cw..row * cw + cw]
            .iter_mut()
            .zip(v[row * cw..row * cw + cw].iter_mut())
            .enumerate()
        {
            *du = (su[crop_x / 2 + i] >> shift) as u8;
            *dv = (sv[crop_x / 2 + i] >> shift) as u8;
        }
    }
    (y, u, v, w, h)
}

/// BT.709 limited-range YUV420 → RGB8。定点整数（x8192），封面精度足够；
/// 短剧封面是标准 709 有限域素材，直接按 709 矩阵还原。
fn yuv420_to_rgb(y: &[u8], u: &[u8], v: &[u8], w: usize, h: usize) -> Vec<u8> {
    const S: i32 = 8192;
    // 有限域 → 全域：255/219 与 255/224
    const Y_SCALE: i32 = (255 * S + 109) / 219; // ≈ 9539
    const C_SCALE: i32 = (255 * S + 112) / 224; // ≈ 9326
    // 709 反矩阵系数（x8192 定点）
    const RV: i32 = 12_902; // 1.5748 · Cr → R
    const GU: i32 = 2_819; // 0.3441 · Cb → G（负）
    const GV: i32 = 5_850; // 0.7141 · Cr → G（负）
    const BU: i32 = 15_201; // 1.8556 · Cb → B

    let cw = w / 2;
    let mut rgb = vec![0u8; w * h * 3];
    for row in 0..h {
        let yr = &y[row * w..(row + 1) * w];
        let (ur, vr) = (&u[(row / 2) * cw..(row / 2 + 1) * cw], &v[(row / 2) * cw..(row / 2 + 1) * cw]);
        for col in 0..w {
            // 先各自 >>13 归一到 0..255 / ±127 量级，再乘矩阵系数——
            // 否则「Y 放大 × 矩阵系数」两级定点连乘会溢出 i32
            let luma = ((yr[col] as i32 - 16) * Y_SCALE) >> 13;
            let cb = ((ur[col / 2] as i32 - 128) * C_SCALE) >> 13;
            let cr = ((vr[col / 2] as i32 - 128) * C_SCALE) >> 13;
            let r = luma + ((RV * cr) >> 13);
            let g = luma - ((GU * cb) >> 13) - ((GV * cr) >> 13);
            let b = luma + ((BU * cb) >> 13);
            let out = &mut rgb[(row * w + col) * 3..(row * w + col) * 3 + 3];
            out[0] = r.clamp(0, 255) as u8;
            out[1] = g.clamp(0, 255) as u8;
            out[2] = b.clamp(0, 255) as u8;
        }
    }
    rgb
}

fn encode_jpeg(rgb: Vec<u8>, width: usize, height: usize) -> AppResult<Vec<u8>> {
    let mut out = Vec::new();
    let enc = jpeg_encoder::Encoder::new(&mut out, 85);
    enc.encode(&rgb, width as u16, height as u16, jpeg_encoder::ColorType::Rgb)
        .map_err(|e| AppError::Media(format!("JPEG 编码失败: {e}")))?;
    Ok(out)
}

fn read_u16(buf: &[u8], pos: usize) -> Option<u16> {
    if pos + 2 > buf.len() {
        return None;
    }
    Some(u16::from_be_bytes([buf[pos], buf[pos + 1]]))
}

fn read_u32(buf: &[u8], pos: usize) -> Option<u32> {
    if pos + 4 > buf.len() {
        return None;
    }
    Some(u32::from_be_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实 CDN 样本回归（tests/fixtures/heic-cover-sample.heic，25KB，
    /// fqnovelpic 封面原样）：单 hvc1 item + hvcC/ispe/iloc。整条链
    /// （解析 → 软解 → JPEG）产物必须是合法 JPEG。
    #[test]
    fn decodes_real_cdn_sample_to_jpeg() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/heic-cover-sample.heic");
        let data = std::fs::read(&path).expect("样本文件存在");
        let jpg = decode_primary_to_jpeg(&data).expect("整链解码成功");
        assert_eq!(&jpg[..2], &[0xFF, 0xD8], "产物应是 JPEG（FF D8 魔数）");
        assert!(jpg.len() > 1_000, "封面产物不应是空壳");
    }

    /// 非 HEIF 输入必须在解析层报错，而不是往后传炸弹。
    #[test]
    fn rejects_garbage_input() {
        let err = decode_primary_to_jpeg(b"not a heif file at all......");
        assert!(err.is_err(), "垃圾输入必须报错");
    }
}
