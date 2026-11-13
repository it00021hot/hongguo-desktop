//! Simulator-only runtime check of the production audio-session lease. This app
//! owns no media and never changes another app's playback or stored preferences.
#[cfg(target_os = "ios")]
#[path = "../src/audio.rs"]
mod audio;

#[cfg(target_os = "ios")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use objc2::{msg_send, rc::Retained, runtime::AnyClass};
    use objc2_foundation::NSString;
    use std::sync::{Arc, Mutex, Weak};

    // SAFETY: The process starts on the simulator application's UIKit thread;
    // these are public AVAudioSession read-only getters.
    fn snapshot() -> Result<(String, String, usize), String> {
        unsafe {
            let class = AnyClass::get(c"AVAudioSession").ok_or("missing AVAudioSession")?;
            let session: Retained<objc2::runtime::AnyObject> = msg_send![class, sharedInstance];
            let category: Retained<NSString> = msg_send![&*session, category];
            let mode: Retained<NSString> = msg_send![&*session, mode];
            let options: usize = msg_send![&*session, categoryOptions];
            Ok((category.to_string(), mode.to_string(), options))
        }
    }

    // SAFETY: Public Foundation methods, with retained objects and finite dates.
    fn drain_main_queue() -> Result<(), String> {
        unsafe {
            let run_loop = AnyClass::get(c"NSRunLoop").ok_or("missing NSRunLoop")?;
            let date = AnyClass::get(c"NSDate").ok_or("missing NSDate")?;
            let run_loop: Retained<objc2::runtime::AnyObject> = msg_send![run_loop, currentRunLoop];
            let until: Retained<objc2::runtime::AnyObject> =
                msg_send![date, dateWithTimeIntervalSinceNow: 0.1f64];
            let _: () = msg_send![&*run_loop, runUntilDate: &*until];
            Ok(())
        }
    }

    let before = snapshot()?;
    let first = audio::Lease::new();
    let second = audio::Lease::new();
    first.play("first-source")?;
    let wanted = snapshot()?;
    assert_eq!(wanted.0, "AVAudioSessionCategoryPlayback");
    assert_eq!(wanted.1, "AVAudioSessionModeSpokenAudio");
    assert_eq!(wanted.2, 0);
    second.play("second-source")?;
    first.release()?;
    assert_eq!(
        snapshot()?,
        wanted,
        "first release interrupted second owner"
    );

    let control = Arc::new(Mutex::new(Weak::new()));
    audio::terminal(second.key(), "stale-source".into(), Arc::clone(&control));
    drain_main_queue()?;
    assert_eq!(snapshot()?, wanted, "stale terminal released active source");
    audio::terminal(second.key(), "second-source".into(), control);
    drain_main_queue()?;
    assert_eq!(
        snapshot()?,
        before,
        "final owner did not restore original category"
    );

    // A fresh owner must work after terminal cleanup, including ordinary drop.
    let third = audio::Lease::new();
    third.play("third-source")?;
    assert_eq!(snapshot()?, wanted);
    drop(third);
    assert_eq!(
        snapshot()?,
        before,
        "drop did not restore original category"
    );
    println!(
        "IOS_AUDIO_LEASE_PROBE_OK: playback/spokenAudio; two owners; stale terminal ignored; final terminal and drop restored category"
    );
    Ok(())
}

#[cfg(not(target_os = "ios"))]
fn main() {
    eprintln!("audio_probe must run as a separate iOS Simulator application");
    std::process::exit(2);
}
