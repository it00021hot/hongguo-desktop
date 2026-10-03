//! `timing` 的单元测试。
//!
//! 四张表都是「定长字段 + 变长条目」的同形结构，测点集中在两处容易错的地方：
//! **条目的符号**（ctts 的 version 决定）和**字段偏移**（mdhd/tkhd 的 version 1
//! 把定长字段从 4 字节换成 8 字节）。

use super::*;
use crate::domain::mp4::r#box::build_box;

/// FullBox：`version` 单独给，flags 恒为 0。
fn full(kind: &[u8; 4], version: u8, payload: &[u8]) -> Vec<u8> {
    let mut body = vec![version, 0, 0, 0];
    body.extend_from_slice(payload);
    build_box(kind, &body)
}

fn entry_count_and_entries(entries: &[u8]) -> Vec<u8> {
    let mut v = (entries.len() as u32 / 8).to_be_bytes().to_vec();
    v.extend_from_slice(entries);
    v
}

fn info_with_samples(n: usize) -> TrackInfo {
    TrackInfo {
        samples: vec![(0, 1); n],
        ..Default::default()
    }
}

#[test]
fn stts_reads_run_length_entries() {
    let mut entries = Vec::new();
    entries.extend(1u32.to_be_bytes()); // 1 个样本
    entries.extend(1001u32.to_be_bytes()); // 时长 1001
    entries.extend(9u32.to_be_bytes()); // 9 个样本
    entries.extend(1001u32.to_be_bytes());
    let data = full(b"stts", 0, &entry_count_and_entries(&entries));

    let mut info = info_with_samples(10);
    read_stts(&data, 8, data.len() - 8, &mut info);

    assert_eq!(info.stts, vec![(1, 1001), (9, 1001)]);
    // 条目数远小于样本数：正是压缩表示的意义
    assert_eq!(info.stts.iter().map(|(n, _)| *n).sum::<u32>(), 10);
}

#[test]
fn ctts_version_1_reads_signed_offsets() {
    // 负的合成偏移（B 帧的典型值）：version 1 下是 i32
    let mut entries = Vec::new();
    entries.extend(2u32.to_be_bytes());
    entries.extend((-1024i32 as u32).to_be_bytes());
    let data = full(b"ctts", 1, &entry_count_and_entries(&entries));

    let mut info = info_with_samples(2);
    read_ctts(&data, 8, data.len() - 8, &mut info);

    assert_eq!(info.ctts, vec![(2, -1024)]);
}

#[test]
fn ctts_version_0_reads_unsigned_offsets() {
    let mut entries = Vec::new();
    entries.extend(1u32.to_be_bytes());
    entries.extend(2048u32.to_be_bytes());
    let data = full(b"ctts", 0, &entry_count_and_entries(&entries));

    let mut info = info_with_samples(1);
    read_ctts(&data, 8, data.len() - 8, &mut info);

    // version 0 的 2048 必须是 +2048：按 i32 直读只有在符号位为 0 时才对，
    // 这条用例把「读错符号」的代价钉住
    assert_eq!(info.ctts, vec![(1, 2048)]);
}

#[test]
fn a_huge_entry_count_cannot_overrun_the_box() {
    // 平台畸形数据：声明 4 亿条，box 里一条都没有
    let mut payload = 4_000_000_000u32.to_be_bytes().to_vec();
    payload.extend_from_slice(&[0u8; 7]); // 装不满一条 8 字节条目
    let data = full(b"stts", 0, &payload);

    let mut info = info_with_samples(0);
    read_stts(&data, 8, data.len() - 8, &mut info);

    // 按 box 实际长度夹住，不能按声明的 count 去分配
    assert_eq!(info.stts.len(), 0);
}

#[test]
fn stss_is_none_until_the_box_is_seen() {
    let mut info = info_with_samples(5);
    assert_eq!(info.stss, None, "box 不存在 = 全部样本都是关键帧");

    let mut payload = 2u32.to_be_bytes().to_vec();
    payload.extend(1u32.to_be_bytes());
    payload.extend(30u32.to_be_bytes());
    let data = full(b"stss", 0, &payload);
    read_stss(&data, 8, data.len() - 8, &mut info);

    assert_eq!(info.stss, Some(vec![1, 30]));
}

#[test]
fn mdhd_version_0_uses_32_bit_dates() {
    let mut payload = Vec::new();
    payload.extend(0u32.to_be_bytes()); // creation
    payload.extend(0u32.to_be_bytes()); // modification
    payload.extend(90_000u32.to_be_bytes()); // timescale
    payload.extend(5_400_000u32.to_be_bytes()); // duration = 60s
    let data = full(b"mdhd", 0, &payload);

    let mut info = TrackInfo::default();
    read_mdhd(&data, 8, data.len() - 8, &mut info);

    assert_eq!(info.media_timescale, 90_000);
    assert_eq!(info.media_duration, 5_400_000);
    assert_eq!(
        info.media_duration * 1000 / u64::from(info.media_timescale),
        60_000
    );
}

#[test]
fn mdhd_version_1_uses_64_bit_dates() {
    let mut payload = Vec::new();
    payload.extend(0u64.to_be_bytes()); // creation
    payload.extend(0u64.to_be_bytes()); // modification
    payload.extend(48_000u32.to_be_bytes()); // timescale
    payload.extend(4_800_000u64.to_be_bytes()); // duration = 100s
    let data = full(b"mdhd", 1, &payload);

    let mut info = TrackInfo::default();
    read_mdhd(&data, 8, data.len() - 8, &mut info);

    assert_eq!(info.media_timescale, 48_000);
    assert_eq!(info.media_duration, 4_800_000);
}

#[test]
fn a_truncated_mdhd_leaves_the_defaults_alone() {
    // version 1 的 payload 还没到 duration 就断了
    let mut payload = Vec::new();
    payload.extend(0u64.to_be_bytes());
    payload.extend(0u64.to_be_bytes());
    payload.extend(48_000u32.to_be_bytes());
    let data = full(b"mdhd", 1, &payload);

    let mut info = TrackInfo::default();
    read_mdhd(&data, 8, data.len() - 8, &mut info);

    assert_eq!(info.media_timescale, 0, "读不满就不写，免得留下半套时间轴");
    assert_eq!(info.media_duration, 0);
}
