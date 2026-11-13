#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use hongguo_nsh_video_api::{EnhancementPhase, Source, VideoEvent, VideoProps};
    use hongguo_nsh_video_apple::{OriginalLayer, PlayerController};
    use nsh_platform_macos::{
        objc2_app_kit::{
            NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEventMask, NSView,
            NSWindow, NSWindowStyleMask,
        },
        objc2_foundation::{
            NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize, NSString, NSURL,
        },
    };
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_quartz_core::{CALayer, CAMetalLayer};
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    let media = std::fs::canonicalize(
        std::env::args()
            .nth(1)
            .ok_or("provide a synthetic local video path")?,
    )?;
    let directory = std::env::args()
        .nth(2)
        .ok_or("provide an output directory")?;
    std::fs::create_dir_all(&directory)?;
    let directory = std::fs::canonicalize(directory)?;
    let mtm = MainThreadMarker::new().ok_or("probe must run on the main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    app.finishLaunching();
    let bounds = NSRect::new(NSPoint::ZERO, NSSize::new(640.0, 360.0));
    // SAFETY: A main-thread, independently owned engineering-probe window.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            bounds,
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: The retained window lives until this probe closes it explicitly.
    unsafe {
        window.setReleasedWhenClosed(false);
    }
    window.setTitle(&NSString::from_str("Hongguo pixel presenter probe"));
    let view = NSView::initWithFrame(NSView::alloc(mtm), bounds);
    let root = CALayer::new();
    let original = OriginalLayer::new()?;
    let metal = CAMetalLayer::new();
    root.addSublayer(&original.layer());
    root.addSublayer(&metal);
    view.setWantsLayer(true);
    view.setLayer(Some(&root));
    window.setContentView(Some(&view));
    window.center();
    window.makeKeyAndOrderFront(None);
    app.activate();
    let (sender, receiver) = mpsc::channel();
    let controller = PlayerController::spawn(original, metal, move |event| {
        let _ = sender.send(event);
    })?;
    let backing = view.convertSizeToBacking(bounds.size);
    controller.resize(
        backing.width.round() as u32,
        backing.height.round() as u32,
        640.0,
        360.0,
        backing.width / 640.0,
    )?;
    let mut props = VideoProps::default();
    let mut source = Source::default();
    source.id = "synthetic-presenter-probe".into();
    source.url = NSURL::fileURLWithPath(&NSString::from_str(&media.to_string_lossy()))
        .absoluteString()
        .ok_or("local media has no URL")?
        .to_string();
    props.source = Some(source);
    props.paused = false;
    props.muted = true;
    props.active = true;
    controller.set_props(props.clone())?;
    let mut stage = 0;
    let deadline = Instant::now() + Duration::from_secs(30);
    while stage < 3 && Instant::now() < deadline {
        // NSApplication must drain AppKit events as well as the CF run loop:
        // otherwise a newly ordered standalone window stays natively occluded.
        // SAFETY: Foundation's global mode is valid for this main-thread AppKit event loop.
        let event = unsafe {
            app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&NSDate::dateWithTimeIntervalSinceNow(0.01)),
                NSDefaultRunLoopMode,
                true,
            )
        };
        if let Some(event) = event {
            app.sendEvent(&event);
        }
        app.updateWindows();
        while let Ok(event) = receiver.try_recv() {
            println!("{}", serde_json::to_string(&event)?);
            match event {
                VideoEvent::Load { .. } => {
                    props.paused = true;
                    controller.set_props(props.clone())?;
                }
                VideoEvent::Error { detail, .. } => return Err(detail.into()),
                VideoEvent::EnhancementStatus {
                    phase: EnhancementPhase::Error,
                    detail,
                    ..
                } => return Err(detail.into()),
                VideoEvent::EnhancementStatus { phase, .. }
                    if (stage == 0 && phase == EnhancementPhase::Idle)
                        || (stage == 1 && phase == EnhancementPhase::Ready)
                        || (stage == 2 && phase == EnhancementPhase::Idle) =>
                {
                    // Core Animation presents after the render completion event. Drain the
                    // run loop for a bounded display interval before the OS-level capture.
                    app.updateWindows();
                    let label = ["raw", "anime-sharp", "reset"][stage];
                    let status = std::process::Command::new("/usr/sbin/screencapture")
                        .args(["-x", "-l", &window.windowNumber().to_string()])
                        .arg(directory.join(format!("{label}.png")))
                        .status()?;
                    if !status.success() {
                        return Err("OS window capture failed".into());
                    }
                    stage += 1;
                    props.enhancement.anime4k_on = stage == 1;
                    props.enhancement.sharpen_on = stage == 1;
                    props.enhancement.strength = 100;
                    controller.set_props(props.clone())?;
                }
                _ => {}
            }
        }
    }
    controller.stop();
    window.close();
    if stage != 3 {
        return Err(
            format!("presenter did not complete the three real display stages: {stage}").into(),
        );
    }
    println!(
        "presentedRaw=true presentedAnimeAndSharpen=true resetToRaw=true source=synthetic-only"
    );
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("present_probe requires macOS");
    std::process::exit(2);
}
