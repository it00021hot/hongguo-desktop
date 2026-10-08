//! 索引重建的单元测试。
//!
//! 测的是 [`plan`] 产出的素材（样本数、时长、`stts` / `ctts` / `stss` 游程）
//! 与交织顺序，不碰文件 IO——落盘与字节搬运在 `remux.rs` 的用例里验证。

use super::*;
use crate::domain::mp4::fixtures::{TrackPlan, mp4_with_samples};

fn samples(n: usize) -> Vec<Vec<u8>> {
    (0..n).map(|i| vec![(i % 251) as u8; 40 + i]).collect()
}

fn source(tracks: &[TrackPlan]) -> SourceFile {
    read_source_file(&mp4_with_samples(tracks)).expect("fixture 应该能解析")
}

/// 把 `ctts` 游程展开成逐样本的合成偏移。
fn expand_ctts(runs: &[(u32, i32)]) -> Vec<i32> {
    runs.iter()
        .flat_map(|&(n, o)| std::iter::repeat_n(o, n as usize))
        .collect()
}

#[test]
fn durations_are_summed_per_track() {
    let files = vec![
        source(&[TrackPlan::video(samples(3))]),
        source(&[TrackPlan::video(samples(5))]),
        source(&[TrackPlan::video(samples(2))]),
    ];
    let merged = plan(&files).unwrap();

    assert_eq!(merged.tracks[0].sizes.len(), 10, "样本数是三集之和");
    // fixture 每样本 1 个单位时长 → 媒体时基下累计 10
    assert_eq!(duration_of(&merged.tracks[0].mdhd, MDHD_DURATION), 10);
    // mvhd 的电影时基也是 1000，所以 mvhd 与 tkhd 同样累计到 10
    assert_eq!(duration_of(&merged.tracks[0].tkhd, TKHD_DURATION), 10);
    assert_eq!(duration_of(&merged.mvhd, MVHD_DURATION), 10);
}

#[test]
fn durations_are_summed_separately_for_each_track() {
    let files = vec![
        source(&[TrackPlan::video(samples(4)), TrackPlan::audio(samples(9))]),
        source(&[TrackPlan::video(samples(2)), TrackPlan::audio(samples(5))]),
    ];
    let merged = plan(&files).unwrap();

    // 两条轨各自推进：视频 6、音频 14，不共用一个游程
    assert_eq!(merged.tracks[0].sizes.len(), 6);
    assert_eq!(merged.tracks[1].sizes.len(), 14);
    assert_eq!(duration_of(&merged.tracks[0].mdhd, MDHD_DURATION), 6);
    assert_eq!(duration_of(&merged.tracks[1].mdhd, MDHD_DURATION), 14);
}

#[test]
fn ragged_durations_stay_separate_runs() {
    // 每样本时长都不同的轨道，拼接后相邻游程不能被误合并
    let files = vec![
        source(&[TrackPlan::video_ragged(samples(4))]),
        source(&[TrackPlan::video_ragged(samples(4))]),
    ];
    let merged = plan(&files).unwrap();

    let deltas: Vec<u32> = merged.tracks[0].stts.iter().map(|&(_, d)| d).collect();
    assert_eq!(
        deltas,
        vec![1, 2, 3, 4, 1, 2, 3, 4],
        "等值相邻才合并：这组没有相邻等值"
    );
    assert_eq!(
        merged.tracks[0].stts.iter().map(|(n, _)| *n).sum::<u32>(),
        8
    );
    // 时长 = (1+2+3+4) * 2 = 20
    assert_eq!(duration_of(&merged.tracks[0].mdhd, MDHD_DURATION), 20);
}

#[test]
fn equal_runs_from_adjacent_files_are_merged() {
    // fixture 默认每样本时长都是 1，三集拼起来应该压成一条游程
    let files = vec![
        source(&[TrackPlan::video(samples(3))]),
        source(&[TrackPlan::video(samples(4))]),
    ];
    let merged = plan(&files).unwrap();
    assert_eq!(merged.tracks[0].stts, vec![(7, 1)]);
}

#[test]
fn a_file_without_ctts_mixes_with_files_that_have_it() {
    let files = vec![
        source(&[TrackPlan::video_with_b_frames(samples(4))]),
        // 这一集没有 ctts
        source(&[TrackPlan::video(samples(3))]),
        source(&[TrackPlan::video_with_b_frames(samples(2))]),
    ];
    let merged = plan(&files).unwrap();

    let ctts = &merged.tracks[0].ctts;
    // 条目覆盖的样本数必须等于总样本数，缺一段就会让后面的样本整体错位
    assert_eq!(ctts.iter().map(|(n, _)| *n).sum::<u32>(), 9);
    // 第 2 集整段补 0；它后面紧接着的第 3 集首个样本也是 0，两段并成一条游程
    assert_eq!(ctts, &vec![(2, 0), (2, -1024), (4, 0), (1, -1024)]);
    // 展开成逐样本：源里是 [0,0,-1024,-1024] / [0,0,0] / [0,-1024]
    assert_eq!(
        expand_ctts(ctts),
        vec![0, 0, -1024, -1024, 0, 0, 0, 0, -1024]
    );
}

#[test]
fn no_file_means_no_ctts_box() {
    let files = vec![
        source(&[TrackPlan::video(samples(3))]),
        source(&[TrackPlan::video(samples(3))]),
    ];
    let merged = plan(&files).unwrap();
    assert!(
        merged.tracks[0].ctts.is_empty(),
        "都没 ctts 就不该凭空造一张"
    );
}

#[test]
fn sync_sample_numbers_are_shifted_by_the_preceding_files() {
    // video() 每 5 帧一个关键帧：样本 1、6、11…
    let files = vec![
        source(&[TrackPlan::video(samples(4))]),
        source(&[TrackPlan::video(samples(7))]),
    ];
    let merged = plan(&files).unwrap();

    let sync = merged.tracks[0].stss.clone().expect("有 stss 就必须输出");
    // 第 1 集 1..4 → 关键帧 1；第 2 集 1..7 → 1、6 平移 4 → 5、10
    assert_eq!(sync, vec![1, 5, 10]);
    assert!(sync.iter().all(|n| *n <= 11), "不能超出合并后的样本总数");
}

#[test]
fn a_file_without_stss_contributes_every_sample_as_a_keyframe() {
    // 音频轨不写 stss（全部样本都是同步样本），视频轨写
    let files = vec![
        source(&[TrackPlan::video(samples(4)), TrackPlan::audio(samples(3))]),
        source(&[TrackPlan::video(samples(2)), TrackPlan::audio(samples(2))]),
    ];
    let merged = plan(&files).unwrap();

    assert_eq!(
        merged.tracks[0].stss.as_deref(),
        Some([1u32, 5].as_slice()),
        "视频轨：每 5 帧一个关键帧 → 第 1 集得 1、第 2 集的 1 平移 4 得 5"
    );
    // 音频轨两集都没有 stss → 全部样本都是关键帧
    assert_eq!(merged.tracks[1].stss, Some(vec![1, 2, 3, 4, 5]));
}

#[test]
fn interleave_order_follows_the_source_layout() {
    // fixture 按「轨0样本0, 轨1样本0, 轨0样本1, …」轮转交织
    let file = source(&[TrackPlan::video(samples(3)), TrackPlan::audio(samples(3))]);
    assert_eq!(
        file.order,
        vec![(0, 0), (1, 0), (0, 1), (1, 1), (0, 2), (1, 2)]
    );
}

#[test]
fn single_input_round_trips() {
    let files = vec![source(&[TrackPlan::video(samples(6))])];
    let merged = plan(&files).unwrap();

    assert_eq!(merged.tracks[0].sizes.len(), 6);
    assert_eq!(duration_of(&merged.tracks[0].mdhd, MDHD_DURATION), 6);
    assert_eq!(duration_of(&merged.mvhd, MVHD_DURATION), 6);
}

#[test]
fn mismatched_track_counts_are_refused() {
    let files = vec![
        source(&[TrackPlan::video(samples(2))]),
        source(&[TrackPlan::video(samples(2)), TrackPlan::audio(samples(2))]),
    ];
    let err = plan(&files).expect_err("轨数不同必须拒绝");
    assert!(err.to_string().contains("轨道"), "实际: {err}");
}

#[test]
fn a_missing_moov_is_an_error_not_an_empty_file() {
    assert!(read_source_file(&[0xffu8; 512]).is_err());
}

#[test]
fn the_two_moov_builds_have_the_same_length() {
    // `prepare` 靠「长度只取决于样本数与偏移宽度」用占位偏移量一次长度。
    // 这条用例把那个前提钉住：一旦偏移宽度变了，长度就必须跟着变。
    let files = vec![
        source(&[TrackPlan::video(samples(3))]),
        source(&[TrackPlan::video(samples(5))]),
    ];
    let merged = plan(&files).unwrap();
    let zeros: Vec<Vec<u64>> = merged
        .tracks
        .iter()
        .map(|t| vec![0u64; t.sizes.len()])
        .collect();
    let real: Vec<Vec<u64>> = merged
        .tracks
        .iter()
        .map(|t| (1..=t.sizes.len() as u64).collect())
        .collect();

    assert_eq!(
        build_moov(&merged, &zeros, false).len(),
        build_moov(&merged, &real, false).len()
    );
    // 升到 64 位偏移时表更大，宽度确实体现在长度上
    assert!(build_moov(&merged, &real, true).len() > build_moov(&merged, &real, false).len());
}

#[test]
fn the_stco_entries_land_inside_the_mdat_payload() {
    let files = vec![source(&[TrackPlan::video(samples(4))])];
    let merged = plan(&files).unwrap();
    let payload: u64 = (0..4).map(|i| 40 + i).sum();
    let start = mdat_data_start(28, build_moov(&merged, &[vec![0; 4]], false).len(), false);

    let offsets: Vec<u64> = (0..4)
        .map(|i| start + (0..i).map(|j| 40 + j).sum::<u64>())
        .collect();
    assert_eq!(offsets[0], start);
    assert_eq!(
        offsets[3] + 43,
        start + payload,
        "最后一个样本的结尾应正好接上 mdat 末尾"
    );
}

#[test]
fn wide_offsets_are_only_needed_past_four_gigabytes() {
    assert!(!needs_wide_offsets(u64::from(u32::MAX)));
    assert!(needs_wide_offsets(u64::from(u32::MAX) + 1));
    assert_eq!(mdat_header_len(false), 8);
    assert_eq!(mdat_header_len(true), 16);
}
