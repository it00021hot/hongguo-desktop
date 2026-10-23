//! 选集区间语法断言（Tauri `utils/range.test.ts` 同集移植）。
//!
//! 纯逻辑，默认构建即可跑（不依赖 gpui test-support）：源文件级引入，
//! 因为壳是纯 bin（模块内 `#[cfg(test)]` 不会执行）。

#[path = "../src/home/downloads_range.rs"]
mod downloads_range;

use downloads_range::{first_n, format_range, last_n, parse_range};

#[test]
fn parses_single_range_and_mixed_expressions() {
    assert_eq!(parse_range("5", 999), vec![5]);
    assert_eq!(parse_range("1-5", 999), vec![1, 2, 3, 4, 5]);
    assert_eq!(
        parse_range("1-3, 7, 10-12", 999),
        vec![1, 2, 3, 7, 10, 11, 12]
    );
    // 中文逗号与数字两侧空格同认
    assert_eq!(parse_range("1 - 3，7", 999), vec![1, 2, 3, 7]);
}

#[test]
fn dedups_sorts_and_drops_illegal_parts() {
    assert_eq!(parse_range("3, 1, 3, 2", 999), vec![1, 2, 3]);
    assert_eq!(parse_range("abc, 5", 999), vec![5]);
    assert!(parse_range("5-1", 999).is_empty(), "倒序区间不算");
    assert!(parse_range("0", 999).is_empty(), "集号从 1 起");
    assert!(parse_range("", 999).is_empty());
    assert!(parse_range("   ", 999).is_empty());
}

#[test]
fn clamps_to_total_and_drops_out_of_range() {
    assert_eq!(parse_range("1-100", 10), (1..=10).collect::<Vec<_>>());
    assert!(parse_range("11", 10).is_empty());
}

#[test]
fn formats_compact_ranges_and_round_trips() {
    assert_eq!(format_range(&[1, 2, 3, 7, 10, 11]), "1-3, 7, 10-11");
    assert_eq!(format_range(&[5]), "5");
    assert_eq!(format_range(&[]), "");
    for input in ["1-5", "1-3, 7, 10-12", "2, 4, 6-8"] {
        assert_eq!(format_range(&parse_range(input, 999)), input);
    }
}

#[test]
fn first_and_last_n_clamp_to_total() {
    assert_eq!(first_n(10, 3), vec![1, 2, 3]);
    assert_eq!(first_n(2, 10), vec![1, 2]);
    assert_eq!(last_n(10, 3), vec![8, 9, 10]);
    assert_eq!(last_n(2, 10), vec![1, 2]);
}
