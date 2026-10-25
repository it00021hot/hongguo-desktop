//! 展示格式化：相对时间、计数、时间与倍速文案（与 Tauri 版口径一致）。

/// 体积格式化（字节 → 可读字符串）。与 Tauri `utils/format.ts` 的
/// formatBytes 同口径：1024 进制、B 档不带小数、其余一位小数。
/// crate 级自由函数：磁盘清理页与下载管理页共用。
pub(crate) fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut i = 0;
    while value >= 1024.0 && i < units.len() - 1 {
        value /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", units[i])
    }
}

/// 秒 → 时分秒（与 Tauri `utils/format.ts` 的 formatDuration 同口径：
/// 不足 1 小时 mm:ss，否则 h:mm:ss）。历史行进度徽标用。
pub(crate) fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "00:00".to_string();
    }
    let total = seconds as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// 播放量短格式（Tauri `formatPlayCount` 同口径：亿/万阶梯、去尾零）。
/// 找剧搜索卡热度行用。
pub(crate) fn format_play_count(n: i64) -> String {
    if crate::i18n::locale() == crate::i18n::Locale::EnUs {
        if n >= 1_000_000_000 {
            return format!("{:.1}B", n as f64 / 1_000_000_000.0).replace(".0", "");
        }
        if n >= 1_000_000 {
            return format!("{:.1}M", n as f64 / 1_000_000.0).replace(".0", "");
        }
        if n >= 1_000 {
            return format!("{:.1}K", n as f64 / 1_000.0).replace(".0", "");
        }
        return n.to_string();
    }
    if n >= 100_000_000 {
        return format!("{:.1}亿", n as f64 / 100_000_000.0).replace(".0", "");
    }
    if n >= 10_000 {
        return format!("{:.1}万", n as f64 / 10_000.0).replace(".0", "");
    }
    n.to_string()
}

/// 相对时间（评论时间戳）。分层与 Tauri `utils/relative-time.ts` 同口径
/// （刚刚 / N 分钟前 / N 小时前 / 昨天 / N 天前 30 天内 /
/// M月D日）——此前自创「N 个月前/N 年前」与前端漂移。技术栈不同不共享
/// 实现，改分层两边同步。crate 级自由函数：首页评论面板与详情页剧评共用。
pub(crate) fn relative_time(ts_ms: i64) -> String {
    if ts_ms <= 0 {
        return String::new();
    }
    let secs = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        - ts_ms / 1000)
        .max(0);
    match secs {
        0..=59 => crate::i18n::t("player.comments.justNow"),
        60..=3_599 => crate::i18n::tf(
            "player.comments.minutesAgo",
            &[("n", &(secs / 60).to_string())],
        ),
        3_600..=86_399 => crate::i18n::tf(
            "player.comments.hoursAgo",
            &[("n", &(secs / 3_600).to_string())],
        ),
        86_400..=172_799 => crate::i18n::t("player.comments.yesterday"),
        172_800..=2_591_999 => crate::i18n::tf(
            "player.comments.daysAgo",
            &[("n", &(secs / 86_400).to_string())],
        ),
        _ => {
            use chrono::{Datelike, TimeZone};
            let dt = chrono::Local
                .timestamp_millis_opt(ts_ms)
                .single()
                .unwrap_or_else(chrono::Local::now);
            match crate::i18n::locale() {
                crate::i18n::Locale::ZhCn => format!("{}月{}日", dt.month(), dt.day()),
                crate::i18n::Locale::EnUs => format!("{:02}/{:02}", dt.month(), dt.day()),
            }
        }
    }
}

impl super::HomeView {
    /// `mm:ss`（分钟补零，Tauri `formatDuration` 同款——「00:05」不显「0:05」，
    /// 读数宽度随进度稳定不抖）。
    pub(super) fn fmt_secs(secs: f64) -> String {
        let s = secs.max(0.0) as u64;
        format!("{:02}:{:02}", s / 60, s % 60)
    }

    /// 倍速按钮文案：整数不带小数点（1x / 1.5x / 0.75x）。
    pub(super) fn fmt_rate(rate: f64) -> String {
        if rate.fract() == 0.0 {
            format!("{}x", rate as i64)
        } else {
            format!("{rate}x")
        }
    }

    /// 互动栏计数格式化：对齐 Tauri `interaction-rail.tsx` 的 fmtCount
    /// （抖音系阶梯）——≥100万 → 「4.4百万」，≥1万 → 「52万」（余数
    /// ≥1000 才带一位小数），其余原样。注意这是**互动栏专用**口径，与
    /// `utils/format.ts` 的 formatCountPrecise（亿/万，详情页头部）是
    /// Tauri 端就分开的两套。
    pub(super) fn fmt_count(n: i64) -> String {
        match crate::i18n::locale() {
            crate::i18n::Locale::ZhCn => {
                if n >= 1_000_000 {
                    return format!("{:.1}", n as f64 / 1_000_000.0).replace(".0", "") + "百万";
                }
                if n >= 10_000 {
                    let text = if n % 10_000 >= 1_000 {
                        format!("{:.1}", n as f64 / 10_000.0)
                    } else {
                        format!("{}", n / 10_000)
                    };
                    return text.replace(".0", "") + "万";
                }
            }
            crate::i18n::Locale::EnUs => {
                if n >= 1_000_000 {
                    return format!("{:.1}M", n as f64 / 1_000_000.0).replace(".0", "");
                }
                if n >= 1_000 {
                    return format!("{:.1}K", n as f64 / 1_000.0).replace(".0", "");
                }
            }
        }
        format!("{n}")
    }

    /// 播放量格式化（系列剧卡片数据行）：对齐 Tauri `formatPlayCount`
    /// （亿/万阶梯，详情页头部同源口径）。
    pub(super) fn fmt_play_count(n: i64) -> String {
        if crate::i18n::locale() == crate::i18n::Locale::EnUs {
            return format_play_count(n);
        }
        if n >= 100_000_000 {
            return format!("{:.1}亿", n as f64 / 100_000_000.0).replace(".0", "");
        }
        if n >= 10_000 {
            return format!("{:.1}万", n as f64 / 10_000.0).replace(".0", "");
        }
        format!("{n}")
    }
}
