#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use hongguo_nsh_video_api::{EnhancementPhase, Seek, Source, VideoEvent, VideoProps};
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
            .ok_or("provide a synthetic local video path")?,
    )?;
    let (sender, receiver) = mpsc::channel();
    let controller =
        PlayerController::spawn(OriginalLayer::new()?, CAMetalLayer::new(), move |event| {
            let _ = sender.send(event);
        })?;
    controller.resize(320, 180, 320.0, 180.0, 1.0)?;
    let mut source = Source::default();
    source.id = "raw-worker-probe".into();
    source.url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
        .absoluteString()
        .ok_or("local source has no URL")?
        .to_string();
    let mut props = VideoProps::default();
    props.source = Some(source);
    props.paused = false;
    props.muted = true;
    props.active = true;
    props.progress_interval_ms = 50;
    controller.set_props(props.clone())?;
    let mut loaded = false;
    let mut idle = false;
    let mut paused_at = None;
    let mut seeked = false;
    let mut stable_samples = 0;
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline && stable_samples < 3 {
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
        while let Ok(event) = receiver.try_recv() {
            match event {
                VideoEvent::Load {
                    width: 320,
                    height: 180,
                    ..
                } => loaded = true,
                VideoEvent::EnhancementStatus {
                    phase: EnhancementPhase::Idle,
                    ..
                } => idle = true,
                VideoEvent::EnhancementStatus {
                    phase: EnhancementPhase::Loading | EnhancementPhase::Ready,
                    ..
                } => {
                    return Err("all-off raw playback initialized the GPU renderer".into());
                }
                VideoEvent::Error { detail, .. } => return Err(detail.into()),
                VideoEvent::Progress { position_ms, .. }
                    if position_ms >= 200 && paused_at.is_none() =>
                {
                    paused_at = Some(position_ms);
                    props.paused = true;
                    controller.set_props(props.clone())?;
                    let mut request = Seek::default();
                    request.source_id = "raw-worker-probe".into();
                    request.request_id = "paused-native-seek".into();
                    request.position_ms = 1_000;
                    controller.seek(request)?;
                }
                VideoEvent::Seeked {
                    request_id,
                    position_ms,
                    ..
                } if request_id == "paused-native-seek" => {
                    if position_ms.abs_diff(1_000) > 1 {
                        return Err(
                            format!("incorrect native seek confirmation: {position_ms}").into()
                        );
                    }
                    seeked = true;
                }
                VideoEvent::Progress { position_ms, .. } if seeked => {
                    if position_ms.abs_diff(1_000) > 1 {
                        return Err(
                            format!("paused native clock moved after seek: {position_ms}").into(),
                        );
                    }
                    stable_samples += 1;
                }
                _ => {}
            }
        }
    }
    controller.stop();
    let deadline = Instant::now() + Duration::from_secs(1);
    while !controller.is_stopped() && Instant::now() < deadline {
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
    }
    if !(loaded && idle && seeked && stable_samples >= 3 && controller.is_stopped()) {
        return Err(format!("raw worker evidence incomplete: loaded={loaded}, idle={idle}, seeked={seeked}, stable={stable_samples}, stopped={}", controller.is_stopped()).into());
    }
    println!(
        "defaultRawNoGpu=true loaded320x180=true nativePausedSeekMs=1000 stableSamples={stable_samples} workerStopped=true"
    );
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("raw_worker_probe requires macOS");
    std::process::exit(2);
}
