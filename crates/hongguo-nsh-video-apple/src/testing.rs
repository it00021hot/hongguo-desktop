use crate::{controller::Viewport, render::Renderer};
use hongguo_media_types::{
    ChromaSampling, ColorMetadata, CropRect, FrameImage, NativeFrame, NativePlane,
    PresentationFrame,
};
use hongguo_nsh_video_api::{Enhancement, ResizeMode};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEventMask, NSScreen,
    NSView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode};
use objc2_quartz_core::CAMetalLayer;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub fn native_present_probe() -> Result<(), Box<dyn std::error::Error>> {
    let mtm = MainThreadMarker::new().ok_or("native present probe requires the main thread")?;
    let application = NSApplication::sharedApplication(mtm);
    application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    application.finishLaunching();
    let screen = NSScreen::mainScreen(mtm).ok_or("no display for native presentation probe")?;
    let visible = screen.visibleFrame();
    let rect = CGRect::new(
        CGPoint::new(visible.origin.x + 20.0, visible.origin.y + 20.0),
        CGSize::new(160.0, 90.0),
    );
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            rect,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // Retained owns the window until this probe closes and drops it.
    unsafe {
        window.setReleasedWhenClosed(false);
    }
    window.setIgnoresMouseEvents(true);
    // A non-activating probe must also be visible over the currently active
    // full-screen Space; this changes only the probe's own tiny window.
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    window.setLevel(3);
    let view = NSView::initWithFrame(NSView::alloc(mtm), CGRect::new(CGPoint::ZERO, rect.size));
    view.setWantsLayer(true);
    let layer = CAMetalLayer::new();
    layer.setFrame(CGRect::new(CGPoint::ZERO, rect.size));
    view.setLayer(Some(&layer));
    window.setContentView(Some(&view));
    window.orderFrontRegardless();
    if std::env::var_os("HONGGUO_NATIVE_PRESENT_ACTIVATE").is_some() {
        application.activate();
    }
    pump_events(&application, Duration::from_millis(50));
    let result = pollster::block_on(async {
        let mut renderer = Renderer::new(layer).await?;
        let viewport = Viewport {
            width_px: 160,
            height_px: 90,
            width_points: 160.0,
            height_points: 90.0,
            ..Viewport::default()
        };
        let frame = Arc::new(PresentationFrame {
            serial: 1,
            generation: 1,
            pts_secs: 0.0,
            duration_secs: 1.0 / 30.0,
            image: FrameImage::Cpu(Arc::new(NativeFrame {
                width: 3,
                height: 3,
                bit_depth: 10,
                chroma: ChromaSampling::Monochrome,
                planes: vec![NativePlane {
                    data: Arc::from(
                        (0..9)
                            .flat_map(|_| 400_u16.to_le_bytes())
                            .collect::<Vec<_>>(),
                    ),
                    stride: 6,
                    width: 3,
                    height: 3,
                    bytes_per_sample: 2,
                }],
                pts: Some(0.0),
                crop: CropRect {
                    x: 0,
                    y: 0,
                    width: 3,
                    height: 3,
                },
                rotation_degrees: 0,
                color: ColorMetadata {
                    primaries: Some(9),
                    transfer: Some(16),
                    matrix: Some(9),
                    full_range: Some(false),
                    ..Default::default()
                },
            })),
        });
        let imported = renderer.upload_native(frame, viewport.color_target())?;
        assert!(
            renderer
                .draw(
                    &imported,
                    viewport,
                    Enhancement::default(),
                    ResizeMode::Contain,
                    1,
                    || true
                )
                .await?
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut skipped = 0;
        loop {
            pump_events(&application, Duration::from_millis(10));
            if let Some(completed) = renderer.poll()? {
                if completed.displayed {
                    println!(
                        "nativePresented=true skippedDrawables={skipped} queueCompletionMilliseconds={:.3}",
                        completed.elapsed.as_secs_f64() * 1000.0
                    );
                    return Ok::<(), crate::OutputError>(());
                }
                skipped += 1;
                let _ = renderer
                    .draw(
                        &imported,
                        viewport,
                        Enhancement::default(),
                        ResizeMode::Contain,
                        1,
                        || true,
                    )
                    .await?;
            }
            if Instant::now() >= deadline {
                return Err(crate::OutputError::new(
                    hongguo_nsh_video_api::ErrorCode::GpuFailed,
                    format!(
                        "native drawable never reported presentation: skipped={skipped}, visible={}, occlusion={:?}",
                        window.isVisible(),
                        window.occlusionState()
                    ),
                ));
            }
        }
    });
    window.orderOut(None);
    window.close();
    result.map_err(Into::into)
}

fn pump_events(application: &NSApplication, duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        // A run loop alone does not dispatch AppKit's window/occlusion events.
        // This pumps only the probe process's own application queue.
        let until = NSDate::dateWithTimeIntervalSinceNow(0.005);
        if let Some(event) = application.nextEventMatchingMask_untilDate_inMode_dequeue(
            NSEventMask::Any,
            Some(&until),
            unsafe { NSDefaultRunLoopMode },
            true,
        ) {
            application.sendEvent(&event);
        }
        application.updateWindows();
    }
}
