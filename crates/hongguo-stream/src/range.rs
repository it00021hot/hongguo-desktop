//! NSH HTTP 供数使用的 Range 请求解析。

/// 解析后的 Range。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RangeSpec {
    /// 无 Range，整文件请求
    Full,
    /// 闭区间 `bytes=start-end`
    Closed { start: u64, end: u64 },
    /// 开放式 `bytes=start-`，只回一段已就绪的窗口
    Open { start: u64 },
    /// 非法 Range
    Unsatisfiable,
}

/// 解析 `Range` 头。`total` 是资源总大小；`start >= total` 视为不可满足。
pub fn parse_range(header: Option<&str>, total: u64) -> RangeSpec {
    let Some(h) = header else {
        return RangeSpec::Full;
    };
    let Some(spec) = h.trim().strip_prefix("bytes=") else {
        return RangeSpec::Full;
    };
    if spec.contains(',') {
        return RangeSpec::Full;
    }

    let (start_s, end_s) = match spec.split_once('-') {
        Some(v) => v,
        None => return RangeSpec::Unsatisfiable,
    };

    if start_s.is_empty() {
        // `bytes=-N` 表示末尾 N 字节
        let Ok(len) = end_s.parse::<u64>() else {
            return RangeSpec::Unsatisfiable;
        };
        if len == 0 {
            return RangeSpec::Unsatisfiable;
        }
        let start = total.saturating_sub(len);
        return RangeSpec::Closed {
            start,
            end: total.saturating_sub(1),
        };
    }

    let Ok(start) = start_s.parse::<u64>() else {
        return RangeSpec::Unsatisfiable;
    };
    if start >= total {
        return RangeSpec::Unsatisfiable;
    }

    if end_s.is_empty() {
        return RangeSpec::Open { start };
    }

    let Ok(end) = end_s.parse::<u64>() else {
        return RangeSpec::Unsatisfiable;
    };
    let end = end.min(total.saturating_sub(1));
    if start > end {
        return RangeSpec::Unsatisfiable;
    }
    RangeSpec::Closed { start, end }
}
