//! 时间助手。
//!
//! signer 的 `now_millis`（u64）是 JS 1:1 移植的一部分、禁改区，**不**收敛进来；它与 [`now_ms`] 只差返回类型（u64/i64），语义各自保留。

use crate::consts::units::{MS_PER_DAY, MS_PER_HOUR};

/// 当前 unix 毫秒（i64；时钟早于 1970 时给 0），供退避窗口、注册时间戳等有符号运算与落库。
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// UTC 毫秒时间戳 → 北京时区（UTC+8）的 "20261004" 形式日期；时间戳 0（未定档）不归任何一天。
pub fn beijing_date(ts_ms: i64) -> String {
    if ts_ms <= 0 {
        return String::new();
    }
    // Hinnant 的 civil_from_days：days 自 1970-01-01
    let z = (ts_ms + 8 * MS_PER_HOUR).div_euclid(MS_PER_DAY) + 719_468;
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
