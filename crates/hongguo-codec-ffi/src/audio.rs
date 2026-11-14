use std::{ffi::c_void, ptr};

use crate::{
    CodecResult,
    error::{check, packet_size},
    ffi,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Clone, Debug)]
pub struct PcmBlock {
    pub samples: Vec<f32>,
    pub format: AudioFormat,
    pub media_start_secs: Option<f64>,
    pub media_duration_secs: f64,
}

pub struct AudioDecoder {
    pointer: *mut c_void,
    format: AudioFormat,
}

// A decoder and its filter graph have exactly one worker owner.
unsafe impl Send for AudioDecoder {}

impl AudioDecoder {
    pub fn reset(&mut self, rate: f64) -> CodecResult<()> {
        check(
            unsafe { ffi::hg_audio_reset(self.pointer, rate) },
            "reset AAC PCM output",
        )
        .map(|_| ())
    }
    pub fn new(
        extradata: &[u8],
        input_rate: u32,
        format: AudioFormat,
        rate: f64,
    ) -> CodecResult<Self> {
        let size = packet_size(extradata.len())?;
        let mut pointer = ptr::null_mut();
        check(
            unsafe {
                ffi::hg_audio_open(
                    &mut pointer,
                    extradata.as_ptr(),
                    size,
                    input_rate as i32,
                    format.sample_rate as i32,
                    i32::from(format.channels),
                    rate,
                )
            },
            "open AAC PCM decoder",
        )?;
        Ok(Self { pointer, format })
    }

    pub fn send(&mut self, data: &[u8], pts: Option<i64>) -> CodecResult<bool> {
        let size = packet_size(data.len())?;
        let accepted = check(
            unsafe {
                ffi::hg_audio_send(
                    self.pointer,
                    data.as_ptr(),
                    size,
                    pts.unwrap_or(ffi::NO_TIMESTAMP),
                )
            },
            "send AAC packet",
        )? == 1;
        Ok(accepted)
    }

    pub fn drain(&mut self) -> CodecResult<bool> {
        check(
            unsafe { ffi::hg_audio_send(self.pointer, ptr::null(), 0, ffi::NO_TIMESTAMP) },
            "drain AAC decoder",
        )
        .map(|status| status == 1)
    }

    pub fn receive(&mut self) -> CodecResult<Option<PcmBlock>> {
        let mut info = ffi::PcmInfo::default();
        if check(
            unsafe { ffi::hg_audio_receive(self.pointer, &mut info) },
            "receive PCM",
        )? == 0
        {
            return Ok(None);
        }
        let media_start_secs = info
            .media_start_secs
            .is_finite()
            .then_some(info.media_start_secs);
        let samples = unsafe {
            std::slice::from_raw_parts(info.samples, info.frames as usize * info.channels as usize)
        }
        .to_vec();
        Ok(Some(PcmBlock {
            samples,
            format: self.format,
            media_start_secs,
            media_duration_secs: info.media_duration_secs,
        }))
    }
}

impl Drop for AudioDecoder {
    fn drop(&mut self) {
        unsafe { ffi::hg_audio_free(&mut self.pointer) }
    }
}
