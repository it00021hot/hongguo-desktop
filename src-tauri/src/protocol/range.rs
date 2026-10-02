//! Range 请求解析与 206 响应构造。
//!
//! `<video>` 拖进度条靠的是 Range 请求，解析错了会出现「从头能播、拖动就坏」。

/// 协议响应：`状态码 + 响应头 + 响应体`。
pub type ProtocolResponse = (u16, Vec<(String, String)>, Vec<u8>);

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

/// 解析 `Range` 头。
///
/// `total` 是资源总大小；`start >= total` 视为不可满足。
pub fn parse_range(header: Option<&str>, total: u64) -> RangeSpec {
    let Some(h) = header else {
        return RangeSpec::Full;
    };
    let Some(spec) = h.trim().strip_prefix("bytes=") else {
        return RangeSpec::Full;
    };
    // 只处理单区间；多区间（逗号分隔）退化为整文件响应
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

/// 构造 206 响应头。
pub fn partial_headers(start: u64, end: u64, total: u64) -> Vec<(String, String)> {
    vec![
        ("Content-Type".into(), "video/mp4".into()),
        ("Accept-Ranges".into(), "bytes".into()),
        (
            "Content-Range".into(),
            format!("bytes {start}-{end}/{total}"),
        ),
        ("Content-Length".into(), (end - start + 1).to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_header_is_full() {
        assert_eq!(parse_range(None, 100), RangeSpec::Full);
    }

    #[test]
    fn closed_range() {
        assert_eq!(
            parse_range(Some("bytes=0-99"), 1000),
            RangeSpec::Closed { start: 0, end: 99 }
        );
    }

    #[test]
    fn open_range() {
        assert_eq!(
            parse_range(Some("bytes=500-"), 1000),
            RangeSpec::Open { start: 500 }
        );
    }

    #[test]
    fn suffix_range() {
        assert_eq!(
            parse_range(Some("bytes=-100"), 1000),
            RangeSpec::Closed {
                start: 900,
                end: 999
            }
        );
    }

    #[test]
    fn end_is_clamped_to_total() {
        assert_eq!(
            parse_range(Some("bytes=0-99999"), 1000),
            RangeSpec::Closed { start: 0, end: 999 }
        );
    }

    #[test]
    fn start_beyond_total_is_unsatisfiable() {
        assert_eq!(
            parse_range(Some("bytes=2000-"), 1000),
            RangeSpec::Unsatisfiable
        );
    }

    #[test]
    fn garbage_is_unsatisfiable() {
        assert_eq!(
            parse_range(Some("bytes=abc-def"), 1000),
            RangeSpec::Unsatisfiable
        );
        assert_eq!(parse_range(Some("items=0-1"), 1000), RangeSpec::Full);
        assert_eq!(
            parse_range(Some("bytes=10-5"), 1000),
            RangeSpec::Unsatisfiable
        );
        assert_eq!(
            parse_range(Some("bytes=-0"), 1000),
            RangeSpec::Unsatisfiable
        );
    }

    #[test]
    fn multi_range_falls_back_to_full() {
        assert_eq!(
            parse_range(Some("bytes=0-99,200-299"), 1000),
            RangeSpec::Full
        );
    }

    #[test]
    fn headers_are_wellformed() {
        let h = partial_headers(0, 99, 1000);
        let get = |k: &str| {
            h.iter()
                .find(|(a, _)| a == k)
                .map(|(_, b)| b.clone())
                .unwrap()
        };
        assert_eq!(get("Content-Range"), "bytes 0-99/1000");
        assert_eq!(get("Content-Length"), "100");
        assert_eq!(get("Accept-Ranges"), "bytes");
    }
}
