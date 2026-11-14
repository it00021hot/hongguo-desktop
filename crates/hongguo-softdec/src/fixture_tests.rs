//! 用 `tests/fixtures/` 里的小样本把「索引 → 跳转 → 解码 → 出帧」整条链跑通。
//!
//! 这些样本是本机 x265 / x264 编的**标准码流**，不是真实剧集，更不是 ByteVC1：
//! 它们证明的是容器解析、时间轴、跳转与打包逻辑正确，以及解核对标准 HEVC 逐字节正确。
//! 「真实 ByteVC1 能否解对」不在这里的证明范围内，要靠真实样本对拍。

use hongguo_common::{AppError, AppResult};

use crate::{DecodedFrame, IndexedVideo, VideoCodec};

const HEVC_MAIN: &[u8] = include_bytes!("../tests/fixtures/hevc-main-136x76-aac.mp4");
const HEVC_MAIN10_HEV1: &[u8] = include_bytes!("../tests/fixtures/hevc-main10-128x72-hev1-aac.mp4");
const HEVC_NO_AUDIO: &[u8] = include_bytes!("../tests/fixtures/hevc-main-64x48-no-audio.mp4");
const H264: &[u8] = include_bytes!("../tests/fixtures/h264-64x48-aac.mp4");

/// ffmpeg 7.1.1 原生 hevc 解码器对同一文件解出的全部帧（显示序，Y、U、V 依次拼接）的 FNV-1a 64。
/// HEVC 解码是规范定死的逐比特过程，两个合规解码器的输出必须一致。
const HEVC_MAIN_FNV: u64 = 0xe3b2_ac82_0a17_93b7;
/// 同上，10-bit 源：ffmpeg 解出 `yuv420p10le` 后每个样本右移 2 位（与本 crate 的截断口径一致）。
const HEVC_MAIN10_FNV: u64 = 0xea2f_6d62_8039_e6c6;

const FRAME_SECS: f64 = 1.0 / 24.0;
const TOLERANCE: f64 = 0.000_5;

fn read_from(data: &[u8]) -> impl FnMut(u64, u64, &mut Vec<u8>) -> AppResult<()> + '_ {
    |offset, length, bytes| {
        let start = offset as usize;
        let chunk = data
            .get(start..start + length as usize)
            .ok_or_else(|| AppError::Media("读取越过样本文件末尾".into()))?;
        bytes.clear();
        bytes.extend_from_slice(chunk);
        Ok(())
    }
}

fn open(data: &[u8]) -> AppResult<IndexedVideo> {
    IndexedVideo::open(data.len() as u64, read_from(data))
}

fn fnv(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

fn frame_fnv(hash: u64, frame: &DecodedFrame) -> u64 {
    fnv(fnv(fnv(hash, &frame.y), &frame.u), &frame.v)
}

/// 一帧的摘要：显示时间、尺寸、单帧像素指纹。
struct Seen {
    pts: f64,
    size: (usize, usize),
    pixels: u64,
}

/// 从 `target` 起解到尾，返回每帧摘要和全部帧的连续指纹。
fn decode(data: &[u8], target: f64) -> (Vec<Seen>, u64) {
    let index = open(data).expect("建索引");
    let mut seen = Vec::new();
    let mut all = FNV_OFFSET;
    index
        .decode_from(target, read_from(data), |frame| {
            assert_eq!(frame.y.len(), frame.width * frame.height, "亮度平面应紧密");
            assert_eq!(frame.u.len(), frame.width / 2 * frame.height / 2);
            assert_eq!(frame.v.len(), frame.u.len());
            all = frame_fnv(all, &frame);
            seen.push(Seen {
                pts: frame.pts.expect("每帧都应带显示时间"),
                size: (frame.width, frame.height),
                pixels: frame_fnv(FNV_OFFSET, &frame),
            });
            Ok(())
        })
        .expect("解码");
    (seen, all)
}

#[test]
fn hevc_main_decodes_every_frame_bit_exact_in_display_order() {
    let index = open(HEVC_MAIN).expect("建索引");
    assert_eq!(index.codec(), VideoCodec::Hevc);

    let (seen, all) = decode(HEVC_MAIN, 0.0);
    assert_eq!(seen.len(), 48, "样本表有 48 个样本，B 帧重排不能丢帧");
    // 136x76：高度不是最小编码块的整数倍，走到了裁剪窗。
    assert!(seen.iter().all(|frame| frame.size == (136, 76)));
    for (number, frame) in seen.iter().enumerate() {
        let expect = number as f64 * FRAME_SECS;
        assert!(
            (frame.pts - expect).abs() < TOLERANCE,
            "第 {number} 帧显示时间 {} 应为 {expect}（码流里有 B 帧，回调必须是显示序）",
            frame.pts
        );
    }
    assert_eq!(all, HEVC_MAIN_FNV, "像素应与 ffmpeg 的解码结果逐字节一致");
}

#[test]
fn hevc_main10_in_hev1_is_truncated_to_8_bit() {
    // 这份样本同时覆盖：`hev1` 四字码、10-bit、`moov` 在文件头。
    let (seen, all) = decode(HEVC_MAIN10_HEV1, 0.0);
    assert_eq!(seen.len(), 24);
    assert!(seen.iter().all(|frame| frame.size == (128, 72)));
    assert_eq!(
        all, HEVC_MAIN10_FNV,
        "应等于 ffmpeg 的 10-bit 输出逐样本右移 2 位"
    );
}

#[test]
fn seeking_starts_at_the_previous_keyframe_and_hides_earlier_frames() {
    let (full, _) = decode(HEVC_MAIN, 0.0);
    // 关键帧每 12 帧一个（0 / 0.5 / 1.0 / 1.5 秒）。1.3 秒落在第 31、32 帧之间。
    let (tail, _) = decode(HEVC_MAIN, 1.3);
    assert_eq!(tail.len(), 16, "应回调第 32..48 帧");
    assert!(
        (tail[0].pts - 32.0 * FRAME_SECS).abs() < TOLERANCE,
        "首帧 {} 应是不早于目标的第一帧",
        tail[0].pts
    );
    // 从中途关键帧起解出来的帧，必须和从头解出来的同一帧一模一样。
    for (got, expect) in tail.iter().zip(&full[32..]) {
        assert!((got.pts - expect.pts).abs() < TOLERANCE);
        assert_eq!(
            got.pixels, expect.pixels,
            "跳转后 {} 秒处画面不一致",
            got.pts
        );
    }
}

#[test]
fn seeking_exactly_onto_a_keyframe_emits_it_first() {
    let (tail, _) = decode(HEVC_MAIN, 1.0);
    assert_eq!(tail.len(), 24);
    assert!(
        (tail[0].pts - 1.0).abs() < TOLERANCE,
        "实际 {}",
        tail[0].pts
    );
}

#[test]
fn seeking_past_the_end_clamps_to_the_last_frame() {
    let (tail, _) = decode(HEVC_MAIN, 99.0);
    assert_eq!(tail.len(), 1);
    assert!((tail[0].pts - 47.0 * FRAME_SECS).abs() < TOLERANCE);
}

#[test]
fn a_failing_frame_callback_stops_decoding_and_surfaces_its_error() {
    let index = open(HEVC_MAIN).expect("建索引");
    let mut frames = 0;
    let stopped = index.decode_from(0.0, read_from(HEVC_MAIN), |_| {
        frames += 1;
        if frames == 3 {
            return Err(AppError::Cancelled);
        }
        Ok(())
    });
    assert!(
        matches!(stopped, Err(AppError::Cancelled)),
        "实际 {stopped:?}"
    );
    assert_eq!(frames, 3, "取消后不应再回调");
}

#[test]
fn a_failing_read_surfaces_its_error() {
    let index = open(HEVC_MAIN).expect("建索引");
    let failed = index.decode_from(
        0.0,
        |_, _, _| Err(AppError::Network("断网".into())),
        |_| Ok(()),
    );
    assert!(
        matches!(failed, Err(AppError::Network(_))),
        "实际 {failed:?}"
    );
}

#[test]
fn h264_is_refused_by_name_instead_of_being_handed_elsewhere() {
    let error = open(H264).err().expect("H.264 不在内置解码器范围内");
    assert!(
        error.to_string().contains("avc1"),
        "错误里应写明实际编码，实际 {error}"
    );
}

#[test]
fn a_file_without_an_aac_track_is_refused() {
    let error = open(HEVC_NO_AUDIO).err().expect("当前直播放以音轨为时钟");
    assert!(error.to_string().contains("AAC"), "实际 {error}");
}
