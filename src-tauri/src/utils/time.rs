//! 时间助手（P3-C9 自 bootstrap/device 与 calendar 收敛）。
//!
//! signer 的 `now_millis`（u64）是 JS 1:1 移植的一部分、禁改区，**不**收敛
//! 进来；它与 [`now_ms`] 只差返回类型（u64/i64），语义各自保留。

/// 当前 unix 毫秒（i64；时钟早于 1970 时给 0）。
///
/// 来源：bootstrap/device.rs 的 `now_ms`（退避窗口、注册时间戳等需要
/// 参与有符号运算与落库）。
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// unix 秒 → 北京时区（UTC+8）的 "20261004" 形式日期。
///
/// 日历的日期桶按北京时间的"当天"划分；时间戳 0（未定档）不归任何一天。
/// 来源：calendar/mod.rs 的 `beijing_date`。
pub(crate) fn beijing_date(ts: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    // Hinnant 的 civil_from_days：days 自 1970-01-01
    let z = (ts + 8 * 3600).div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_ms_is_epoch_millis() {
        assert!(now_ms() > 1_700_000_000_000, "应晚于 2023-11");
    }

    #[test]
    fn beijing_date_buckets_by_utc8_midnight() {
        assert!(beijing_date(0).is_empty(), "未定档（ts=0）不归任何一天");
        assert!(beijing_date(-5).is_empty());
        assert_eq!(beijing_date(1), "19700101");
        // 15:59:59 UTC 还是北京当天，16:00:00 就翻日
        assert_eq!(beijing_date(57_599), "19700101");
        assert_eq!(beijing_date(57_600), "19700102");
    }

    #[test]
    fn beijing_date_matches_known_captures() {
        // 2026-10-04 抓包样本里的 publish_time（日历/预约域实测值）
        assert_eq!(beijing_date(1_790_957_041), "20261003");
        assert_eq!(beijing_date(1_791_216_360), "20261006");
        assert_eq!(beijing_date(1_759_276_800), "20251001");
    }
}
