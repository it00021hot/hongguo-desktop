use super::*;
use crate::ReadResult;
use std::sync::atomic::AtomicU64;

struct Fixture {
    epoch: AtomicU64,
    reads: AtomicU64,
    played: AtomicU64,
}

impl PcmSource for Fixture {
    fn read(&self, output: &mut [f32]) -> ReadResult {
        self.reads.fetch_add(1, Ordering::Relaxed);
        output.fill(0.5);
        ReadResult {
            epoch: self.epoch.load(Ordering::Relaxed),
            media_frames: 1,
        }
    }
    fn played(&self, _epoch: u64, frames: u64) {
        self.played.store(frames, Ordering::Relaxed);
    }
}

fn fixture() -> Arc<Fixture> {
    Arc::new(Fixture {
        epoch: AtomicU64::new(7),
        reads: AtomicU64::new(0),
        played: AtomicU64::new(0),
    })
}

struct Device {
    callbacks: Callbacks,
    epoch: u64,
    consumed: u64,
    output: [f32; 8],
    error: i32,
}

unsafe extern "C" fn open(_format: Format, callbacks: Callbacks, device: *mut *mut c_void) -> i32 {
    let state = Box::new(Device {
        callbacks,
        epoch: 0,
        consumed: 0,
        output: [0.0; 8],
        error: 0,
    });
    // SAFETY: The test caller passes a live out pointer, matching output.h.
    unsafe {
        *device = Box::into_raw(state).cast();
    }
    0
}
unsafe extern "C" fn control(device: *mut c_void, operation: i32, epoch: u64) -> i32 {
    // SAFETY: This pointer was made by open and remains live until close.
    let state = unsafe { &mut *device.cast::<Device>() };
    if state.error != 0 {
        return state.error;
    }
    if operation == FLUSH {
        state.epoch = epoch;
        state.consumed = 0;
    }
    if operation == START {
        // SAFETY: The fixture gives eight aligned samples for four stereo frames.
        unsafe {
            (state.callbacks.read)(
                state.callbacks.context,
                state.output.as_mut_ptr(),
                4,
                2,
                state.epoch,
            );
        }
    }
    0
}
unsafe extern "C" fn position(device: *mut c_void, position: *mut NativePosition) -> i32 {
    // SAFETY: Both pointers follow the test's output.h contract.
    let state = unsafe { &*device.cast::<Device>() };
    unsafe {
        (*position).epoch = state.epoch;
        (*position).device_frames = state.consumed;
        (state.callbacks.played)(state.callbacks.context, state.epoch, state.consumed);
    }
    0
}
unsafe extern "C" fn close(device: *mut c_void) {
    // SAFETY: Each successfully opened test device is closed exactly once.
    drop(unsafe { Box::from_raw(device.cast::<Device>()) });
}
static API: Api = Api {
    open,
    control,
    position,
    close,
};

#[test]
fn opening_does_not_consume_and_queued_frames_do_not_advance_the_device_clock() {
    let source = fixture();
    // SAFETY: The fixture implements output.h synchronously.
    let mut output = unsafe {
        NativeOutput::open(
            PcmFormat {
                sample_rate: 48000,
                channels: 2,
            },
            source.clone(),
            &API,
        )
    }
    .unwrap();
    assert_eq!(source.reads.load(Ordering::Relaxed), 0);
    output.flush(7).unwrap();
    output.start().unwrap();
    assert_eq!(output.position().unwrap().device_frames, 0);
    assert_eq!(source.played.load(Ordering::Relaxed), 0);
    // SAFETY: Device belongs exclusively to this test through NativeOutput.
    let state = unsafe { output.device.cast::<Device>().as_ref() };
    assert_eq!(state.output, [0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    drop(output);
    assert_eq!(Arc::strong_count(&source), 1);
}

#[test]
fn a_stale_epoch_is_silenced_and_flush_resets_the_device_position() {
    let source = fixture();
    // SAFETY: Same synchronous fixture API.
    let mut output = unsafe {
        NativeOutput::open(
            PcmFormat {
                sample_rate: 48000,
                channels: 2,
            },
            source,
            &API,
        )
    }
    .unwrap();
    output.flush(8).unwrap();
    output.start().unwrap();
    // SAFETY: The test owns the opaque fixture handle exclusively.
    assert_eq!(
        unsafe { output.device.cast::<Device>().as_ref() }.output,
        [0.0; 8]
    );
    assert_eq!(
        output.position().unwrap(),
        Position {
            epoch: 8,
            device_frames: 0,
            latency_frames: 0
        }
    );
}

#[test]
fn invalid_format_never_reaches_native_open() {
    for format in [
        PcmFormat {
            sample_rate: 0,
            channels: 2,
        },
        PcmFormat {
            sample_rate: 48000,
            channels: 0,
        },
    ] {
        // SAFETY: The fixture is valid; format validation happens before calling it.
        let error = unsafe { NativeOutput::open(format, fixture(), &API) }
            .err()
            .unwrap();
        assert_eq!(error.kind(), crate::AudioErrorKind::InvalidFormat);
    }
}

#[test]
fn rust_and_c_position_contract_has_explicit_tail_padding() {
    assert_eq!(size_of::<Format>(), 8);
    assert_eq!(size_of::<NativePosition>(), 24);
    assert_eq!(size_of::<Callbacks>(), size_of::<usize>() * 3);
}

#[test]
fn a_panicking_played_callback_fails_the_same_position_query() {
    struct Panics;
    impl PcmSource for Panics {
        fn read(&self, _: &mut [f32]) -> ReadResult {
            ReadResult::default()
        }
        fn played(&self, _: u64, _: u64) {
            panic!("fixture callback failure");
        }
    }
    // SAFETY: Same synchronous fixture API; its Rust callback panic must be contained.
    let mut output = unsafe {
        NativeOutput::open(
            PcmFormat {
                sample_rate: 48000,
                channels: 2,
            },
            Arc::new(Panics),
            &API,
        )
    }
    .unwrap();
    assert_eq!(
        output.position().unwrap_err().kind(),
        crate::AudioErrorKind::Callback
    );
    assert_eq!(
        output.start().unwrap_err().kind(),
        crate::AudioErrorKind::Callback
    );
}

#[test]
fn native_control_keeps_device_faults_distinct_from_invalid_formats() {
    // SAFETY: Same synchronous fixture API, with a test-owned error injection.
    let mut output = unsafe {
        NativeOutput::open(
            PcmFormat {
                sample_rate: 48_000,
                channels: 2,
            },
            fixture(),
            &API,
        )
    }
    .unwrap();
    for (status, expected) in [
        (-10003, crate::AudioErrorKind::InvalidFormat),
        (-10004, crate::AudioErrorKind::Device),
    ] {
        // SAFETY: The opaque fixture device is exclusively owned by this test.
        unsafe { output.device.cast::<Device>().as_mut() }.error = status;
        assert_eq!(output.start().unwrap_err().kind(), expected);
    }
}
