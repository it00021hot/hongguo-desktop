//! HEVC software fallback for native Video; no audio or playback engine.

mod ffi;
mod picture;
pub mod syntax;

use hongguo_common::{AppError, AppResult};
use hongguo_media_types::NativeFrame;
use std::{ffi::CStr, ptr::NonNull};

const MAX_ACCESS_UNIT_BYTES: usize = 64 * 1024 * 1024;
const MAX_PUMP_STEPS: usize = 4096;
const MAX_OUTPUT_FRAMES: usize = 64;

pub struct Decoder {
    context: NonNull<ffi::Context>,
    timescale: u32,
    drained: bool,
}

impl Decoder {
    pub fn new(threads: u32, timescale: u32) -> AppResult<Self> {
        if timescale == 0 || threads == 0 || threads > 64 {
            return Err(AppError::Media(
                "invalid HEVC thread count or timescale".into(),
            ));
        }
        // Each context has one owner; the native kernel joins its workers at free.
        let context = NonNull::new(unsafe { ffi::de265_new_decoder() })
            .ok_or_else(|| AppError::Media("cannot allocate HEVC decoder".into()))?;
        let decoder = Self {
            context,
            timescale,
            drained: false,
        };
        if threads > 1 {
            check(unsafe { ffi::de265_start_worker_threads(context.as_ptr(), threads as i32) })?;
        }
        Ok(decoder)
    }

    pub fn decode(&mut self, annex_b: &[u8], pts: i64) -> AppResult<Vec<NativeFrame>> {
        if self.drained || annex_b.is_empty() || annex_b.len() > MAX_ACCESS_UNIT_BYTES {
            return Err(AppError::Media(
                "invalid HEVC access unit or decoder already drained".into(),
            ));
        }
        unsafe {
            check(ffi::de265_push_data(
                self.context.as_ptr(),
                annex_b.as_ptr().cast(),
                annex_b.len() as i32,
                pts,
                std::ptr::null_mut(),
            ))?;
            ffi::de265_push_end_of_NAL(self.context.as_ptr());
        }
        self.pump()
    }

    pub fn drain(&mut self) -> AppResult<Vec<NativeFrame>> {
        if self.drained {
            return Ok(Vec::new());
        }
        check(unsafe { ffi::de265_flush_data(self.context.as_ptr()) })?;
        self.drained = true;
        self.pump()
    }

    fn pump(&mut self) -> AppResult<Vec<NativeFrame>> {
        let mut frames = Vec::new();
        for _ in 0..MAX_PUMP_STEPS {
            let mut more = 0;
            let error = unsafe { ffi::de265_decode(self.context.as_ptr(), &mut more) };
            let before = frames.len();
            self.take_pictures(&mut frames)?;
            match error {
                ffi::WAITING_FOR_INPUT => return Ok(frames),
                ffi::IMAGE_BUFFER_FULL if frames.len() > before => continue,
                ffi::OK if more == 0 => return Ok(frames),
                ffi::OK => {}
                error => check(error)?,
            }
        }
        Err(AppError::Media(
            "HEVC decoder made no bounded progress".into(),
        ))
    }

    fn take_pictures(&mut self, frames: &mut Vec<NativeFrame>) -> AppResult<()> {
        loop {
            let Some(image) = NonNull::new(
                unsafe { ffi::de265_peek_next_picture(self.context.as_ptr()) }.cast_mut(),
            ) else {
                return Ok(());
            };
            let frame = unsafe { picture::copy(image.as_ptr(), self.timescale) };
            unsafe { ffi::de265_release_next_picture(self.context.as_ptr()) };
            if frames.len() >= MAX_OUTPUT_FRAMES {
                return Err(AppError::Media("HEVC output exceeds frame budget".into()));
            }
            frames.push(frame?);
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe { ffi::de265_free_decoder(self.context.as_ptr()) };
    }
}

fn check(error: u32) -> AppResult<()> {
    if error == ffi::OK {
        return Ok(());
    }
    let text = unsafe { ffi::de265_get_error_text(error) };
    let text = if text.is_null() {
        format!("code {error}")
    } else {
        unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned()
    };
    Err(AppError::Media(format!("HEVC decode failed: {text}")))
}
