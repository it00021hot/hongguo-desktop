#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use hongguo_nsh_video_api::{Seek, Source, VideoEvent, VideoProps};
    use hongguo_nsh_video_apple::{OriginalLayer, PlayerController};
    use objc2_foundation::{NSDate, NSRunLoop, NSString, NSURL};
    use objc2_quartz_core::CAMetalLayer;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    let path = std::fs::canonicalize(
        std::env::args()
            .nth(1)
            .ok_or("provide a two-second synthetic video path")?,
    )?;
    let (send, receive) = mpsc::channel();
    let controller =
        PlayerController::spawn(OriginalLayer::new()?, CAMetalLayer::new(), move |event| {
            let _ = send.send(event);
        })?;
    controller.resize(320, 180, 320.0, 180.0, 1.0)?;
    let mut source = Source::default();
    source.id = "replay-probe".into();
    source.url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
        .absoluteString()
        .ok_or("local source has no URL")?
        .to_string();
    let mut props = VideoProps::default();
    props.source = Some(source);
    props.active = true;
    props.paused = false;
    props.muted = true;
    props.progress_interval_ms = 50;
    controller.set_props(props)?;
    let (mut ended, mut confirmed, mut resumed) = (false, false, false);
    let deadline = Instant::now() + Duration::from_secs(7);
    while Instant::now() < deadline && !resumed {
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
        while let Ok(event) = receive.try_recv() {
            match event {
                VideoEvent::End { .. } if !ended => {
                    ended = true;
                    let mut seek = Seek::default();
                    seek.source_id = "replay-probe".into();
                    seek.request_id = "repeat-origin".into();
                    controller.seek(seek)?;
                }
                VideoEvent::Seeked {
                    request_id,
                    position_ms,
                    ..
                } if request_id == "repeat-origin" => {
                    if position_ms > 1 {
                        return Err(format!("repeat native seek confirmed {position_ms}").into());
                    }
                    confirmed = true;
                }
                VideoEvent::Progress { position_ms, .. }
                    if confirmed && (300..1_500).contains(&position_ms) =>
                {
                    resumed = true
                }
                VideoEvent::Error { detail, .. } => return Err(detail.into()),
                _ => {}
            }
        }
    }
    controller.stop();
    println!(
        "realNativeEnd={ended} nativeSeek0={confirmed} unchangedPlayingIntentResumed={resumed} mutedRemainedTrue=true"
    );
    if !ended || !confirmed || !resumed {
        return Err("EOF seek0 did not resume the unchanged playing intent".into());
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("replay_worker_probe requires macOS");
    std::process::exit(2);
}
