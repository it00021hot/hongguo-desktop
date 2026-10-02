//! MP4 样本表解析的单元测试。
//!
//! 以 `#[path]` 挂到 `sample_table.rs` 下，`use super::*` 直接拿实现里的
//! 私有函数与类型。

use super::*;
use crate::domain::mp4::fixtures::{full_box, mp4, TrackSpec};

#[test]
fn reads_stsz_variable_sizes() {
    // stsz 载荷：sample_size(4)=0(变长) | count(4)=3 | sizes[3]
    let mut payload = 0u32.to_be_bytes().to_vec();
    payload.extend_from_slice(&3u32.to_be_bytes()); // count
    payload.extend_from_slice(&100u32.to_be_bytes());
    payload.extend_from_slice(&200u32.to_be_bytes());
    payload.extend_from_slice(&300u32.to_be_bytes());
    let stsz = full_box(b"stsz", &payload);

    let mut info = TrackInfo::default();
    read_stsz(&stsz, 8, stsz.len() - 8, &mut info);
    assert_eq!(info.sample_count, 3);
    assert_eq!(info.samples.len(), 3);
    assert_eq!(info.samples[1].1, 200);
}

#[test]
fn reads_stsz_constant_size() {
    // sample_size(4)=256 | count(4)=4
    let mut payload = 256u32.to_be_bytes().to_vec();
    payload.extend_from_slice(&4u32.to_be_bytes());
    let stsz = full_box(b"stsz", &payload);

    let mut info = TrackInfo::default();
    read_stsz(&stsz, 8, stsz.len() - 8, &mut info);
    assert_eq!(info.sample_count, 4);
    assert!(info.samples.iter().all(|(_, s)| *s == 256));
}

#[test]
fn reads_video_codec_parameters_from_stsd() {
    // 走完整解析路径：编码参数要真的从 sample entry 里读出来，
    // 同样两个 hvc1，1080p 与 720p 拼在一起是坏文件
    let data = mp4(&[TrackSpec::video(1, b"hvc1", 1920, 1080)]);
    let tracks = collect_tracks(&data).expect("应能解析出轨道");

    assert_eq!(tracks.len(), 1);
    let t = &tracks[0];
    assert_eq!(t.codec, "hvc1");
    assert_eq!(t.width, 1920);
    assert_eq!(t.height, 1080);
    assert_eq!(t.channels, 0, "视频轨不该带出音频字段");
    assert_eq!(t.sample_rate, 0);
}

#[test]
fn reads_audio_parameters_from_stsd() {
    let data = mp4(&[
        TrackSpec::video(1, b"hvc1", 1920, 1080),
        TrackSpec::audio(2, b"mp4a", 2, 44100),
    ]);
    let tracks = collect_tracks(&data).expect("应能解析出轨道");

    assert_eq!(tracks.len(), 2);
    let audio = &tracks[1];
    assert_eq!(audio.codec, "mp4a");
    assert_eq!(audio.channels, 2);
    assert_eq!(audio.sample_rate, 44100);
    assert_eq!(audio.width, 0, "音频轨不该带出视频字段");
    assert_eq!(audio.height, 0);
}

#[test]
fn reads_video_parameters_beside_a_second_video_track() {
    // 两段都是视频：不能因为「有两条轨」就串位读到后一条的字段
    let data = mp4(&[
        TrackSpec::video(1, b"hvc1", 1920, 1080),
        TrackSpec::video(2, b"avc1", 1280, 720),
    ]);
    let tracks = collect_tracks(&data).expect("应能解析出轨道");

    assert_eq!(
        tracks
            .iter()
            .map(|t| (t.codec.as_str(), t.width, t.height))
            .collect::<Vec<_>>(),
        vec![("hvc1", 1920, 1080), ("avc1", 1280, 720)]
    );
}

#[test]
fn truncated_stsd_leaves_parameters_at_zero_without_panicking() {
    // entry 的前 24 字节（reserved / data_reference_index / pre_defined）在，
    // width 与 height 被截断
    let mut payload = 1u32.to_be_bytes().to_vec(); // entry_count
    payload.extend_from_slice(&24u32.to_be_bytes()); // entry size
    payload.extend_from_slice(b"hvc1"); // format
    payload.extend_from_slice(&[0u8; 24]); // entry 载荷里 width 之前的部分
    let stsd = full_box(b"stsd", &payload);

    let mut info = TrackInfo {
        is_video: true,
        ..Default::default()
    };
    read_stsd(&stsd, 8, stsd.len() - 8, &mut info);

    assert_eq!(info.codec, "hvc1");
    assert_eq!(info.width, 0, "越界的字段留 0，不该 panic");
    assert_eq!(info.height, 0);
}

#[test]
fn truncated_audio_stsd_leaves_sample_rate_at_zero() {
    // channelcount 在，samplerate 整段被截断
    let mut payload = 1u32.to_be_bytes().to_vec(); // entry_count
    payload.extend_from_slice(&24u32.to_be_bytes()); // entry size
    payload.extend_from_slice(b"mp4a"); // format
    payload.extend_from_slice(&[0u8; 16]); // entry 载荷里 channelcount 之前的部分
    payload.extend_from_slice(&2u16.to_be_bytes()); // channelcount
    let stsd = full_box(b"stsd", &payload);

    let mut info = TrackInfo::default();
    read_stsd(&stsd, 8, stsd.len() - 8, &mut info);

    assert_eq!(info.codec, "mp4a");
    assert_eq!(info.channels, 2);
    assert_eq!(info.sample_rate, 0, "越界的字段留 0，不该 panic");
}

#[test]
fn stsd_shorter_than_the_entry_header_is_ignored() {
    let mut info = TrackInfo::default();
    read_stsd(&[0u8; 12], 8, 4, &mut info);
    assert_eq!(info.codec, "", "连四字符码都读不到时不该编造编码");
}

#[test]
fn single_chunk_offsets_advance_by_sample_size() {
    // stsz 变长 3 个样本 100/200/300
    let mut payload = 0u32.to_be_bytes().to_vec();
    payload.extend_from_slice(&3u32.to_be_bytes());
    for size in [100u32, 200, 300] {
        payload.extend_from_slice(&size.to_be_bytes());
    }
    let stsz = full_box(b"stsz", &payload);

    let mut info = TrackInfo::default();
    read_stsz(&stsz, 8, stsz.len() - 8, &mut info);

    // stco 单 chunk：entry_count(4)=1 | offset(4)=1024
    let mut stco_payload = 1u32.to_be_bytes().to_vec();
    stco_payload.extend_from_slice(&1024u32.to_be_bytes());
    let stco = full_box(b"stco", &stco_payload);
    info.wide_offsets = false;
    read_chunk_offsets(&stco, 8, stco.len() - 8, &mut info);

    // 单 chunk 场景下样本偏移按各样本自身大小顺序累加
    assert_eq!(info.samples, vec![(1024, 100), (1124, 200), (1324, 300)]);
    assert_eq!(info.samples.iter().map(|(_, size)| size).sum::<u64>(), 600);
}
