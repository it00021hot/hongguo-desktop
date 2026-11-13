//! Apple 的原生解析器会把初始媒体时长加到 fragment 时长上；只检查 ffprobe 会漏掉翻倍。
#![cfg(target_os = "macos")]

use std::process::Command;

use hongguo_media::mux::{FragmentTiming, GrowingH264};

/// 16×16 灰色 IDR，离线用 libx264（10fps、keyint=1）生成；去掉与解码无关的 SEI。
const GREY_IDR: &[u8] = &[
    0, 0, 0, 1, 0x67, 0x42, 0xc0, 0x0a, 0xdd, 0xec, 0x04, 0x40, 0, 0, 3, 0, 0x40, 0, 0, 5, 0x23,
    0xc4, 0x89, 0xe0, 0, 0, 0, 1, 0x68, 0xce, 0x0f, 0xc8, 0, 0, 1, 0x65, 0x88, 0x84, 0x3a, 0x26,
    0x28, 0x0e,
];

#[test]
fn apple_reads_fragmented_movie_without_doubling_its_duration() {
    let dir =
        std::env::temp_dir().join(format!("hongguo-fragment-duration-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建独立测试目录");
    let path = dir.join("one-second.mp4");
    let frames: Vec<_> = (0..10)
        .map(|index| (f64::from(index) / 10.0, GREY_IDR.to_vec(), true))
        .collect();
    let mut movie = GrowingH264::create(&path, 16, 16, None, 1.0).expect("创建渐进文件");
    movie
        .push(
            &frames[..3],
            &[],
            FragmentTiming {
                video_end_secs: 0.3,
                audio_end_secs: None,
            },
        )
        .expect("发布首段");
    movie
        .push(
            &frames[3..],
            &[],
            FragmentTiming {
                video_end_secs: 1.0,
                audio_end_secs: None,
            },
        )
        .expect("发布后续片段");
    drop(movie);

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
