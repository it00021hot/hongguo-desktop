//! ByteVC2 解帧。librempeg `ff_bvc2_decoder` 与 VVC 同核，差别只在
//! `cbs_bvc2_split_fragment` 把 MP4 样本固定按长度前缀拆开；这里先转 Annex-B，
//! 再交给解核。运行时不调用 ffmpeg / librempeg。
//!
//! 默认解核：Fraunhofer libvvdec（Rust `vvdec`）。可用
//! `HONGGUO_BYTEVC2_DECODER=oxideav` 回退到 oxideav-h266（需启用 `oxideav` feature）。

use std::path::{Path, PathBuf};

use hongguo_common::{AppError, AppResult};

use crate::container::{self, Bytevc2Track};
use crate::extradata::annexb_from_extradata;
use crate::sample::sample_to_annexb;

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

/// 一帧 8-bit 4:2:0。10/12-bit 源在打包时右移到 8-bit，供平台 H.264 编码器使用。
pub struct DecodedFrame {
    pub width: usize,
    pub height: usize,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    /// 源样本的显示时间（秒）。样本表给不出逐样本时间时为 `None`，调用方不能当成 0。
    pub pts: Option<f64>,
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

/// 解码明文 ByteVC2 MP4，按显示序逐帧回调。返回容器平均帧率（读不到时 25）。
pub fn decode_plaintext_mp4(
    path: &Path,
    on_frame: impl FnMut(DecodedFrame) -> AppResult<()>,
) -> AppResult<f64> {
    let source = open_plaintext(path)?;
    let fps = source.fps();
    source.decode(on_frame)?;
    Ok(fps)
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
                decode_track_vvdec(path, track, on_frame)
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
    if let Ok(raw) = std::env::var("HONGGUO_BYTEVC2_VVDEC_THREADS") {
        if let Ok(n) = raw.parse::<i32>() {
            // -1 = VVdeC auto；0/正数按库语义。
            return n;
        }
    }
    // 默认 capped，避免与 UI/播放器抢满核；自测可抬高。
    std::thread::available_parallelism()
        .map(|n| (n.get() as i32).clamp(1, 8))
        .unwrap_or(4)
}

#[cfg(feature = "vvdec")]
fn decode_track_vvdec(
    path: &Path,
    track: &Bytevc2Track,
    on_frame: &mut impl FnMut(DecodedFrame) -> AppResult<()>,
) -> AppResult<()> {
    use vvdec::{AccessUnit, Decoder, Error as VvError};

    let length_size = crate::length_size_from_extradata(&track.config)?;
    let params = annexb_from_extradata(&track.config)?;
    let timeline = track.info.sample_pts();
    let stamp_pts = timeline.len() == track.info.samples.len() && !timeline.is_empty();
    let mut dec = Decoder::builder()
        .num_threads(vvdec_thread_count())
        .build()
        .map_err(map_vvdec_err)?;
    let mut sent_params = false;

    for (index, &(offset, size)) in track.info.samples.iter().enumerate() {
        let raw = container::read_sample(path, offset, size)?;
        let sample = sample_to_annexb(&raw, length_size);
        if sample.is_empty() {
            continue;
        }
        let first_au = !sent_params;
        let annexb = if sent_params {
            sample
        } else {
            sent_params = true;
            let mut with_params = params.clone();
            with_params.extend_from_slice(&sample);
            with_params
        };
        let mut au = AccessUnit::new(annexb);
        // 首包（参数集+样本）标 RAP，便于随机接入。
        if first_au {
            au.is_random_access_point = true;
        }
        if stamp_pts {
            let ticks = presentation_tick(timeline[index]);
            if ticks >= 0 {
                au.cts = Some(ticks as u64);
            }
        }
        match dec.decode(au) {
            Ok(Some(frame)) => on_frame(pack_vvdec_frame(&frame)?)?,
            Ok(None) => {}
            Err(VvError::TryAgain) => {}
            Err(e) => return Err(map_vvdec_err(e)),
        }
    }
    if !sent_params {
        return Err(AppError::Media("ByteVC2 没有可解码的样本".into()));
    }
    loop {
        match dec.flush() {
            Ok(Some(frame)) => on_frame(pack_vvdec_frame(&frame)?)?,
            Ok(None) | Err(VvError::Eof) => break,
            Err(VvError::RestartRequired) => break,
            Err(e) => return Err(map_vvdec_err(e)),
        }
    }
    Ok(())
}

#[cfg(feature = "vvdec")]
fn pack_vvdec_frame(frame: &vvdec::Frame) -> AppResult<DecodedFrame> {
    use vvdec::{ColorFormat, PlaneComponent};

    let width = (frame.width() as usize) & !1;
    let height = (frame.height() as usize) & !1;
    if width == 0 || height == 0 {
        return Err(AppError::Media("ByteVC2 解码分辨率无效".into()));
    }
    let bit_depth = frame.bit_depth();
    let shift = match bit_depth {
        0 | 8 => 0u8,
        10 => 2,
        12 => 4,
        other => {
            return Err(AppError::Media(format!(
                "ByteVC2 位深暂不支持: {other}"
            )));
        }
    };
    let with_chroma = match frame.color_format() {
        ColorFormat::Yuv420Planar => true,
        ColorFormat::Yuv400Planar => false,
        other => {
            return Err(AppError::Media(format!(
                "ByteVC2 像素格式暂不支持: {other:?}"
            )));
        }
    };
    let y_plane = frame
        .plane(PlaneComponent::Y)
        .ok_or_else(|| AppError::Media("ByteVC2 帧缺 Y 平面".into()))?;
    let y = plane8_raw(
        y_plane.as_ref(),
        y_plane.stride() as usize,
        width,
        height,
        y_plane.bytes_per_sample() as usize,
        shift,
    )?;
    let cw = width / 2;
    let ch = height / 2;
    let (u, v) = if with_chroma {
        let u_plane = frame
            .plane(PlaneComponent::U)
            .ok_or_else(|| AppError::Media("ByteVC2 帧缺 U 平面".into()))?;
        let v_plane = frame
            .plane(PlaneComponent::V)
            .ok_or_else(|| AppError::Media("ByteVC2 帧缺 V 平面".into()))?;
        (
            plane8_raw(
                u_plane.as_ref(),
                u_plane.stride() as usize,
                cw,
                ch,
                u_plane.bytes_per_sample() as usize,
                shift,
            )?,
            plane8_raw(
                v_plane.as_ref(),
                v_plane.stride() as usize,
                cw,
                ch,
                v_plane.bytes_per_sample() as usize,
                shift,
            )?,
        )
    } else {
        (vec![128u8; cw * ch], vec![128u8; cw * ch])
    };
    let pts = frame
        .cts()
        .map(|ticks| ticks as f64 / PRESENTATION_TIMESCALE as f64);
    Ok(DecodedFrame {
        width,
        height,
        y,
        u,
        v,
        pts,
    })
}

#[cfg(feature = "vvdec")]
fn map_vvdec_err(err: vvdec::Error) -> AppError {
    AppError::Media(format!("ByteVC2 vvdec 解码失败: {err}"))
}

fn plane8_raw(
    data: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    bytes_per_sample: usize,
    shift: u8,
) -> AppResult<Vec<u8>> {
    let bps = if shift == 0 {
        1usize
    } else {
        bytes_per_sample.max(2)
    };
    let need = stride.saturating_mul(height.saturating_sub(1)) + width * bps;
    if data.len() < need || stride < width * bps {
        return Err(AppError::Media("ByteVC2 帧平面越界".into()));
    }
    let mut out = Vec::with_capacity(width * height);
    for row in 0..height {
        let start = row * stride;
        if shift == 0 {
            out.extend_from_slice(&data[start..start + width]);
            continue;
        }
        for col in 0..width {
            let i = start + col * 2;
            let sample = u16::from_le_bytes([data[i], data[i + 1]]);
            out.push((sample >> shift).min(255) as u8);
        }
    }
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
    let mut dec =
        H266Decoder::new(&CodecParameters::video(CodecId::new(CODEC_ID_STR))).map_err(map_oxide_err)?;
    let mut sent_params = false;
    for (index, &(offset, size)) in track.info.samples.iter().enumerate() {
        let raw = container::read_sample(path, offset, size)?;
        let sample = sample_to_annexb(&raw, length_size);
        if sample.is_empty() {
            continue;
        }
        let annexb = if sent_params {
            sample
        } else {
            sent_params = true;
            let mut with_params = params.clone();
            with_params.extend_from_slice(&sample);
            with_params
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    const COMPOSITION_OFFSET_SECS: f64 = 0.2;
    const MID_TOLERANCE_SECS: f64 = 0.002;

    #[test]
    fn official_plaintext_emits_a_frame() {
        let path = Path::new("/tmp/hg-bytevc2-plain.mp4");
        if !path.is_file() {
            return;
        }
        let source = open_plaintext(path).expect("打开官方明文");
        let mut frames = 0usize;
        let mut dims = (0usize, 0usize);
        let stopped = source.decode(|frame| {
            dims = (frame.width, frame.height);
            frames += 1;
            if frames >= 8 {
                return Err(AppError::Media("stop".into()));
            }
            Ok(())
        });
        assert_eq!(frames, 8, "应解出 8 帧再停: {stopped:?} dims={dims:?}");
        assert!(dims.0 >= 2 && dims.1 >= 2, "实际 {dims:?}");
    }

    #[test]
    fn official_plaintext_frames_follow_source_presentation_time() {
        let path = Path::new("/tmp/hg-bytevc2-plain.mp4");
        if !path.is_file() {
            return;
        }
        let source = open_plaintext(path).expect("打开官方明文");
        let mut pts = Vec::new();
        let _ = source.decode(|frame| {
            pts.push(frame.pts.expect("官方样本表应带出显示时间"));
            if pts.len() >= 8 {
                return Err(AppError::Media("stop".into()));
            }
            Ok(())
        });
        assert_eq!(pts.len(), 8, "实际 {pts:?}");
        let track = crate::container::open_track(path).expect("打开轨");
        let got: Vec<i64> = pts.iter().copied().map(presentation_tick).collect();
        if active_decoder_name() == "vvdec" {
            // vvdec 按显示序出帧：前 8 回调 = 样本表按显示时间排序后的前 8。
            let mut expect: Vec<i64> = track
                .info
                .sample_pts()
                .into_iter()
                .map(presentation_tick)
                .collect();
            expect.sort_unstable();
            expect.truncate(8);
            assert_eq!(got, expect, "显示序前 8 帧 PTS 应对齐样本表");
        } else {
            // oxideav 回退路径仍按解出顺序带回对应包 PTS。
            let mut expect: Vec<i64> = track
                .info
                .sample_pts()
                .into_iter()
                .take(8)
                .map(presentation_tick)
                .collect();
            let mut got_sorted = got.clone();
            expect.sort_unstable();
            got_sorted.sort_unstable();
            assert_eq!(got_sorted, expect, "回调时间应是这 8 个源样本的显示时间");
        }
    }

    #[test]
    fn official_plaintext_decode_midpoint_is_the_composition_offset() {
        let path = Path::new("/tmp/hg-bytevc2-plain.mp4");
        if !path.is_file() {
            return;
        }
        let track = crate::container::open_track(path).expect("打开轨");
        let pts = track.info.sample_pts();
        let mid = pts.len() / 2;
        let mut present = pts.clone();
        present.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let fps = track.info.average_framerate().expect("帧率");
        let composition = pts[mid] - present[mid];
        assert!(
            (composition - COMPOSITION_OFFSET_SECS).abs() < MID_TOLERANCE_SECS,
            "解码序 {} 显示序 {} 差 {composition}",
            pts[mid],
            present[mid]
        );
        assert!(
            (present[mid] - mid as f64 / fps).abs() < MID_TOLERANCE_SECS,
            "显示序中点 {} 帧率 {fps}",
            present[mid]
        );
    }

    #[test]
    fn missing_file_is_an_io_error() {
        let err = decode_plaintext_mp4(Path::new("/no/such/bytevc2.mp4"), |_| Ok(())).unwrap_err();
        assert!(matches!(err, AppError::Io(_)), "实际 {err}");
    }

    #[cfg(feature = "oxideav")]
    #[test]
    fn flat_grey_idr_decodes_to_i420() {
        use oxideav_core::{CodecId, CodecParameters, Decoder, Error as OxideError, Frame, Packet, TimeBase};
        use oxideav_h266::CODEC_ID_STR;
        use oxideav_h266::decoder::H266Decoder;
        use oxideav_h266::encoder_pipeline::encode_idr_with_residuals;
        use oxideav_h266::reconstruct::PictureBuffer;

        let src = PictureBuffer::yuv420_filled(64, 64, 128);
        let (bitstream, _) = encode_idr_with_residuals(&src, 26).expect("应产出 Annex-B");
        let params = CodecParameters::video(CodecId::new(CODEC_ID_STR));
        let mut dec = H266Decoder::new(&params).expect("解码器");
        dec.send_packet(&Packet::new(0, TimeBase::new(1, 25), bitstream))
            .expect("送入码流");
        dec.flush().expect("冲刷");
        let mut frames = 0usize;
        loop {
            match dec.receive_frame() {
                Ok(Frame::Video(frame)) => {
                    let packed = pack_oxide_frame(&dec, &frame).expect("打包");
                    assert_eq!((packed.width, packed.height), (64, 64));
                    assert_eq!(packed.y.len(), 64 * 64);
                    assert_eq!(packed.u.len(), 32 * 32);
                    let mid = packed
                        .y
                        .iter()
                        .filter(|&&s| (100..156).contains(&s))
                        .count();
                    assert!(mid > packed.y.len() / 2, "灰帧亮度应落在中灰附近");
                    frames += 1;
                }
                Ok(_) => {}
                Err(OxideError::NeedMore | OxideError::Eof) => break,
                Err(e) => panic!("解码失败: {e}"),
            }
        }
        assert_eq!(frames, 1, "一张 IDR 应解出一帧");
    }

    #[test]
    fn default_backend_is_vvdec_when_enabled() {
        // 不强制清环境：只断言编译进 vvdec 时名字可选。
        let name = active_decoder_name();
        assert!(
            name == "vvdec" || name == "oxideav",
            "unexpected decoder {name}"
        );
        #[cfg(feature = "vvdec")]
        {
            if std::env::var_os("HONGGUO_BYTEVC2_DECODER").is_none() {
                assert_eq!(name, "vvdec");
            }
        }
    }
}
