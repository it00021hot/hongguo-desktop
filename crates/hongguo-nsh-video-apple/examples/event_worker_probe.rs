#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use hongguo_nsh_video_api::{Seek, Source, VideoEvent, VideoProps};
    use hongguo_nsh_video_apple::{OriginalLayer, PlayerController, VideoOutput};
    use objc2_foundation::{NSDate, NSRunLoop};
    use objc2_quartz_core::CAMetalLayer;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    let url = std::env::args()
        .nth(1)
        .ok_or("provide the localhost synthetic EVENT HLS URL")?;
    let seek_origin = std::env::args()
        .skip(2)
        .any(|value| value == "--seek-origin");
    let finish = std::env::args()
        .skip(2)
        .find_map(|value| value.strip_prefix("--finish-event=").map(str::to_owned));
    let finish = finish.map(std::fs::canonicalize).transpose()?;
    if let Some(path) = &finish
        && (!path.starts_with(std::env::current_dir()?.join("target"))
            || !std::fs::read_to_string(path)?.contains("#EXT-X-PLAYLIST-TYPE:EVENT"))
    {
        return Err("only the probe's synthetic target/ EVENT playlist can be finalized".into());
    }
    if !url.starts_with("http://127.0.0.1:") {
        return Err("probe only accepts its own localhost synthetic server".into());
    }
    {
        let video = VideoOutput::open(&url)?;
        video.set_volume(0.0, true)?;
        video.set_paused(false, 1.0)?;
        let mut frames = 0;
        let mut first = None;
        let mut last = 0;
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
            if let Some(frame) = video.next_frame()? {
                frames += 1;
                first.get_or_insert(frame.metadata().pts_ms);
                last = frame.metadata().pts_ms;
            }
        }
        println!(
            "nativeFrames={frames} firstPtsMs={first:?} lastPtsMs={last} nativePositionMs={:?} nativeDurationMs={:?}",
            video.position_ms(),
            video.duration_ms()
        );
        if frames < 2 || first.is_none_or(|first| last <= first) {
            return Err("synthetic EVENT HLS did not decode advancing video frames".into());
        }
    }
    let (send, receive) = mpsc::channel();
    let controller =
        PlayerController::spawn(OriginalLayer::new()?, CAMetalLayer::new(), move |event| {
            let _ = send.send(event);
        })?;
    controller.resize(320, 180, 320.0, 180.0, 1.0)?;
    let mut source = Source::default();
    source.id = "event-hls-probe".into();
    source.url = url;
    let mut props = VideoProps::default();
    props.source = Some(source);
    props.active = true;
    props.paused = false;
    props.muted = true;
    props.progress_interval_ms = 100;
    controller.set_props(props.clone())?;
    let (mut loads, mut progress) = (0, 0);
    let (mut finite_loads, mut finite_progress) = (0, 0);
    let (mut ends, mut last_position) = (0, None);
    let (mut origin_confirmed, mut stable_origin) = (false, 0);
    let deadline = Instant::now() + Duration::from_secs(if finish.is_some() { 8 } else { 4 });
    while Instant::now() < deadline {
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
        while let Ok(event) = receive.try_recv() {
            match event {
                VideoEvent::Load { duration_ms, .. } => {
                    loads += 1;
                    println!("{event:?}");
                    if duration_ms.is_some() {
                        finite_loads += 1;
                    }
                    if loads == 1
                        && let Some(path) = &finish
                    {
                        use std::io::Write;
                        // This models the real producer sealing its own synthetic EVENT.
                        std::fs::OpenOptions::new()
                            .append(true)
                            .open(path)?
                            .write_all(b"#EXT-X-ENDLIST\n")?;
                    }
                    if seek_origin && loads == 1 {
                        props.paused = true;
                        controller.set_props(props.clone())?;
                        let mut request = Seek::default();
                        request.source_id = "event-hls-probe".into();
                        request.request_id = "origin-seek".into();
                        controller.seek(request)?;
                    }
                }
                VideoEvent::Progress {
                    position_ms,
                    duration_ms,
                    ..
                } => {
                    progress += 1;
                    last_position = Some(position_ms);
                    if duration_ms.is_some() {
                        finite_progress += 1;
                    }
                    if progress <= 2 {
                        println!("{event:?}");
                    }
                    if origin_confirmed {
                        if position_ms > 1 {
                            return Err(format!(
                                "paused EVENT origin clock drifted to {position_ms}"
                            )
                            .into());
                        }
                        stable_origin += 1;
                    }
                }
                VideoEvent::Seeked {
                    request_id,
                    position_ms,
                    ..
                } if request_id == "origin-seek" => {
                    if position_ms > 1 {
                        return Err(format!(
                            "EVENT origin seek confirmed {position_ms}, expected0"
                        )
                        .into());
                    }
                    origin_confirmed = true;
                }
                VideoEvent::Error { detail, .. } => return Err(detail.into()),
                VideoEvent::End { .. } => {
                    ends += 1;
                    println!("eventNativeEnd=true lastPositionMs={last_position:?}");
                }
                _ => {}
            }
        }
    }
    controller.stop();
    println!("eventWorkerLoads={loads} eventWorkerProgress={progress}");
    if loads == 0 || progress < 2 {
        return Err("decoded EVENT HLS produced no usable Worker Load/Progress while duration was indefinite".into());
    }
    if seek_origin && (!origin_confirmed || stable_origin < 2) {
        return Err("EVENT initial seek0 did not confirm a stable native origin".into());
    }
    if seek_origin {
        println!("eventOriginSeekedMs=0 stableNativeOriginSamples={stable_origin}");
    }
    if finish.is_some() {
        if ends == 0 {
            return Err(
                "sealing EVENT did not report the real native end within the bounded probe".into(),
            );
        }
        println!(
            "eventSealedFiniteLoads={finite_loads} eventSealedFiniteProgress={finite_progress}"
        );
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("event_worker_probe requires macOS");
    std::process::exit(2);
}
