//! Apple 的原生解析器会把初始媒体时长加到 fragment 时长上；只检查 ffprobe 会漏掉翻倍。
#![cfg(target_os = "macos")]

use std::process::Command;

use hongguo_media::mux::{ArchiveHevc, ArchiveTiming};

#[test]
fn apple_reads_fragmented_movie_without_doubling_its_duration() {
    let dir =
        std::env::temp_dir().join(format!("hongguo-fragment-duration-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建独立测试目录");
    let path = dir.join("one-second.mp4");
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../hongguo-softdec/tests/fixtures/hevc-main-64x48-no-audio.mp4");
    let demux = hongguo_media::demux::demux_file(&source).unwrap();
    let video = &demux.video_track().unwrap().info;
    let (sets, length) = hongguo_media::hevc::read_parameter_set_nalus(&source, video).unwrap();
    let mut first = Vec::new();
    for set in sets {
        first.extend([0, 0, 0, 1]);
        first.extend(set);
    }
    let (offset, size) = video.samples[0];
    let raw = hongguo_media::transcode::read_range(&source, offset, size).unwrap();
    first.extend(hongguo_media::hevc::to_annexb(&raw, length));
    let frames: Vec<_> = (0..10)
        .map(|index| (f64::from(index) / 10.0, first.clone(), true))
        .collect();
    let mut movie = ArchiveHevc::create(&path, 64, 48, None, 1.0).expect("创建分片文件");
    movie
        .push(
            &frames[..3],
            &[],
            ArchiveTiming {
                video_end_secs: 0.3,
                audio_end_secs: None,
            },
        )
        .expect("发布首段");
    movie
        .push(
            &frames[3..],
            &[],
            ArchiveTiming {
                video_end_secs: 1.0,
                audio_end_secs: None,
            },
        )
        .expect("发布后续片段");
    movie.finish().expect("封存归档 MP4");

    let probe = Command::new("/usr/bin/swift")
        .args([
            "-e",
            "import AVFoundation; let asset = AVURLAsset(url: URL(fileURLWithPath: CommandLine.arguments[1])); print(try await asset.load(.duration).seconds)",
        ])
        .arg(&path)
        .output()
        .expect("运行系统 AVFoundation 时长检查");
    std::fs::remove_dir_all(&dir).expect("清理测试产物");
    assert!(
        probe.status.success(),
        "AVFoundation 解析失败：{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    let duration: f64 = String::from_utf8(probe.stdout)
        .expect("原生检查输出 UTF-8")
        .trim()
        .parse()
        .expect("原生检查返回秒数");
    assert!(
        (duration - 1.0).abs() < 0.002,
        "1 秒片段电影被 Apple 解析为 {duration} 秒"
    );
}
