use std::{
    collections::HashMap,
    ffi::CStr,
    ptr::{self, NonNull},
};

use hongguo_common::{AppError, AppResult};
use hongguo_media_types::{ChromaSampling, CropRect, NativeFrame, NativePlane};
use hongguo_vvdec_sys as ffi;

#[path = "vvdec_metadata.rs"]
mod metadata;

pub struct VvdecDecoder {
    raw: NonNull<ffi::vvdecDecoder>,
    timestamps: HashMap<u64, f64>,
    next_token: u64,
}

impl VvdecDecoder {
    pub fn new(threads: i32) -> AppResult<Self> {
        if threads < -1 {
            return Err(AppError::Media("invalid VVdeC thread count".into()));
        }
        let mut params = std::mem::MaybeUninit::<ffi::vvdecParams>::uninit();
        // The C initializer fills the complete native struct, including reserved fields.
        let raw = unsafe {
            ffi::vvdec_params_default(params.as_mut_ptr());
            let mut params = params.assume_init();
            params.threads = threads;
            params.logLevel = ffi::vvdecLogLevel_VVDEC_SILENT;
            ffi::vvdec_decoder_open(&mut params)
        };
        let raw = NonNull::new(raw)
            .ok_or_else(|| AppError::Media("VVdeC initialization failed".into()))?;
        Ok(Self {
            raw,
            timestamps: HashMap::new(),
            next_token: 1,
        })
    }

    pub fn version() -> &'static str {
        // VVdeC owns a process-lifetime, NUL-terminated version string.
        unsafe { CStr::from_ptr(ffi::vvdec_get_version()) }
            .to_str()
            .unwrap_or("invalid VVdeC version")
    }

    pub fn decode(
        &mut self,
        data: &[u8],
        pts: Option<f64>,
        random_access: bool,
    ) -> AppResult<Option<NativeFrame>> {
        if data.is_empty() || pts.is_some_and(|pts| !pts.is_finite()) {
            return Err(AppError::Media("invalid VVdeC access unit".into()));
        }
        let size = i32::try_from(data.len())
            .map_err(|_| AppError::Media("VVdeC access unit exceeds i32".into()))?;
        let mut packet = std::mem::MaybeUninit::<ffi::vvdecAccessUnit>::uninit();
        unsafe {
            ffi::vvdec_accessUnit_default(packet.as_mut_ptr());
        }
        let mut packet = unsafe { packet.assume_init() };
        packet.payload = data.as_ptr().cast_mut();
        packet.payloadSize = size;
        packet.payloadUsedSize = size;
        packet.rap = random_access;
        if let Some(pts) = pts {
            if self.timestamps.len() >= 128 {
                return Err(AppError::Media(
                    "VVdeC pending timestamp budget exhausted".into(),
                ));
            }
            let token = self.next_token;
            self.next_token = token
                .checked_add(1)
                .ok_or_else(|| AppError::Media("VVdeC timestamp token exhausted".into()))?;
            self.timestamps.insert(token, pts);
            packet.cts = token;
            packet.ctsValid = true;
        }
        let mut frame = ptr::null_mut();
        // VVdeC copies AU bytes during the call and does not retain packet.payload.
        let result = unsafe { ffi::vvdec_decode(self.raw.as_ptr(), &mut packet, &mut frame) };
        self.output(result, frame)
    }

    pub fn flush(&mut self) -> AppResult<Option<NativeFrame>> {
        let mut frame = ptr::null_mut();
        let result = unsafe { ffi::vvdec_flush(self.raw.as_ptr(), &mut frame) };
        self.output(result, frame)
    }

    fn output(
        &mut self,
        result: i32,
        frame: *mut ffi::vvdecFrame,
    ) -> AppResult<Option<NativeFrame>> {
        if !matches!(
            result,
            ffi::vvdecErrorCodes_VVDEC_OK
                | ffi::vvdecErrorCodes_VVDEC_TRY_AGAIN
                | ffi::vvdecErrorCodes_VVDEC_EOF
        ) {
            if !frame.is_null() {
                // Native errors may still return an owned frame. Release it,
                // without inspecting potentially incomplete frame attributes.
                unsafe {
                    ffi::vvdec_frame_unref(self.raw.as_ptr(), frame);
                }
            }
            let message =
                unsafe { CStr::from_ptr(ffi::vvdec_get_error_msg(result)) }.to_string_lossy();
            let detail =
                unsafe { CStr::from_ptr(ffi::vvdec_get_last_additional_error(self.raw.as_ptr())) }
                    .to_string_lossy();
            return Err(AppError::Media(format!(
                "ByteVC2 VVdeC failed ({result}): {message}: {detail}"
            )));
        }
        NonNull::new(frame)
            .map(|frame| {
                let decoded = self.copy_frame(frame);
                // Returned frames are released exactly once, including copy errors.
                unsafe {
                    ffi::vvdec_frame_unref(self.raw.as_ptr(), frame.as_ptr());
                }
                decoded
            })
            .transpose()
    }

    fn copy_frame(&mut self, frame: NonNull<ffi::vvdecFrame>) -> AppResult<NativeFrame> {
        let raw = unsafe { frame.as_ref() };
        let pts = if raw.ctsValid {
            self.timestamps.remove(&raw.cts)
        } else {
            None
        };
        let color = unsafe { metadata::read(self.raw.as_ptr(), frame.as_ptr()) };
        copy_pixels(raw, pts, color)
    }
}

impl Drop for VvdecDecoder {
    fn drop(&mut self) {
        unsafe {
            ffi::vvdec_decoder_close(self.raw.as_ptr());
        }
    }
}

fn copy_pixels(
    raw: &ffi::vvdecFrame,
    pts: Option<f64>,
    color: hongguo_media_types::ColorMetadata,
) -> AppResult<NativeFrame> {
    if raw.frameFormat != ffi::vvdecFrameFormat_VVDEC_FF_PROGRESSIVE {
        return Err(AppError::Media(
            "interlaced VVdeC output is unsupported".into(),
        ));
    }
    let chroma = match raw.colorFormat {
        ffi::vvdecColorFormat_VVDEC_CF_YUV400_PLANAR => ChromaSampling::Monochrome,
        ffi::vvdecColorFormat_VVDEC_CF_YUV420_PLANAR => ChromaSampling::Yuv420,
        ffi::vvdecColorFormat_VVDEC_CF_YUV422_PLANAR => ChromaSampling::Yuv422,
        ffi::vvdecColorFormat_VVDEC_CF_YUV444_PLANAR => ChromaSampling::Yuv444,
        other => {
            return Err(AppError::Media(format!(
                "unsupported VVdeC color format: {other}"
            )));
        }
    };
    let expected_planes = if chroma == ChromaSampling::Monochrome {
        1
    } else {
        3
    };
    if !matches!(raw.bitDepth, 8 | 10)
        || raw.width == 0
        || raw.height == 0
        || raw.numPlanes != expected_planes
    {
        return Err(AppError::Media("invalid VVdeC frame layout".into()));
    }
    let planes = raw.planes[..expected_planes as usize]
        .iter()
        .map(copy_plane)
        .collect::<AppResult<_>>()?;
    Ok(NativeFrame {
        width: raw.width as usize,
        height: raw.height as usize,
        bit_depth: raw.bitDepth as u8,
        chroma,
        planes,
        pts,
        color,
        crop: CropRect {
            x: 0,
            y: 0,
            width: raw.width as usize,
            height: raw.height as usize,
        },
        rotation_degrees: 0,
    })
}

fn copy_plane(plane: &ffi::vvdecPlane) -> AppResult<NativePlane> {
    let (width, height, stride, bps) = (
        plane.width as usize,
        plane.height as usize,
        plane.stride as usize,
        plane.bytesPerSample as usize,
    );
    let row_bytes = width
        .checked_mul(bps)
        .ok_or_else(|| AppError::Media("VVdeC plane overflow".into()))?;
    let length = stride
        .checked_mul(height)
        .filter(|&bytes| bytes <= isize::MAX as usize)
        .ok_or_else(|| AppError::Media("VVdeC plane overflow".into()))?;
    if plane.ptr.is_null()
        || width == 0
        || height == 0
        || !matches!(bps, 1 | 2)
        || row_bytes > stride
    {
        return Err(AppError::Media("invalid VVdeC plane".into()));
    }
    // A cropped native pointer may start inside a larger row. Only visible
    // bytes are guaranteed readable in the final row; keep upload padding owned
    // and zeroed instead of reading native trailing padding past that row.
    let mut bytes = Vec::with_capacity(length);
    for row in 0..height {
        let offset = row * stride;
        // SAFETY: The native frame lease stays live; validated stride and native
        // plane dimensions guarantee each visible row, without assuming padding.
        let visible = unsafe { std::slice::from_raw_parts(plane.ptr.add(offset), row_bytes) };
        bytes.extend_from_slice(visible);
        bytes.resize(offset + stride, 0);
    }
    Ok(NativePlane {
        data: bytes.into(),
        stride,
        width,
        height,
        bytes_per_sample: bps as u8,
    })
}

#[cfg(test)]
#[path = "vvdec_tests.rs"]
mod tests;
