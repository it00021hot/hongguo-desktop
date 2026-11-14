use std::{
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{AudioError, AudioOutput, PcmFormat, PcmSource, Position};

const START: i32 = 1;
const PAUSE: i32 = 2;
const FLUSH: i32 = 3;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Format {
    pub sample_rate: u32,
    pub channels: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Callbacks {
    pub context: *mut c_void,
    pub read: unsafe extern "C" fn(*mut c_void, *mut f32, u32, u32, u64) -> u32,
    pub played: unsafe extern "C" fn(*mut c_void, u64, u64),
}

#[repr(C)]
#[derive(Default)]
pub struct NativePosition {
    pub epoch: u64,
    pub device_frames: u64,
    pub latency_frames: u32,
    pub reserved: u32,
}

pub struct Api {
    pub open: unsafe extern "C" fn(Format, Callbacks, *mut *mut c_void) -> i32,
    pub control: unsafe extern "C" fn(*mut c_void, i32, u64) -> i32,
    pub position: unsafe extern "C" fn(*mut c_void, *mut NativePosition) -> i32,
    pub close: unsafe extern "C" fn(*mut c_void),
}

struct Source {
    source: Arc<dyn PcmSource>,
    failed: AtomicBool,
}

pub struct NativeOutput {
    device: NonNull<c_void>,
    source: Box<Source>,
    format: PcmFormat,
    api: &'static Api,
}

// SAFETY: Each backend serializes control/device access; callbacks hold a pinned Source.
unsafe impl Send for NativeOutput {}

impl NativeOutput {
    /// # Safety
    /// `api` must implement output.h and stop/join all callbacks before `close` returns.
    pub unsafe fn open(
        format: PcmFormat,
        source: Arc<dyn PcmSource>,
        api: &'static Api,
    ) -> Result<Self, AudioError> {
        let format = format.validate()?;
        let mut source = Box::new(Source {
            source,
            failed: AtomicBool::new(false),
        });
        let callbacks = Callbacks {
            context: std::ptr::from_mut(source.as_mut()).cast(),
            read,
            played,
        };
        let mut device = std::ptr::null_mut();
        // SAFETY: The caller provides this ABI; Source remains pinned through close.
        check("open", unsafe {
            (api.open)(
                Format {
                    sample_rate: format.sample_rate,
                    channels: u32::from(format.channels),
                },
                callbacks,
                &mut device,
            )
        })?;
        let device = NonNull::new(device)
            .ok_or_else(|| AudioError::new("Native PCM open returned no device"))?;
        Ok(Self {
            device,
            source,
            format,
            api,
        })
    }

    fn control(&mut self, operation: i32, epoch: u64) -> Result<(), AudioError> {
        self.check_callback()?;
        // SAFETY: This exclusively owned handle is live until Drop.
        let status = unsafe { (self.api.control)(self.device.as_ptr(), operation, epoch) };
        self.check_callback()?;
        check("control", status)
    }

    fn check_callback(&self) -> Result<(), AudioError> {
        if self.source.failed.load(Ordering::Acquire) {
            Err(AudioError::callback("PCM source callback failed"))
        } else {
            Ok(())
        }
    }
}

impl AudioOutput for NativeOutput {
    fn start(&mut self) -> Result<(), AudioError> {
        self.control(START, 0)
    }
    fn pause(&mut self) -> Result<(), AudioError> {
        self.control(PAUSE, 0)
    }
    fn flush(&mut self, epoch: u64) -> Result<(), AudioError> {
        self.control(FLUSH, epoch)
    }
    fn position(&self) -> Result<Position, AudioError> {
        self.check_callback()?;
        let mut position = NativePosition::default();
        // SAFETY: Backend position access is synchronized and the result pointer is valid.
        let status = unsafe { (self.api.position)(self.device.as_ptr(), &mut position) };
        self.check_callback()?;
        check("position", status)?;
        Ok(Position {
            epoch: position.epoch,
            device_frames: position.device_frames,
            latency_frames: position.latency_frames,
        })
    }
    fn format(&self) -> PcmFormat {
        self.format
    }
}

impl Drop for NativeOutput {
    fn drop(&mut self) {
        // SAFETY: close joins callbacks before Source is freed.
        unsafe {
            (self.api.close)(self.device.as_ptr());
        }
    }
}

unsafe extern "C" fn read(
    context: *mut c_void,
    output: *mut f32,
    frames: u32,
    channels: u32,
    epoch: u64,
) -> u32 {
    // SAFETY: Backend follows output.h and calls only while Source is live.
    let source = unsafe { &*context.cast::<Source>() };
    let Some(samples) = (frames as usize).checked_mul(channels as usize) else {
        return 0;
    };
    if output.is_null() || samples == 0 {
        return 0;
    }
    // SAFETY: Backend supplies frames*channels writable, aligned interleaved f32 samples.
    let output = unsafe { std::slice::from_raw_parts_mut(output, samples) };
    output.fill(0.0);
    if source.failed.load(Ordering::Acquire) {
        return 0;
    }
    match catch_unwind(AssertUnwindSafe(|| source.source.read(output))) {
        Ok(result) if result.epoch == epoch && result.media_frames <= frames => {
            output[result.media_frames as usize * channels as usize..].fill(0.0);
            result.media_frames
        }
        Ok(_) => {
            output.fill(0.0);
            0
        }
        Err(_) => {
            source.failed.store(true, Ordering::Release);
            output.fill(0.0);
            0
        }
    }
}

unsafe extern "C" fn played(context: *mut c_void, epoch: u64, frames: u64) {
    // SAFETY: Same pinned lifetime as read.
    let source = unsafe { &*context.cast::<Source>() };
    if catch_unwind(AssertUnwindSafe(|| source.source.played(epoch, frames))).is_err() {
        source.failed.store(true, Ordering::Release);
    }
}

fn check(operation: &str, status: i32) -> Result<(), AudioError> {
    if status == 0 {
        Ok(())
    } else {
        let message = format!("Native PCM {operation} failed: {status}");
        // output.h distinguishes device/OS faults from caller format errors.
        Err(if matches!(status, -10001 | -10003) {
            AudioError::invalid_format(message)
        } else {
            AudioError::new(message)
        })
    }
}

#[cfg(test)]
mod tests;
