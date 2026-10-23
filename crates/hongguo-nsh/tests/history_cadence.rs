//! 观看历史上报节拍断言（Tauri `history-cadence.test.ts` 同规则；GPUI 壳
//! 同名测试的 NSH 份）。
//!
//! 纯逻辑，默认构建即可跑（不依赖任何壳设施）：源文件级引入，因为壳是
//! 纯 bin（模块内 `#[cfg(test)]` 不会执行）。

#[path = "../src/player/cadence.rs"]
mod cadence;

use cadence::{FIRST_REPORT_MS, HistoryCadence};

/// 中途更新节拍（毫秒），与实现同值（改实现必须同步改这里）。
const PERIODIC: i64 = 60_000;

fn fresh() -> HistoryCadence {
    HistoryCadence::default()
}

#[test]
fn new_series_first_report_waits_five_seconds_then_periodic() {
    let mut cadence = fresh();
    assert!(
        !cadence.due("s1:1", "s1", FIRST_REPORT_MS - 1),
        "4.999s 不建行"
    );
    assert!(cadence.due("s1:1", "s1", FIRST_REPORT_MS), "5s 首报");
    // 首报之后 60 秒内不再上报——旧实现在这里永远不再抬头
    assert!(!cadence.due("s1:1", "s1", 10_000));
    assert!(!cadence.due("s1:1", "s1", FIRST_REPORT_MS + PERIODIC - 1));
    assert!(cadence.due("s1:1", "s1", FIRST_REPORT_MS + PERIODIC));
    // 第二拍从第一拍的位置重新计，而不是从 0 点算
    assert!(!cadence.due("s1:1", "s1", FIRST_REPORT_MS + PERIODIC + 30_000));
    assert!(cadence.due("s1:1", "s1", FIRST_REPORT_MS + PERIODIC * 2));
}

#[test]
fn same_series_next_episode_flips_immediately() {
    let mut cadence = fresh();
    assert!(cadence.due("s1:7", "s1", 5_000));
    // 第七集播完切第八集：0.2 秒也要报（历史行属于剧，不能还停在第七集）
    assert!(cadence.due("s1:8", "s1", 200));
    assert!(cadence.has_reported("s1:8"));
    assert!(!cadence.due("s1:8", "s1", 1_000), "同位置后续心跳不重复");
    // 翻页只带了 0 秒附近的位置：跨过 5 秒补一次首报，历史行的进度才有意义
    assert!(cadence.due("s1:8", "s1", 5_000));
    assert!(!cadence.due("s1:8", "s1", 6_000));
}

#[test]
fn switching_to_other_series_keeps_five_second_threshold() {
    let mut cadence = fresh();
    assert!(cadence.due("s1:1", "s1", 5_000));
    assert!(!cadence.due("s2:1", "s2", 1_000), "快滑一路不落噪点行");
    assert!(!cadence.due("s2:1", "s2", 4_999));
    assert!(cadence.due("s2:1", "s2", 5_000));
}

#[test]
fn seek_back_or_forward_reports_at_once() {
    let mut cadence = fresh();
    assert!(cadence.due("s1:1", "s1", 5_000));
    assert!(!cadence.due("s1:1", "s1", 40_000));
    assert!(cadence.due("s1:1", "s1", 1_000), "回跳立即补");
    assert!(!cadence.due("s1:1", "s1", 1_500), "3 秒内抖动不算 seek");
    assert!(cadence.due("s1:1", "s1", 200_000), "前跳也立即补");
}

#[test]
fn stalled_position_never_reports_until_forced() {
    let mut cadence = fresh();
    assert!(cadence.due("s1:1", "s1", 5_000));
    for _ in 0..5 {
        assert!(!cadence.due("s1:1", "s1", 5_000), "暂停不计时");
    }
    assert!(!fresh().due("s1:1", "s1", 0), "片头 0 秒不建行");
}

#[test]
fn note_rewinds_the_clock_and_marks_episode_reported() {
    let mut cadence = fresh();
    cadence.note("s1:8", "s1", 2_000);
    assert!(cadence.has_reported("s1:8"));
    assert!(!cadence.has_reported("s1:7"));
    assert!(cadence.due("s1:8", "s1", 2_000 + PERIODIC));
}
