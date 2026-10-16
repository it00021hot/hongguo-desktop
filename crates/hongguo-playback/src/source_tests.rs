//! 本地成品源的字节层断言：源抽象 [`ByteSource`](crate::source::ByteSource)
//! 在「按样本表取字节」这件事上必须与在线流同构——本地路径读错位、长度算
//! 错，解码端只会表现为「画面花/解不出」，线上很难归因，所以在这里钉死。
//!
//! 样本 MP4 用 hongguo-video 的 fixtures 现造（dev-dependency 开
//! test-support）：造出来的文件只含编码信息与样本字节，正好验「按样本表读回
//! 原字节 + 边界与错误」。
//!
//! 头部解析（`open_stream`）不在这里断言：fixtures 的 box 嵌套是给
//! hongguo-video 自己的解析器用的（没有 minf 中间层），`hongguo_media::demux`
//! 在这份合成样本上读不出 stsd 字段——那条路径与在线流完全同一段代码，
//! 真实 MP4 的端到端验证放在真机（`HONGGUO_SHOT_PANEL=local-play` 探针，
//! 用 ffmpeg 现造样本）。

use std::io::Write as _;

use hongguo_video::fixtures::{TrackPlan, mp4_with_samples};

use crate::source::ByteSource;

/// 造一个两轨小 MP4（fixtures 固定写视频 1920×1080 / 音频 2ch 44.1kHz）。
fn fixture_mp4() -> (Vec<u8>, Vec<Vec<u8>>) {
    let video_samples: Vec<Vec<u8>> = (0..6).map(|i| vec![0xA0 | i as u8; 32 + i]).collect();
    let audio_samples: Vec<Vec<u8>> = (0..4).map(|i| vec![0xB0 | i as u8; 24]).collect();
    let bytes = mp4_with_samples(&[
        TrackPlan::video(video_samples.clone()),
        TrackPlan::audio(audio_samples),
    ]);
    (bytes, video_samples)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("测试运行时")
}

fn write_fixture(tag: &str, bytes: &[u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hg-playback-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("临时目录");
    let path = dir.join("fixture.mp4");
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(bytes))
        .expect("写样本文件");
    path
}

/// 本地源：总长、首读对齐、按偏移读回原字节、越过文件尾返回空。
#[test]
fn local_source_reads_matching_bytes() {
    let (bytes, video_samples) = fixture_mp4();
    let path = write_fixture("local", &bytes);
    let rt = runtime();
    rt.block_on(async {
        let source = ByteSource::local(&path).expect("本地源");
        // 总长 = 文件长度（在线源走 Content-Range，本地走 metadata）
        assert_eq!(source.total().await.expect("总长"), bytes.len() as u64);
        // 开头 16 字节就是文件开头（ftyp）：读偏移错位的话首帧解析必崩
        assert_eq!(
            source.read(0, 15, || false).await.expect("读头部"),
            bytes[..16],
            "本地读必须与文件逐字节对齐"
        );

        // 按文件内偏移读回首个视频样本的字节，必须逐字节一致——这就是
        // 「样本表偏移 → 本地文件字节」这条链的全部契约
        let first = video_samples.first().expect("样本");
        let start = bytes
            .windows(first.len())
            .position(|w| w == first.as_slice())
            .expect("样本可定位") as u64;
        let got = source
            .read(start, start + first.len() as u64 - 1, || false)
            .await
            .expect("读样本");
        assert_eq!(got, *first, "读回字节必须与写盘的一致");
    });
}

/// 越过文件尾的读返回空（调用方按样本表读，越界不该报错也不该 panic）。
#[test]
fn local_source_read_past_eof_is_empty() {
    let (bytes, _) = fixture_mp4();
    let path = write_fixture("eof", &bytes);
    let rt = runtime();
    rt.block_on(async {
        let source = ByteSource::local(&path).expect("本地源");
        let len = bytes.len() as u64;
        let got = source
            .read(len + 8, len + 32, || false)
            .await
            .expect("越界读不报错");
        assert!(got.is_empty());
    });
}

/// 文件不在 = 构造失败：调用方据此回落在在线流（Tauri 本地优先失败回落同款）。
#[test]
fn local_source_missing_file_is_error() {
    let missing = std::env::temp_dir().join("hg-playback-not-there.mp4");
    assert!(ByteSource::local(&missing).is_err());
}
