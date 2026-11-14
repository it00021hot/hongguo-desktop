use std::{
    ffi::c_void,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use hongguo_media_types::{
    ChromaSampling, ColorMetadata, ContentLightMetadata, CropRect, FrameDescriptor, FrameImage,
    HardwareFrame, HardwareHandle, HardwareKind, MasteringDisplayMetadata, NativeFrame,
    NativePlane,
};

use crate::{
    CodecError, CodecResult,
    error::{check, packet_size},
    ffi,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Codec {
    Hevc,
    H264,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Backend {
    Software,
    VideoToolbox,
    D3d11 { device: usize },
    D3d12 { device: usize },
    MediaCodec { java_vm: usize, surface: usize },
}

#[derive(Clone, Debug)]
pub struct DecoderConfig {
    pub codec: Codec,
    pub backend: Backend,
    pub extradata: Vec<u8>,
    pub time_base: (i32, i32),
    pub threads: u32,
}

pub struct Packet<'a> {
    pub data: &'a [u8],
    pub pts: Option<i64>,
    pub dts: Option<i64>,
    pub duration: i64,
}

pub struct VideoDecoder {
    pointer: *mut c_void,
    config: DecoderConfig,
}

// A context has one worker owner; immutable AVFrame references own their buffers.
unsafe impl Send for VideoDecoder {}

impl VideoDecoder {
    /// # Safety
    /// D3D11 and JNI handles must be live native objects of the declared kind during creation.
    pub unsafe fn new(config: DecoderConfig) -> CodecResult<Self> {
        let (backend, device, surface) = match config.backend {
            Backend::Software => (0, 0, 0),
            Backend::VideoToolbox => (1, 0, 0),
            Backend::D3d11 { device } => (2, device, 0),
            Backend::D3d12 { device } => (4, device, 0),
            Backend::MediaCodec { java_vm, surface } => (3, java_vm, surface),
        };
        let mut pointer = ptr::null_mut();
        let codec = match config.codec {
            Codec::Hevc => 0,
            Codec::H264 => 1,
        };
        let size = packet_size(config.extradata.len())?;
        let threads = i32::try_from(config.threads)
            .map_err(|_| CodecError::Invalid("invalid thread count"))?;
        check(
            unsafe {
                ffi::hg_video_open(
                    &mut pointer,
                    codec,
                    backend,
                    device,
                    surface,
                    config.extradata.as_ptr(),
                    size,
                    config.time_base.0,
                    config.time_base.1,
                    threads,
                )
            },
            "open video decoder",
        )?;
        Ok(Self { pointer, config })
    }

    pub fn software(codec: Codec, extradata: Vec<u8>, time_base: (i32, i32)) -> CodecResult<Self> {
        unsafe {
            Self::new(DecoderConfig {
                codec,
                backend: Backend::Software,
                extradata,
                time_base,
                threads: 0,
            })
        }
    }

    pub fn backend(&self) -> Backend {
        self.config.backend
    }

    pub fn send(&mut self, packet: &Packet<'_>) -> CodecResult<bool> {
        let size = packet_size(packet.data.len())?;
        check(
            unsafe {
                ffi::hg_video_send(
                    self.pointer,
                    packet.data.as_ptr(),
                    size,
                    packet.pts.unwrap_or(ffi::NO_TIMESTAMP),
                    packet.dts.unwrap_or(ffi::NO_TIMESTAMP),
                    packet.duration,
                )
            },
            "send video packet",
        )
        .map(|status| status == 1)
    }

    pub fn drain(&mut self) -> CodecResult<bool> {
        check(
            unsafe {
                ffi::hg_video_send(
                    self.pointer,
                    ptr::null(),
                    0,
                    ffi::NO_TIMESTAMP,
                    ffi::NO_TIMESTAMP,
                    0,
                )
            },
            "drain video decoder",
        )
        .map(|status| status == 1)
    }

    pub fn receive(&mut self) -> CodecResult<Option<VideoFrame>> {
        let mut pointer = ptr::null_mut();
        let status = check(
            unsafe { ffi::hg_video_receive(self.pointer, &mut pointer) },
            "receive video frame",
        )?;
        if status == 0 {
            return Ok(None);
        }
        VideoFrame::from_pointer(pointer, self.config.time_base, self.config.backend).map(Some)
    }

    pub fn flush(&mut self) {
        unsafe { ffi::hg_video_flush(self.pointer) }
    }
}

impl Drop for VideoDecoder {
    fn drop(&mut self) {
        unsafe { ffi::hg_video_free(&mut self.pointer) }
    }
}

pub struct VideoFrame {
    pointer: *mut c_void,
    info: ffi::FrameInfo,
    time_base: (i32, i32),
    backend: Backend,
    rendered: AtomicBool,
}

// Published frames are immutable and retain AVBufferRef/native surface ownership.
unsafe impl Send for VideoFrame {}
unsafe impl Sync for VideoFrame {}

impl VideoFrame {
    fn from_pointer(
        mut pointer: *mut c_void,
        time_base: (i32, i32),
        backend: Backend,
    ) -> CodecResult<Self> {
        let mut info = ffi::FrameInfo::default();
        if let Err(error) = check(
            unsafe { ffi::hg_frame_info(pointer, &mut info) },
            "inspect video frame",
        ) {
            unsafe { ffi::hg_frame_free(&mut pointer) };
            return Err(error);
        }
        Ok(Self {
            pointer,
            info,
            time_base,
            backend,
            rendered: AtomicBool::new(false),
        })
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.info.width as u32, self.info.height as u32)
    }
    pub fn bit_depth(&self) -> u8 {
        self.info.bit_depth as u8
    }
    pub fn pts(&self) -> Option<f64> {
        (self.info.pts != ffi::NO_TIMESTAMP).then(|| {
            self.info.pts as f64 * f64::from(self.time_base.0) / f64::from(self.time_base.1)
        })
    }
    pub fn duration(&self) -> f64 {
        self.info.duration as f64 * f64::from(self.time_base.0) / f64::from(self.time_base.1)
    }
    pub fn into_image(self) -> CodecResult<FrameImage> {
        if matches!(self.backend, Backend::D3d12 { .. }) {
            return Err(CodecError::Invalid(
                "D3D12 frames require the development audit presenter",
            ));
        }
        if self.info.backend == 0 {
            return self.cpu().map(|frame| FrameImage::Cpu(Arc::new(frame)));
        }
        if self.info.handle == 0 {
            return Err(CodecError::Invalid(
                "native decoder returned a null surface",
            ));
        }
        Ok(FrameImage::Hardware(Arc::new(HardwareVideoFrame(self))))
    }
    pub fn native_handle(&self) -> Option<(Backend, usize, usize)> {
        (self.info.backend != 0).then_some((self.backend, self.info.handle, self.info.subresource))
    }
    pub fn d3d12_handle(&self) -> CodecResult<Option<crate::D3d12FrameHandle>> {
        if !matches!(self.backend, Backend::D3d12 { .. }) {
            return Ok(None);
        }
        let mut handle = ffi::D3d12Frame::default();
        check(
            unsafe { ffi::hg_frame_d3d12(self.pointer, &mut handle) },
            "inspect D3D12 frame",
        )?;
        Ok(Some(handle))
    }

    pub fn descriptor(&self) -> FrameDescriptor {
        let info = &self.info;
        FrameDescriptor {
            width: info.width as usize,
            height: info.height as usize,
            bit_depth: self.bit_depth(),
            crop: CropRect {
                x: info.crop_left as usize,
                y: info.crop_top as usize,
                width: (info.width - info.crop_left - info.crop_right) as usize,
                height: (info.height - info.crop_top - info.crop_bottom) as usize,
            },
            rotation_degrees: 0,
            color: self.color(),
        }
    }

    pub fn cpu(&self) -> CodecResult<NativeFrame> {
        if self.info.backend != 0 {
            let mut pointer = ptr::null_mut();
            check(
                unsafe { ffi::hg_frame_download(self.pointer, &mut pointer) },
                "download video frame",
            )?;
            return Self::from_pointer(pointer, self.time_base, Backend::Software)?.cpu();
        }
        let chroma = match (self.info.planes, self.info.chroma_w, self.info.chroma_h) {
            (1, _, _) => ChromaSampling::Monochrome,
            (3, 1, 1) => ChromaSampling::Yuv420,
            (3, 1, 0) => ChromaSampling::Yuv422,
            (3, 0, 0) => ChromaSampling::Yuv444,
            _ => return Err(CodecError::Invalid("unsupported native chroma layout")),
        };
        let planes = (0..self.info.planes as usize)
            .map(|plane| self.copy_plane(plane))
            .collect::<CodecResult<Vec<_>>>()?;
        let crop = CropRect {
            x: self.info.crop_left as usize,
            y: self.info.crop_top as usize,
            width: (self.info.width - self.info.crop_left - self.info.crop_right) as usize,
            height: (self.info.height - self.info.crop_top - self.info.crop_bottom) as usize,
        };
        Ok(NativeFrame {
            width: self.info.width as usize,
            height: self.info.height as usize,
            bit_depth: self.bit_depth(),
            chroma,
            planes,
            crop,
            rotation_degrees: 0,
            pts: self.pts(),
            color: self.color(),
        })
    }

    fn copy_plane(&self, plane: usize) -> CodecResult<NativePlane> {
        let width = usize::try_from(self.info.plane_width[plane])
            .map_err(|_| CodecError::Invalid("invalid plane width"))?;
        let height = usize::try_from(self.info.plane_height[plane])
            .map_err(|_| CodecError::Invalid("invalid plane height"))?;
        let bytes = self.info.bytes_per_sample as usize;
        let stride = width
            .checked_mul(bytes)
            .ok_or(CodecError::Invalid("plane stride overflow"))?;
        let size = stride
            .checked_mul(height)
            .ok_or(CodecError::Invalid("plane size overflow"))?;
        let mut data = vec![0_u8; size];
        for y in 0..height {
            let row = unsafe {
                self.info.data[plane].offset(y as isize * self.info.stride[plane] as isize)
            };
            for x in 0..width {
                let source = unsafe { row.add(x * self.info.sample_step[plane] as usize) };
                let sample = if bytes == 1 {
                    (unsafe { *source }) as u16
                } else {
                    let pair = unsafe { [*source, *source.add(1)] };
                    if self.info.big_endian != 0 {
                        u16::from_be_bytes(pair)
                    } else {
                        u16::from_le_bytes(pair)
                    }
                } >> self.info.sample_shift[plane];
                let offset = y * stride + x * bytes;
                if bytes == 1 {
                    data[offset] = sample as u8;
                } else {
                    data[offset..offset + bytes].copy_from_slice(&sample.to_le_bytes());
                }
            }
        }
        Ok(NativePlane {
            data: Arc::from(data),
            stride,
            width,
            height,
            bytes_per_sample: bytes as u8,
        })
    }

    pub fn color(&self) -> ColorMetadata {
        let value = |value: i32, unspecified| (value != unspecified).then_some(value as u8);
        ColorMetadata {
            primaries: value(self.info.primaries, 2),
            transfer: value(self.info.transfer, 2),
            matrix: value(self.info.matrix, 2),
            full_range: (self.info.full_range >= 0).then_some(self.info.full_range == 1),
            chroma_location: value(self.info.chroma_location, 0),
            sample_aspect_ratio: (self.info.sar_num > 0 && self.info.sar_den > 0)
                .then_some((self.info.sar_num as u32, self.info.sar_den as u32)),
            mastering_display: (self.info.has_mastering != 0).then_some(MasteringDisplayMetadata {
                primaries: [
                    self.info.mastering_primaries[0..2].try_into().unwrap(),
                    self.info.mastering_primaries[2..4].try_into().unwrap(),
                    self.info.mastering_primaries[4..6].try_into().unwrap(),
                ],
                white_point: self.info.white_point,
                max_luminance: self.info.max_luminance,
                min_luminance: self.info.min_luminance,
            }),
            content_light: (self.info.has_content_light != 0).then_some(ContentLightMetadata {
                max_content_light_level: self.info.max_cll,
                max_pic_average_light_level: self.info.max_fall,
            }),
        }
    }

    pub fn render_at(&mut self, monotonic_ns: i64) -> CodecResult<()> {
        check(
            unsafe { ffi::hg_frame_render(self.pointer, monotonic_ns) },
            "render MediaCodec surface",
        )
        .map(|_| ())
    }
}

struct HardwareVideoFrame(VideoFrame);

unsafe impl HardwareFrame for HardwareVideoFrame {
    fn handle(&self) -> HardwareHandle {
        let kind = match self.0.backend {
            Backend::VideoToolbox => HardwareKind::VideoToolbox,
            Backend::D3d11 { .. } => HardwareKind::D3d11,
            Backend::MediaCodec { .. } => HardwareKind::MediaCodec,
            Backend::Software | Backend::D3d12 { .. } => {
                unreachable!("production wrapper only accepts compatible native frames")
            }
        };
        HardwareHandle {
            kind,
            object: self.0.info.handle,
            subresource: self.0.info.subresource,
        }
    }

    fn descriptor(&self) -> FrameDescriptor {
        self.0.descriptor()
    }

    fn present_at(&self, monotonic_ns: i64) -> Result<(), String> {
        if self.0.rendered.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        check(
            unsafe { ffi::hg_frame_render(self.0.pointer, monotonic_ns) },
            "render native surface",
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    }
}

impl Drop for VideoFrame {
    fn drop(&mut self) {
        unsafe { ffi::hg_frame_free(&mut self.pointer) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mediacodec_matches_quantized_microseconds_without_losing_original_pts() {
        // 1/90,000,000 ticks is legal MP4 timing; its 30 fps timestamp does
        // not survive MediaCodec's mandatory microsecond conversion exactly.
        assert_eq!(
            unsafe { ffi::hg_mediacodec_timestamp_key(3_000_000, 1, 90_000_000) },
            2_999_970
        );
        assert_eq!(
            unsafe { ffi::hg_mediacodec_timestamp_key(-3_000_000, 1, 90_000_000) },
            -2_999_970
        );
        assert_eq!(
            unsafe { ffi::hg_mediacodec_timestamp_key(3_000, 1, 90_000) },
            3_000
        );
        assert_eq!(
            unsafe { ffi::hg_mediacodec_timestamp_key(ffi::NO_TIMESTAMP, 1, 90_000_000) },
            ffi::NO_TIMESTAMP
        );
    }
}
