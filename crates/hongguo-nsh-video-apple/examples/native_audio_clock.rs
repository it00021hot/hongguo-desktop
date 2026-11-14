use std::{
    ffi::c_void,
    sync::mpsc,
    time::{Duration, Instant},
};

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopDefaultMode: *const c_void;
    fn CFRunLoopRunInMode(
        mode: *const c_void,
        seconds: f64,
        return_after_source_handled: u8,
    ) -> i32;
}

fn main() -> Result<(), String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let result =
            std::panic::catch_unwind(hongguo_nsh_video_apple::testing::probe_native_audio_clock);
        let _ = sender.send(result.is_ok());
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let success = loop {
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.01, 0) };
        match receiver.try_recv() {
            Ok(success) => break success,
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("native audio probe worker disconnected".into());
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if Instant::now() >= deadline {
            return Err("native audio probe exceeded its deadline".into());
        }
    };
    worker
        .join()
        .map_err(|_| "native audio probe worker panicked")?;
    if success {
        Ok(())
    } else {
        Err("native audio clock verification failed".into())
    }
}
