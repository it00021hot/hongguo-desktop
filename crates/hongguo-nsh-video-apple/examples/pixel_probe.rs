#[cfg(target_os = "macos")]
mod support;

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use hongguo_nsh_video_apple::{MetalFrames, MetalSurface, VideoOutput};
    use objc2_foundation::{NSDate, NSRunLoop, NSString, NSURL};
    use std::{
        collections::HashSet,
        time::{Duration, Instant},
    };

    const REQUIRED_FRAMES: usize = 3;
    const DEADLINE: Duration = Duration::from_secs(10);
    let path = std::env::args()
        .nth(1)
        .ok_or("provide a synthetic local video path")?;
    let path = std::fs::canonicalize(path)?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let source = url.absoluteString().ok_or("local video has no URL")?;
    let surface = pollster::block_on(MetalSurface::new(objc2_quartz_core::CAMetalLayer::new()))?;
    let device = surface.device();
    let queue = surface.queue();
    let bridge = MetalFrames::new(device.clone())?;
    let video = VideoOutput::open(&source.to_string())?;
    video.set_volume(0.0, true)?;
    video.set_paused(false, 1.0)?;
    let start = Instant::now();
    let mut timestamps = Vec::new();
    let mut hashes = HashSet::new();
    let mut dimensions = (0, 0);
    while timestamps.len() < REQUIRED_FRAMES && start.elapsed() < DEADLINE {
        if let Some(frame) = video.next_frame()? {
            let metadata = frame.metadata();
            dimensions = (metadata.width, metadata.height);
            let frame = bridge.import(frame)?;
            let hash = read_frame(&device, &queue, frame.texture())?;
            timestamps.push(metadata.pts_ms);
            hashes.insert(hash);
            frame.retire_after_submitted_work(&queue);
            device.poll(wgpu::PollType::Poll)?;
        }
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
    }
    video.set_paused(true, 1.0)?;
    if timestamps.len() < REQUIRED_FRAMES || hashes.len() < 2 {
        return Err(format!(
            "decoded/imported frame evidence incomplete: pts={timestamps:?}, uniquePixels={}",
            hashes.len()
        )
        .into());
    }
    if !timestamps.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(format!("decoded PTS did not advance: {timestamps:?}").into());
    }
    verify_paused_seek(&video, &bridge, &device, &queue)?;
    println!(
        "decodedFrames={} size={}x{} ptsMs={timestamps:?} uniqueGpuPixelHashes={} pausedSeekMs=1000 pausedPositionStable=true",
        timestamps.len(),
        dimensions.0,
        dimensions.1,
        hashes.len()
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn verify_paused_seek(
    video: &hongguo_nsh_video_apple::VideoOutput,
    bridge: &hongguo_nsh_video_apple::MetalFrames,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), Box<dyn std::error::Error>> {
    use objc2_foundation::{NSDate, NSRunLoop};
    use std::{
        sync::mpsc::TryRecvError,
        time::{Duration, Instant},
    };
    const SEEK_MS: u64 = 1_000;
    let completion = video.seek(SEEK_MS)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match completion.try_recv() {
            Ok(true) => break,
            Ok(false) => return Err("native paused seek was cancelled".into()),
            Err(TryRecvError::Disconnected) => {
                return Err("native seek completion disconnected".into());
            }
            Err(TryRecvError::Empty) if Instant::now() >= deadline => {
                return Err("native seek completion timed out".into());
            }
            Err(TryRecvError::Empty) => NSRunLoop::currentRunLoop()
                .runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01)),
        }
    }
    let actual = video
        .position_ms()
        .ok_or("native seek has no numeric position")?;
    if actual.abs_diff(SEEK_MS) > 1 {
        return Err(format!("native seek confirmed an incorrect position: {actual}").into());
    }
    loop {
        if let Some(frame) = video.next_frame()? {
            if frame.metadata().pts_ms.abs_diff(SEEK_MS) > 100 {
                return Err(format!(
                    "paused seek exposed old pixels at {}ms",
                    frame.metadata().pts_ms
                )
                .into());
            }
            let frame = bridge.import(frame)?;
            read_frame(device, queue, frame.texture())?;
            frame.retire_after_submitted_work(queue);
            break;
        }
        if Instant::now() >= deadline {
            return Err("paused seek produced no decoded pixels".into());
        }
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
    }
    NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.2));
    if video.position_ms() != Some(actual) {
        return Err("paused seek changed playback position without resuming".into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn read_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> Result<u64, Box<dyn std::error::Error>> {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in support::pixels(device, queue, texture)? {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    Ok(hash)
}

#[cfg(not(target_os = "macos"))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(2)
}
