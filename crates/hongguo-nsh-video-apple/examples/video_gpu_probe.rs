#[cfg(target_os = "macos")]
mod support;

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use hongguo_enhancement::Pipeline;
    use hongguo_nsh_video_api::Enhancement;
    use hongguo_nsh_video_apple::{MetalFrames, MetalSurface, VideoOutput};
    use objc2_foundation::{NSDate, NSRunLoop, NSString, NSURL};
    use std::time::{Duration, Instant};

    let path = std::fs::canonicalize(
        std::env::args()
            .nth(1)
            .ok_or("provide a synthetic local video path")?,
    )?;
    let source = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
        .absoluteString()
        .ok_or("local video has no URL")?;
    let surface = pollster::block_on(MetalSurface::new(objc2_quartz_core::CAMetalLayer::new()))?;
    let device = surface.device();
    let queue = surface.queue();
    let bridge = MetalFrames::new(device.clone())?;
    let video = VideoOutput::open(&source.to_string())?;
    video.set_volume(0.0, true)?;
    video.set_paused(false, 1.0)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let decoded = loop {
        if let Some(frame) = video.next_frame()? {
            break frame;
        }
        if Instant::now() >= deadline {
            return Err("AVPlayer did not expose a decoded frame".into());
        }
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
    };
    video.set_paused(true, 1.0)?;
    let metadata = decoded.metadata();
    let frame = bridge.import(decoded)?;
    let mut pipeline = pollster::block_on(Pipeline::new(
        device.clone(),
        queue.clone(),
        metadata.width,
        metadata.height,
    ))?;
    let mut samples = Vec::new();
    for (label, anime, sharpen, strength) in [
        ("raw", false, false, 50),
        ("sharp0", false, true, 0),
        ("sharp100", false, true, 100),
        ("anime2x", true, false, 50),
        ("combo", true, true, 100),
        ("reset", false, false, 50),
    ] {
        let mut options = Enhancement::default();
        options.anime4k_on = anime;
        options.sharpen_on = sharpen;
        options.strength = strength;
        pollster::block_on(pipeline.prepare(options))?;
        let mut encoder = device.create_command_encoder(&Default::default());
        let processed = pipeline.encode(
            &mut encoder,
            &frame.texture().create_view(&Default::default()),
        )?;
        queue.submit([encoder.finish()]);
        let expected = if anime {
            (metadata.width * 2, metadata.height * 2)
        } else {
            (metadata.width, metadata.height)
        };
        if (processed.width(), processed.height()) != expected {
            return Err(format!("{label} returned an incorrect video size").into());
        }
        let pixels = support::pixels(&device, &queue, processed.texture())?;
        if !pixels
            .as_chunks::<2>()
            .0
            .iter()
            .all(|bytes| u16::from_le_bytes(*bytes) & 0x7c00 != 0x7c00)
        {
            return Err(format!("{label} video output contains a non-finite half float").into());
        }
        println!(
            "{label}={}x{} finiteHalfPixels=true",
            processed.width(),
            processed.height()
        );
        samples.push(pixels);
    }
    if samples[0] != samples[1] {
        return Err("zero-strength sharpen changed decoded video pixels".into());
    }
    if samples[0] == samples[2] {
        return Err("sharpen100 did not change decoded video pixels".into());
    }
    if samples[3] == samples[4] {
        return Err("combined sharpen did not change CNN video pixels".into());
    }
    if samples[0] != samples[5] {
        return Err("disabling enhancement did not restore raw video pixels".into());
    }
    frame.retire_after_submitted_work(&queue);
    device.poll(wgpu::PollType::Poll)?;
    println!(
        "decodedVideoPtsMs={} input={}x{} nativeMetalImport=true realCnn2x=true sharpenChangesPixels=true zeroStrengthIdentity=true resetIdentity=true",
        metadata.pts_ms, metadata.width, metadata.height
    );
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("video_gpu_probe requires macOS");
    std::process::exit(2);
}
