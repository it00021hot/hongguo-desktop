//! ByteVC2 解帧。librempeg `ff_bvc2_decoder` 与 VVC 同核，差别只在
//! `cbs_bvc2_split_fragment` 把 MP4 样本固定按长度前缀拆开；这里先转 Annex-B，
//! 再交给解核。运行时不调用 ffmpeg / librempeg。
//!
//! 默认解核：Fraunhofer libvvdec（Rust `vvdec`）。可用
//! `HONGGUO_BYTEVC2_DECODER=oxideav` 回退到 oxideav-h266（需启用 `oxideav` feature）。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use hongguo_common::{AppError, AppResult};

use crate::container::{self, Bytevc2Track, SampleReader};
use crate::extradata::annexb_from_extradata;

/// 送进解码器的显示时间刻度。与封装层 90kHz 相同，重排后按这个刻度还原秒。
const PRESENTATION_TIMESCALE: i64 = 90_000;

/// 显示时间（秒）→ 90kHz 刻度。相等比较用这个，避免 1/30 的二进制误差。
pub fn presentation_tick(secs: f64) -> i64 {
    (secs * PRESENTATION_TIMESCALE as f64).round() as i64
}

/// 当前实际选用的解核名（供日志/自测）。
pub fn active_decoder_name() -> &'static str {
    match selected_backend() {
        Backend::Vvdec => "vvdec",
        Backend::Oxideav => "oxideav",
    }
}

/// 一帧 8-bit 4:2:0。10/12-bit 源在打包时右移到 8-bit，供平台 HEVC 编码器使用。
///
/// 平面缓冲来自本次解码的缓冲池，帧销毁时自动归还——1080p 一帧约 3 MB，
/// 逐帧向系统要新页再清零比拷贝本身还贵。
pub struct DecodedFrame {
    pub width: usize,
    pub height: usize,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    /// 源样本的显示时间（秒）。样本表给不出逐样本时间时为 `None`，调用方不能当成 0。
    pub pts: Option<f64>,
    pool: Option<Arc<PlanePool>>,
}

impl Drop for DecodedFrame {
    fn drop(&mut self) {
        let Some(pool) = self.pool.take() else {
            return;
        };
        pool.give_luma(std::mem::take(&mut self.y));
        pool.give_chroma(std::mem::take(&mut self.u));
        pool.give_chroma(std::mem::take(&mut self.v));
    }
}

/// 池里最多留多少块空闲亮度缓冲（色度是它的两倍）。在途帧数由调用方的队列深度决定，
/// 这里只是给「归还后暂存」设个上限，防止异常调用方把内存撑大。
const POOL_MAX_LUMA: usize = 16;

/// 解码帧平面缓冲池。亮度、色度尺寸不同，分开存，避免小缓冲被拿去装大平面反复扩容。
#[derive(Default)]
struct PlanePool {
    luma: Mutex<Vec<Vec<u8>>>,
    chroma: Mutex<Vec<Vec<u8>>>,
}

impl PlanePool {
    fn take(list: &Mutex<Vec<Vec<u8>>>) -> Vec<u8> {
        list.lock()
            .ok()
            .and_then(|mut free| free.pop())
            .unwrap_or_default()
    }

    fn give(list: &Mutex<Vec<Vec<u8>>>, buf: Vec<u8>, max: usize) {
        if buf.capacity() == 0 {
            return;
        }
        if let Ok(mut free) = list.lock()
            && free.len() < max
        {
            free.push(buf);
        }
    }

    fn take_luma(&self) -> Vec<u8> {
        Self::take(&self.luma)
    }

    fn take_chroma(&self) -> Vec<u8> {
        Self::take(&self.chroma)
    }

    fn give_luma(&self, buf: Vec<u8>) {
        Self::give(&self.luma, buf, POOL_MAX_LUMA);
    }

    fn give_chroma(&self, buf: Vec<u8>) {
        Self::give(&self.chroma, buf, POOL_MAX_LUMA * 2);
    }
}

/// 解码帧缓冲池的对外句柄。帧契约是「8-bit I420 + 归还即复用的平面缓冲」，
/// 与解核无关——软解核（`hongguo-playback` 经 `hongguo-hevc`）经它出帧，和 ByteVC2 走同一份契约。
#[derive(Clone, Default)]
pub struct FramePool(Arc<PlanePool>);

impl FramePool {
    pub fn new() -> Self {
        Self::default()
    }

    /// 取一块亮度平面缓冲（可能带着上一帧的容量，内容由调用方覆盖）。
    pub fn luma(&self) -> Vec<u8> {
        self.0.take_luma()
    }

    /// 取一块色度平面缓冲。
    pub fn chroma(&self) -> Vec<u8> {
        self.0.take_chroma()
    }

    /// 把填好的三个平面装成一帧；帧销毁时平面回到本池。
    pub fn frame(
        &self,
        width: usize,
        height: usize,
        planes: [Vec<u8>; 3],
        pts: Option<f64>,
    ) -> DecodedFrame {
        let [y, u, v] = planes;
        DecodedFrame {
            width,
            height,
            y,
            u,
            v,
            pts,
            pool: Some(Arc::clone(&self.0)),
        }
    }
}

/// 已打开的明文 ByteVC2。帧率在解帧前就可用，编码会话按它建时间轴。
pub struct Plaintext {
    path: PathBuf,
    track: Bytevc2Track,
    fps: f64,
}

impl Plaintext {
    pub fn fps(&self) -> f64 {
        self.fps
    }

    /// 按显示序逐帧回调。
    pub fn decode(&self, mut on_frame: impl FnMut(DecodedFrame) -> AppResult<()>) -> AppResult<()> {
        decode_track(self.path.as_path(), &self.track, &mut on_frame)
    }
}

/// 打开明文 ByteVC2 MP4。不是 ByteVC2 时返回错误。
pub fn open_plaintext(path: &Path) -> AppResult<Plaintext> {
    let track = container::open_track(path)?;
    let fps = track
        .info
        .average_framerate()
        .filter(|fps| *fps > 0.0)
        .unwrap_or(25.0);
    Ok(Plaintext {
        path: path.to_path_buf(),
        track,
        fps,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backend {
    Vvdec,
    Oxideav,
}

fn selected_backend() -> Backend {
    let want = std::env::var("HONGGUO_BYTEVC2_DECODER")
        .unwrap_or_default()
        .to_ascii_lowercase();
    match want.as_str() {
        "oxideav" | "oxideav-h266" => {
            if cfg!(feature = "oxideav") {
                return Backend::Oxideav;
            }
        }
        "vvdec" | "libvvdec" | "vvdecapp" => {
            if cfg!(feature = "vvdec") {
                return Backend::Vvdec;
            }
        }
        _ => {}
    }
    if cfg!(feature = "vvdec") {
        Backend::Vvdec
    } else if cfg!(feature = "oxideav") {
        Backend::Oxideav
    } else {
        // features 至少会启用一个；这里兜底避免空匹配。
        Backend::Vvdec
    }
}

fn decode_track(
    path: &Path,
    track: &Bytevc2Track,
    on_frame: &mut impl FnMut(DecodedFrame) -> AppResult<()>,
) -> AppResult<()> {
    match selected_backend() {
        Backend::Vvdec => {
            #[cfg(feature = "vvdec")]
            {
                let mut reader = SampleReader::open(path)?;
                decode_track_vvdec(
                    track,
                    0,
                    |offset, size, bytes| reader.read_into(offset, size, bytes),
                    on_frame,
                )
            }
            #[cfg(not(feature = "vvdec"))]
            {
                let _ = (path, track, on_frame);
                Err(AppError::Media(
                    "ByteVC2 解核 vvdec 未编入（缺 feature `vvdec`）".into(),
                ))
            }
        }
        Backend::Oxideav => {
            #[cfg(feature = "oxideav")]
            {
                decode_track_oxideav(path, track, on_frame)
            }
            #[cfg(not(feature = "oxideav"))]
            {
                let _ = (path, track, on_frame);
                Err(AppError::Media(
                    "ByteVC2 解核 oxideav 未编入（缺 feature `oxideav`）".into(),
                ))
            }
        }
    }
}

#[cfg(feature = "vvdec")]
fn vvdec_thread_count() -> i32 {
    if let Ok(raw) = std::env::var("HONGGUO_BYTEVC2_VVDEC_THREADS")
        && let Ok(n) = raw.parse::<i32>()
    {
        // -1 = VVdeC auto；0/正数按库语义。
        return n;
    }
    // 默认 capped，避免与 UI/播放器抢满核；自测可抬高。
    std::thread::available_parallelism()
        .map(|n| (n.get() as i32).clamp(1, 8))
        .unwrap_or(4)
}

/// 从解码序第 `start_index` 个样本起解到轨尾，按显示序逐帧回调。
/// `start_index` 须是关键帧；`read(偏移, 长度, 缓冲)` 由调用方供数（在线 Range 或本地文件）。
#[cfg(feature = "vvdec")]
pub fn decode_track_vvdec(
    track: &Bytevc2Track,
    start_index: usize,
    read: impl FnMut(u64, u64, &mut Vec<u8>) -> AppResult<()>,
    on_frame: &mut impl FnMut(DecodedFrame) -> AppResult<()>,
) -> AppResult<()> {
    let pool = FramePool::new();
    decode_track_vvdec_native(track, start_index, read, &mut |frame| {
        on_frame(legacy_i4208(&frame, &pool)?)
    })
}

#[cfg(feature = "vvdec")]
pub fn decode_track_vvdec_native(
    track: &Bytevc2Track,
    start_index: usize,
    mut read: impl FnMut(u64, u64, &mut Vec<u8>) -> AppResult<()>,
    on_frame: &mut impl FnMut(crate::NativeFrame) -> AppResult<()>,
) -> AppResult<()> {
    let length_size = crate::length_size_from_extradata(&track.config)?;
    let params = annexb_from_extradata(&track.config)?;
    let timeline = track.info.sample_pts();
    let stamp_pts = timeline.len() == track.info.samples.len() && !timeline.is_empty();
    let mut dec = crate::VvdecDecoder::new(vvdec_thread_count())?;
    // 样本缓冲与 Annex-B 缓冲整集复用；vvdec 在 `decode` 返回前已把载荷拷进自己的码流缓冲。
    let mut raw = Vec::new();
    let mut annexb = Vec::new();
    let mut sent_params = false;

    for (index, &(offset, size)) in track.info.samples.iter().enumerate().skip(start_index) {
        read(offset, size, &mut raw)?;
        annexb.clear();
        let first_au = !sent_params;
        if first_au {
            annexb.extend_from_slice(&params);
        }
        let prefix = annexb.len();
        crate::sample::append_annexb_checked(&raw, length_size, &mut annexb)?;
        if annexb.len() == prefix {
            continue;
        }
        sent_params = true;
        if let Some(frame) = dec.decode(&annexb, stamp_pts.then(|| timeline[index]), first_au)? {
            on_frame(frame)?;
        }
    }
    if !sent_params {
        return Err(AppError::Media("ByteVC2 没有可解码的样本".into()));
    }
    while let Some(frame) = dec.flush()? {
        on_frame(frame)?;
    }
    Ok(())
}

#[cfg(feature = "vvdec")]
fn legacy_i4208(frame: &crate::NativeFrame, pool: &FramePool) -> AppResult<DecodedFrame> {
    use crate::ChromaSampling;
    let width = frame.width & !1;
    let height = frame.height & !1;
    if width == 0 || height == 0 {
        return Err(AppError::Media("ByteVC2 解码分辨率无效".into()));
    }
    let bit_depth = frame.bit_depth;
    let shift = match bit_depth {
        8 => 0u8,
        10 => 2,
        other => {
            return Err(AppError::Media(format!("ByteVC2 位深暂不支持: {other}")));
        }
    };
    let with_chroma = match frame.chroma {
        ChromaSampling::Yuv420 => true,
        ChromaSampling::Monochrome => false,
        other => {
            return Err(AppError::Media(format!(
                "ByteVC2 像素格式暂不支持: {other:?}"
            )));
        }
    };
    let y_plane = frame
        .planes
        .first()
        .ok_or_else(|| AppError::Media("ByteVC2 帧缺 Y 平面".into()))?;
    let mut y = pool.luma();
    plane8_into(
        &y_plane.data,
        y_plane.stride,
        width,
        height,
        y_plane.bytes_per_sample as usize,
        shift,
        &mut y,
    )?;
    let cw = width / 2;
    let ch = height / 2;
    let mut u = pool.chroma();
    let mut v = pool.chroma();
    if with_chroma {
        let u_plane = frame
            .planes
            .get(1)
            .ok_or_else(|| AppError::Media("ByteVC2 帧缺 U 平面".into()))?;
        let v_plane = frame
            .planes
            .get(2)
            .ok_or_else(|| AppError::Media("ByteVC2 帧缺 V 平面".into()))?;
        plane8_into(
            &u_plane.data,
            u_plane.stride,
            cw,
            ch,
            u_plane.bytes_per_sample as usize,
            shift,
            &mut u,
        )?;
        plane8_into(
            &v_plane.data,
            v_plane.stride,
            cw,
            ch,
            v_plane.bytes_per_sample as usize,
            shift,
            &mut v,
        )?;
    } else {
        for plane in [&mut u, &mut v] {
            plane.clear();
            plane.resize(cw * ch, 128);
        }
    }
    Ok(pool.frame(width, height, [y, u, v], frame.pts))
}

/// 把一个平面收紧成 8-bit、去掉行尾填充，写进 `out`（先清空，容量复用）。
///
/// 高位深走「小端 u16 右移再封顶」：按行用定长迭代器写，编译器能在 NEON / SSE2 / AVX2 上
/// 自动向量化（逐元素 `push` 带容量检查，向量化不了）。结果与逐像素标量实现逐字节一致。
fn plane8_into(
    data: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    bytes_per_sample: usize,
    shift: u8,
    out: &mut Vec<u8>,
) -> AppResult<()> {
    let bps = if shift == 0 {
        1usize
    } else {
        bytes_per_sample.max(2)
    };
    let need = stride.saturating_mul(height.saturating_sub(1)) + width * bps;
    if data.len() < need || stride < width * bps {
        return Err(AppError::Media("ByteVC2 帧平面越界".into()));
    }
    out.clear();
    out.reserve(width * height);
    if shift == 0 && stride == width {
        out.extend_from_slice(&data[..width * height]);
        return Ok(());
    }
    for row in 0..height {
        let start = row * stride;
        if shift == 0 {
            out.extend_from_slice(&data[start..start + width]);
            continue;
        }
        let src = &data[start..start + width * 2];
        out.extend(
            src.as_chunks::<2>()
                .0
                .iter()
                .map(|px| (u16::from_le_bytes(*px) >> shift).min(255) as u8),
        );
    }
    Ok(())
}

#[cfg(feature = "oxideav")]
fn plane8_raw(
    data: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    bytes_per_sample: usize,
    shift: u8,
) -> AppResult<Vec<u8>> {
    let mut out = Vec::new();
    plane8_into(
        data,
        stride,
        width,
        height,
        bytes_per_sample,
        shift,
        &mut out,
    )?;
    Ok(out)
}

#[cfg(feature = "oxideav")]
fn decode_track_oxideav(
    path: &Path,
    track: &Bytevc2Track,
    on_frame: &mut impl FnMut(DecodedFrame) -> AppResult<()>,
) -> AppResult<()> {
    use oxideav_core::{CodecId, CodecParameters, Decoder, Packet, TimeBase};
    use oxideav_h266::CODEC_ID_STR;
    use oxideav_h266::decoder::H266Decoder;

    let length_size = crate::length_size_from_extradata(&track.config)?;
    let params = annexb_from_extradata(&track.config)?;
    let timeline = track.info.sample_pts();
    let stamp_pts = timeline.len() == track.info.samples.len() && !timeline.is_empty();
    let mut dec = H266Decoder::new(&CodecParameters::video(CodecId::new(CODEC_ID_STR)))
        .map_err(map_oxide_err)?;
    let mut reader = SampleReader::open(path)?;
    let mut raw = Vec::new();
    let mut sent_params = false;
    for (index, &(offset, size)) in track.info.samples.iter().enumerate() {
        reader.read_into(offset, size, &mut raw)?;
        // 回退核的 `Packet` 要拿走载荷所有权，这里每包一块新缓冲。
        let mut annexb = Vec::with_capacity(raw.len() + 16);
        if !sent_params {
            annexb.extend_from_slice(&params);
        }
        let prefix = annexb.len();
        crate::sample::append_annexb_checked(&raw, length_size, &mut annexb)?;
        if annexb.len() == prefix {
            continue;
        }
        sent_params = true;
        let mut packet = Packet::new(0, TimeBase::new(1, PRESENTATION_TIMESCALE), annexb);
        if stamp_pts {
            let ticks = presentation_tick(timeline[index]);
            packet = packet.with_pts(ticks);
        }
        dec.send_packet(&packet).map_err(map_oxide_err)?;
        drain_oxide(&mut dec, on_frame)?;
    }
    if !sent_params {
        return Err(AppError::Media("ByteVC2 没有可解码的样本".into()));
    }
    dec.flush().map_err(map_oxide_err)?;
    drain_oxide(&mut dec, on_frame)?;
    Ok(())
}

#[cfg(feature = "oxideav")]
fn drain_oxide(
    dec: &mut oxideav_h266::decoder::H266Decoder,
    on_frame: &mut impl FnMut(DecodedFrame) -> AppResult<()>,
) -> AppResult<()> {
    use oxideav_core::{Decoder, Error as OxideError, Frame};

    loop {
        match dec.receive_frame() {
            Ok(Frame::Video(frame)) => on_frame(pack_oxide_frame(dec, &frame)?)?,
            Ok(_) => {}
            Err(OxideError::NeedMore | OxideError::Eof) => return Ok(()),
            Err(e) => return Err(map_oxide_err(e)),
        }
    }
}

#[cfg(feature = "oxideav")]
fn pack_oxide_frame(
    dec: &oxideav_h266::decoder::H266Decoder,
    frame: &oxideav_core::VideoFrame,
) -> AppResult<DecodedFrame> {
    use oxideav_core::{Decoder, PixelFormat};

    let format = dec
        .output_pixel_format()
        .ok_or_else(|| AppError::Media("ByteVC2 解码没有像素格式".into()))?;
    let (width, height) = dec
        .output_dimensions()
        .ok_or_else(|| AppError::Media("ByteVC2 解码没有分辨率".into()))?;
    let width = (width as usize) & !1;
    let height = (height as usize) & !1;
    if width == 0 || height == 0 {
        return Err(AppError::Media("ByteVC2 解码分辨率无效".into()));
    }
    let (shift, with_chroma) = match format {
        PixelFormat::Yuv420P => (0u8, true),
        PixelFormat::Yuv420P10Le => (2, true),
        PixelFormat::Yuv420P12Le => (4, true),
        PixelFormat::Gray8 => (0, false),
        other => {
            return Err(AppError::Media(format!(
                "ByteVC2 像素格式暂不支持: {other:?}"
            )));
        }
    };
    let y = plane8_oxide(frame.planes.first(), width, height, shift)?;
    let cw = width / 2;
    let ch = height / 2;
    let (u, v) = if with_chroma {
        (
            plane8_oxide(frame.planes.get(1), cw, ch, shift)?,
            plane8_oxide(frame.planes.get(2), cw, ch, shift)?,
        )
    } else {
        (vec![128u8; cw * ch], vec![128u8; cw * ch])
    };
    let pts = frame
        .pts
        .map(|ticks| ticks as f64 / PRESENTATION_TIMESCALE as f64);
    Ok(DecodedFrame {
        width,
        height,
        y,
        u,
        v,
        pts,
        pool: None,
    })
}

#[cfg(feature = "oxideav")]
fn plane8_oxide(
    plane: Option<&oxideav_core::VideoPlane>,
    width: usize,
    height: usize,
    shift: u8,
) -> AppResult<Vec<u8>> {
    let plane = plane.ok_or_else(|| AppError::Media("ByteVC2 帧缺平面".into()))?;
    plane8_raw(
        &plane.data,
        plane.stride,
        width,
        height,
        if shift == 0 { 1 } else { 2 },
        shift,
    )
}

#[cfg(feature = "oxideav")]
fn map_oxide_err(err: oxideav_core::Error) -> AppError {
    AppError::Media(format!("ByteVC2 解码失败: {err}"))
}
