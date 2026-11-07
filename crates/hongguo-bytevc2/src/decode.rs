//! ByteVC2 解帧。librempeg `ff_bvc2_decoder` 与 VVC 同核，差别只在
//! `cbs_bvc2_split_fragment` 把 MP4 样本固定按长度前缀拆开；这里先转 Annex-B，
//! 再交给纯 Rust H.266 解码器。运行时不调用 ffmpeg / librempeg。

use std::path::{Path, PathBuf};

use hongguo_common::{AppError, AppResult};
use oxideav_core::{
    CodecId, CodecParameters, Decoder, Error as OxideError, Frame, Packet, PixelFormat, TimeBase,
};
use oxideav_h266::CODEC_ID_STR;
use oxideav_h266::decoder::H266Decoder;

use crate::container::{self, Bytevc2Track};
use crate::extradata::annexb_from_extradata;
use crate::sample::sample_to_annexb;

/// 一帧 8-bit 4:2:0。10/12-bit 源在打包时右移到 8-bit，供平台 H.264 编码器使用。
pub struct DecodedFrame {
    pub width: usize,
    pub height: usize,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
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

fn decode_track(
    path: &Path,
    track: &Bytevc2Track,
    on_frame: &mut impl FnMut(DecodedFrame) -> AppResult<()>,
) -> AppResult<()> {
    let length_size = crate::length_size_from_extradata(&track.config)?;
    let params = annexb_from_extradata(&track.config)?;
    let mut dec =
        H266Decoder::new(&CodecParameters::video(CodecId::new(CODEC_ID_STR))).map_err(map_err)?;
    let mut sent_params = false;
    for &(offset, size) in &track.info.samples {
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
        let packet = Packet::new(0, TimeBase::new(1, 90_000), annexb);
        dec.send_packet(&packet).map_err(map_err)?;
        drain(&mut dec, on_frame)?;
    }
    if !sent_params {
        return Err(AppError::Media("ByteVC2 没有可解码的样本".into()));
    }
    dec.flush().map_err(map_err)?;
    drain(&mut dec, on_frame)?;
    Ok(())
}

fn drain(
    dec: &mut H266Decoder,
    on_frame: &mut impl FnMut(DecodedFrame) -> AppResult<()>,
) -> AppResult<()> {
    loop {
        match dec.receive_frame() {
            Ok(Frame::Video(frame)) => on_frame(pack_frame(dec, &frame)?)?,
            Ok(_) => {}
            Err(OxideError::NeedMore | OxideError::Eof) => return Ok(()),
            Err(e) => return Err(map_err(e)),
        }
    }
}

fn pack_frame(dec: &H266Decoder, frame: &oxideav_core::VideoFrame) -> AppResult<DecodedFrame> {
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
    let y = plane8(frame.planes.first(), width, height, shift)?;
    let cw = width / 2;
    let ch = height / 2;
    let (u, v) = if with_chroma {
        (
            plane8(frame.planes.get(1), cw, ch, shift)?,
            plane8(frame.planes.get(2), cw, ch, shift)?,
        )
    } else {
        (vec![128u8; cw * ch], vec![128u8; cw * ch])
    };
    Ok(DecodedFrame {
        width,
        height,
        y,
        u,
        v,
    })
}

fn plane8(
    plane: Option<&oxideav_core::VideoPlane>,
    width: usize,
    height: usize,
    shift: u8,
) -> AppResult<Vec<u8>> {
    let plane = plane.ok_or_else(|| AppError::Media("ByteVC2 帧缺平面".into()))?;
    let bps = if shift == 0 { 1 } else { 2 };
    let need = plane.stride.saturating_mul(height.saturating_sub(1)) + width * bps;
    if plane.data.len() < need || plane.stride < width * bps {
        return Err(AppError::Media("ByteVC2 帧平面越界".into()));
    }
    let mut out = Vec::with_capacity(width * height);
    for row in 0..height {
        let start = row * plane.stride;
        if shift == 0 {
            out.extend_from_slice(&plane.data[start..start + width]);
            continue;
        }
        for col in 0..width {
            let i = start + col * 2;
            let sample = u16::from_le_bytes([plane.data[i], plane.data[i + 1]]);
            out.push((sample >> shift).min(255) as u8);
        }
    }
    Ok(out)
}

fn map_err(err: OxideError) -> AppError {
    AppError::Media(format!("ByteVC2 解码失败: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_an_io_error() {
        let err = decode_plaintext_mp4(Path::new("/no/such/bytevc2.mp4"), |_| Ok(())).unwrap_err();
        assert!(matches!(err, AppError::Io(_)), "实际 {err}");
    }

    #[test]
    fn flat_grey_idr_decodes_to_i420() {
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
                    let packed = pack_frame(&dec, &frame).expect("打包");
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
}
