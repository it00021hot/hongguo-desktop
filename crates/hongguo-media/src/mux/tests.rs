//! 封装器的单元测试。
//!
//! 测的是「写出来的 MP4 能不能被自己的 demuxer 读回去」：轨道描述符、样本
//! 数、样本边界逐字节、`ctts` 的合成偏移。解码器级的验证（真码流能被
//! ffprobe / 平台解码器认）由真实码流的 e2e 承担。

use super::*;

/// 临时目录（按进程号隔离，避免并行用例互相踩）。
fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hg-mux-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 造一帧 Annex-B 数据。
fn frame(nals: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for nal in nals {
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(nal);
    }
    out
}

/// 同一份 NAL 列表的 Annex-B 与 AVCC 两种形态。
fn pair(nals: &[&[u8]]) -> (Vec<u8>, Vec<u8>) {
    let annexb = frame(nals);
    let mut avcc = Vec::new();
    for nal in nals {
        avcc.extend_from_slice(&(nal.len() as u32).to_be_bytes());
        avcc.extend_from_slice(nal);
    }
    (annexb, avcc)
}

/// 带 SPS/PPS 的 IDR 帧。
fn idr_frame() -> (Vec<u8>, Vec<u8>) {
    pair(&[&[0x65, 0x88], &[0x67, 0x64, 0x00, 0x1f], &[0x68, 0xeb]])
}

/// 普通帧。
fn plain_frame(i: u8) -> (Vec<u8>, Vec<u8>) {
    pair(&[&[0x41, 0x55, i]])
}

/// 一条假音轨（`esds` 只是有辨识度的字节，封装器原样搬运）。
fn audio_track(samples: Vec<(f64, Vec<u8>)>) -> AudioTrack {
    let mut esds = vec![0u8; 4];
    esds.extend_from_slice(&(20u32).to_be_bytes());
    esds.extend_from_slice(b"esds");
    esds.extend_from_slice(&[0xAA; 12]);
    AudioTrack {
        sample_rate: 44100,
        channels: 2,
        esds: Some(esds),
        samples,
    }
}

/// 产物必须真能解复用，且轨道描述符写成 avc1。
///
/// 封装器不解析 H.264 语法，哑 NAL 即可：写进去的样本要能原样读回来。
/// 解码器级的验证由真实码流的 e2e（ffprobe）承担。
#[test]
fn muxed_output_is_a_playable_avc1_mp4() {
    let dir = temp_dir("avc1");
    let out = dir.join("out.mp4");

    // 3 帧：首帧是带 SPS(7)/PPS(8) 的 IDR，其余是普通帧
    let mut frames: Vec<(f64, Vec<u8>, bool)> = Vec::new();
    let (first, _) = idr_frame();
    frames.push((0.0, first, true));
    for i in 1..3u8 {
        let (data, _) = plain_frame(i);
        frames.push((f64::from(i) / 25.0, data, false));
    }

    let size = mux_h264(&out, &frames, 192, 108, None).expect("封装应成功");
    assert!(size > 0);
    assert!(out.exists());

    let demuxed = crate::demux::demux_file(&out).expect("产物应可解复用");
    let video = demuxed.video_track().expect("应有视频轨");
    assert_eq!(video.info.codec, "avc1", "H.264 的 fourcc 应为 avc1");
    assert_eq!(video.info.samples.len(), frames.len(), "样本数应与写入一致");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 没有帧时不产出文件，也不该 panic。
#[test]
fn empty_frames_are_rejected() {
    let dir = temp_dir("empty");
    let out = dir.join("empty.mp4");
    assert!(mux_h264(&out, &[], 64, 64, None).is_err());
    assert!(!out.exists(), "失败时不产出文件");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 首帧不是关键帧时拒收：产物没有同步起点，播放器起不来播。
#[test]
fn a_stream_without_a_leading_keyframe_is_rejected() {
    let dir = temp_dir("nokey");
    let out = dir.join("nokey.mp4");
    let (data, _) = plain_frame(1);
    assert!(mux_h264(&out, &[(0.0, data, false)], 64, 64, None).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// B 帧流（解码序 + 非单调 PTS）封装回归。
///
/// 曾整段翻车两次——faststart 搬移假设写入序≈pts 序（样本边界错位），
/// 以及 PTS 透传撞上「写入序严格 PTS 递增」的校验。封装器不解析 H.264
/// 语法，哑 NAL 即可做字节级核对：解复用回来的每个样本必须与写入帧的
/// AVCC 形态逐字节一致，且样本序=写入序（解码序）。哑帧不可解码，
/// 解码器级的验证由真实码流的 e2e（ffprobe）承担。
#[test]
fn bframe_decode_order_stream_survives_the_mux() {
    // 解码序 + 非单调 PTS（I P B B 型 GOP 重复几轮）
    let display = [0.0f64, 0.3, 0.1, 0.2, 0.6, 0.4, 0.5, 0.9, 0.7, 0.8];
    let mut frames: Vec<(f64, Vec<u8>, bool)> = Vec::new();
    let mut expected: Vec<Vec<u8>> = Vec::new();
    for (i, &t) in display.iter().enumerate() {
        let (annexb, avcc) = if i == 0 {
            idr_frame()
        } else {
            plain_frame(i as u8)
        };
        frames.push((t, annexb, i == 0));
        expected.push(avcc);
    }

    let dir = temp_dir("bframe");
    let out = dir.join("bframe.mp4");
    let size = mux_h264(&out, &frames, 192, 108, None).expect("B 帧流封装应成功");
    assert!(size > 0);

    let demuxed = crate::demux::demux_file(&out).expect("产物应能解复用");
    let video = demuxed.video_track().expect("产物应有视频轨");
    assert_eq!(
        video.info.samples.len(),
        frames.len(),
        "样本数必须与写入帧数一致"
    );
    let data = std::fs::read(&out).unwrap();
    for (i, &(at, len)) in video.info.samples.iter().enumerate() {
        let got = &data[at as usize..(at + len) as usize];
        assert_eq!(
            got, &expected[i],
            "第 {i} 个样本字节与写入帧（AVCC 形态）不一致——样本边界错位"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// B 帧的合成偏移要落在 `ctts` 里，且带符号（version 1）。
///
/// 显示序 I B B P 对应的解码序是 I P B B——`stts` 描述解码时间轴，
/// `ctts` 补上「显示时间 − 解码时间」，两者合起来才是显示时间。
#[test]
fn the_ctts_table_carries_the_composition_offsets() {
    let dir = temp_dir("ctts");
    let out = dir.join("ctts.mp4");
    // 解码序：I(0) P(3) B(1) B(2)，单位 1/25 秒
    let display = [0.0f64, 0.12, 0.04, 0.08];
    let mut frames: Vec<(f64, Vec<u8>, bool)> = Vec::new();
    for (i, &t) in display.iter().enumerate() {
        let (data, _) = if i == 0 {
            idr_frame()
        } else {
            plain_frame(i as u8)
        };
        frames.push((t, data, i == 0));
    }
    mux_h264(&out, &frames, 192, 108, None).expect("封装应成功");

    let demuxed = crate::demux::demux_file(&out).expect("产物应可解复用");
    let video = demuxed.video_track().expect("应有视频轨");
    let offsets: Vec<i32> = video
        .info
        .ctts
        .iter()
        .flat_map(|&(n, o)| std::iter::repeat_n(o, n as usize))
        .collect();
    // 解码时间取「显示时间的升序」：0, 0.04, 0.08, 0.12
    // 合成偏移 = 显示 − 解码：0, +0.08, 0, −0.04（秒）
    let expect: Vec<i32> = display
        .iter()
        .zip([0.0f64, 0.04, 0.08, 0.12])
        .map(|(&p, d)| (((p - d) * 25.0).round() as i32) * 3600)
        .collect();
    assert_eq!(offsets, expect, "ctts 里的合成偏移不对");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 早于首帧视频的音频要裁掉：容器没有 edit list，那段音频会让播放器
/// 在画面出来之前先播一段没画面的声音。
#[test]
fn audio_before_the_first_video_frame_is_dropped() {
    let dir = temp_dir("trim");
    let out = dir.join("trim.mp4");
    let mut frames: Vec<(f64, Vec<u8>, bool)> = Vec::new();
    let (first, _) = idr_frame();
    frames.push((0.2, first, true)); // 首帧视频在 0.2s
    let (second, _) = plain_frame(1);
    frames.push((0.24, second, false));

    // 音频从 0s 开始，每帧 1024/44100 秒
    let step = 1024.0 / 44100.0;
    let audio: Vec<(f64, Vec<u8>)> = (0..40)
        .map(|i| (i as f64 * step, vec![0xAAu8; 64]))
        .collect();
    let track = audio_track(audio);

    mux_h264(&out, &frames, 192, 108, Some(&track)).expect("封装应成功");

    let demuxed = crate::demux::demux_file(&out).expect("产物应可解复用");
    let audio_track_info = demuxed.audio_track().expect("应有音轨");
    // 0.2s 之前（约 8 帧）被裁掉
    assert!(
        audio_track_info.info.samples.len() < 40,
        "早于首帧视频的音频应被裁掉，实际 {} 帧",
        audio_track_info.info.samples.len()
    );
    assert!(audio_track_info.info.samples.len() > 30, "不该裁过头");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 源音轨的 `esds` 要原样搬进产物——解码器初始化参数不能自己猜。
#[test]
fn the_source_esds_is_carried_over() {
    let dir = temp_dir("esds");
    let out = dir.join("esds.mp4");
    let mut frames: Vec<(f64, Vec<u8>, bool)> = Vec::new();
    let (first, _) = idr_frame();
    frames.push((0.0, first, true));
    let track = audio_track(vec![(0.0, vec![0xAAu8; 64])]);
    mux_h264(&out, &frames, 192, 108, Some(&track)).expect("封装应成功");

    let data = std::fs::read(&out).unwrap();
    assert!(data.windows(4).any(|w| w == b"esds"), "产物里应有 esds box");
    let demuxed = crate::demux::demux_file(&out).expect("产物应可解复用");
    let audio = demuxed.audio_track().expect("应有音轨");
    assert_eq!(audio.info.channels, 2, "声道数应从源搬过来");
    assert_eq!(audio.info.sample_rate, 44100, "采样率应从源搬过来");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 真实规模复现：3000 帧 × ~4KB + AAC 音频交织——小样本全过但真机
/// 产物在 ~302 帧处样本边界炸开，规模/交织相关就靠这条钉住。
#[test]
fn bframe_stream_at_realistic_scale_with_audio() {
    let n = 3000usize;
    let mut frames: Vec<(f64, Vec<u8>, bool)> = Vec::with_capacity(n);
    let mut expected: Vec<Vec<u8>> = Vec::with_capacity(n);
    for i in 0..n {
        // 解码序 I P B B：组内显示序 I B B P
        let gop = i % 4;
        let pts = match gop {
            0 => (i as f64) / 30.0,
            1 => (i as f64 + 2.0) / 30.0,
            2 | 3 => (i as f64 - 1.0) / 30.0,
            _ => unreachable!(),
        };
        let (annexb, avcc) = if gop == 0 {
            // 关键帧中流内前插 SPS/PPS
            pair(&[
                &[0x65][..],
                &[0x67, 0x64, 0x00, 0x1f][..],
                &[0x68, 0xAA][..],
                &vec![0u8; 4000][..],
            ])
        } else {
            pair(&[&[0x41, (i % 251) as u8]])
        };
        frames.push((pts, annexb, gop == 0));
        expected.push(avcc);
    }
    let audio: Vec<(f64, Vec<u8>)> = (0..n * 2)
        .map(|i| (i as f64 / 44100.0 * 1024.0, vec![0xAAu8; 400]))
        .collect();
    let dir = temp_dir("scale");
    let out = dir.join("scale.mp4");
    let track = audio_track(audio);
    mux_h264(&out, &frames, 192, 108, Some(&track)).expect("规模封装应成功");

    let demuxed = crate::demux::demux_file(&out).expect("产物应可解复用");
    let video = demuxed.video_track().expect("应有视频轨");
    assert_eq!(video.info.samples.len(), n, "样本数应一致");
    let data = std::fs::read(&out).unwrap();
    for (i, &(at, len)) in video.info.samples.iter().enumerate() {
        let got = &data[at as usize..(at + len) as usize];
        assert_eq!(got, &expected[i], "第 {i} 个样本边界错位");
    }

    let _ = std::fs::remove_dir_all(&dir);
}
