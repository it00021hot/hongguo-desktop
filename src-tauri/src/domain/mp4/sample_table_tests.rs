//! MP4 样本表解析的单元测试。
//!
//! 以 `#[path]` 挂到 `sample_table.rs` 下，`use super::*` 直接拿实现里的
//! 私有函数与类型。

use super::*;
use crate::domain::mp4::r#box::build_box;

/// 构造 FullBox：`version(1) + flags(3)` 由本函数写入，
/// `payload` 只放紧随其后的字段（如 sample_size / count / 数据）。
fn full_box(kind: &str, version: u8, flags: u8, payload: &[u8]) -> Vec<u8> {
    let mut body = vec![version, flags, 0, 0];
    body.extend_from_slice(payload);
    let k: [u8; 4] = kind.as_bytes().try_into().unwrap_or([0; 4]);
    build_box(&k, &body)
}

#[test]
fn reads_stsz_variable_sizes() {
    // stsz 载荷：sample_size(4)=0(变长) | count(4)=3 | sizes[3]
    let mut payload = 0u32.to_be_bytes().to_vec();
    payload.extend_from_slice(&3u32.to_be_bytes()); // count
    payload.extend_from_slice(&100u32.to_be_bytes());
    payload.extend_from_slice(&200u32.to_be_bytes());
    payload.extend_from_slice(&300u32.to_be_bytes());
    let stsz = full_box("stsz", 0, 0, &payload);

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
    let stsz = full_box("stsz", 0, 0, &payload);

    let mut info = TrackInfo::default();
    read_stsz(&stsz, 8, stsz.len() - 8, &mut info);
    assert_eq!(info.sample_count, 4);
    assert!(info.samples.iter().all(|(_, s)| *s == 256));
}

#[test]
fn reads_codec_from_stsd() {
    // stsd 载荷：entry_count(4) | entry: size(4) + format(4) + ...
    let mut payload = 1u32.to_be_bytes().to_vec(); // entry_count
    payload.extend_from_slice(&16u32.to_be_bytes()); // sample entry size
    payload.extend_from_slice(b"hvc1"); // format
    payload.extend_from_slice(&[0u8; 8]);
    let stsd = full_box("stsd", 0, 0, &payload);

    assert_eq!(read_codec(&stsd, 8, stsd.len() - 8), "hvc1");
}

#[test]
fn total_size_sums_samples() {
    let info = TrackInfo {
        samples: vec![(0, 100), (100, 200), (300, 50)],
        ..Default::default()
    };
    assert_eq!(info.total_size(), 350);
}
