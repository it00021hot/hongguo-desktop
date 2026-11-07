//! 离线分测已有明文 ByteVC2：解码、以及原分辨率的解码+编码+封装。
//! 不删除产物。用法：
//! `bytevc2_stages <输入> <输出mp4> <阶段json>`

use std::path::Path;
use std::time::Instant;

use hongguo_media_platform::platform::PlatformRequest;
use hongguo_media_platform::{RateProfile, transcode_bytevc2};

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("输入");
    let output = args.next().expect("输出");
    let stage = args.next().expect("阶段 json");
    let input_path = Path::new(&input);
    let demuxed = hongguo_media::demux::demux_file(input_path).expect("输入应能解复用");
    let video = demuxed.video_track().expect("输入应有视频轨");
    let pts = video.info.sample_pts();
    let mut present = pts.clone();
    present.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let audio_samples = demuxed
        .audio_track()
        .map(|t| t.info.samples.len())
        .unwrap_or(0);
    let mid = pts.len() / 2;
    println!(
        "input_samples={} input_wh={}x{} input_fps={:.6} input_audio_samples={} input_pts_first={:?} input_pts_decode_mid={:?} input_pts_present_mid={:?} input_pts_last={:?}",
        video.info.samples.len(),
        video.info.width,
        video.info.height,
        video.info.average_framerate().unwrap_or(0.0),
        audio_samples,
        pts.first().copied(),
        pts.get(mid).copied(),
        present.get(mid).copied(),
        pts.last().copied(),
    );

    let pipeline_only = std::env::var_os("HONGGUO_BVC2_PIPELINE_ONLY").is_some();
    if pipeline_only {
        println!("decode_only_ns=skipped");
    } else {
        let mut frames = 0usize;
        let mut checksum = 0u64;
        let mut first_frame_ns = 0u128;
        let decode_started = Instant::now();
        let source = hongguo_bytevc2::open_plaintext(input_path).expect("打开 ByteVC2");
        source
            .decode(|frame| {
                if frames == 0 {
                    first_frame_ns = decode_started.elapsed().as_nanos();
                }
                frames += 1;
                checksum = checksum.wrapping_add(u64::from(frame.y.first().copied().unwrap_or(0)));
                checksum = checksum.wrapping_add(frame.width as u64);
                Ok(())
            })
            .expect("解码");
        println!(
            "decode_only_ns={} decode_first_frame_ns={} decode_frames={} checksum={}",
            decode_started.elapsed().as_nanos(),
            first_frame_ns,
            frames,
            checksum
        );
    }

    unsafe { std::env::set_var("HONGGUO_BVC2_STAGE_LOG", &stage) };
    let full_started = Instant::now();
    let output_path = Path::new(&output).to_path_buf();
    let on_prefix = |path: &Path| {
        println!(
            "prefix_ready_ns={} prefix_path={} screen_first_frame=unverified",
            full_started.elapsed().as_nanos(),
            path.display()
        );
    };
    transcode_bytevc2(&PlatformRequest {
        input: input_path,
        output: Path::new(&output),
        scale_to: None,
        rate: RateProfile::Playback,
        on_progress: None,
        on_playable_prefix: Some(&on_prefix),
        on_committed: None,
        is_cancelled: None,
    })
    .expect("原分辨率转码");
    println!("full_wall_ns={}", full_started.elapsed().as_nanos());

    let video_pts =
        hongguo_media::mux::read_fragment_video(&output_path).expect("产物应有视频片段");
    let mid = video_pts.pts.len() / 2;
    println!(
        "output_samples={} output_pts_first={:?} output_pts_mid={:?} output_pts_last={:?} output_fragments={} output_first_sync={}",
        video_pts.pts.len(),
        video_pts.pts.first().copied(),
        video_pts.pts.get(mid).copied(),
        video_pts.pts.last().copied(),
        video_pts.fragment_bases.len(),
        video_pts.first_is_sync,
    );
    println!("screen_first_frame=unverified");
}
