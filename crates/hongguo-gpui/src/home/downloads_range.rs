//! 选集区间语法（Tauri `utils/range.ts` 1:1 移植）：`1-50`、`1-10, 25,
//! 30-45`、`1, 3, 5`；空串 = 空选集（不是全选，调用方据此区分「没填」与
//! 「全选」）。上限夹到总集数，避免 `1-99999` 把整部剧都选上。
//!
//! 纯逻辑文件：集成测试按源文件引入（壳是纯 bin，模块内 `#[cfg(test)]`
//! 不会执行），见 `tests/range_picker.rs`。

/// 解析区间串，返回升序去重的集号列表。
pub(crate) fn parse_range(input: &str, max: u32) -> Vec<u32> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<u32> = Vec::new();
    for part in trimmed.split([',', '，']) {
        let seg = part.trim();
        if seg.is_empty() {
            continue;
        }
        if let Some((from, to)) = seg.split_once('-') {
            let (Ok(from), Ok(to)) = (from.trim().parse::<u32>(), to.trim().parse::<u32>()) else {
                continue;
            };
            if from < 1 || to < from {
                continue;
            }
            let end = to.min(max);
            for i in from..=end {
                out.push(i);
            }
            continue;
        }
        if let Ok(n) = seg.parse::<u32>()
            && n >= 1
            && n <= max
        {
            out.push(n);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// 集号序列 → 紧凑区间串（`1-5, 8, 12-14`）。
pub(crate) fn format_range(indices: &[u32]) -> String {
    if indices.is_empty() {
        return String::new();
    }
    let mut sorted = indices.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut parts: Vec<String> = Vec::new();
    let mut start = sorted[0];
    let mut prev = sorted[0];
    for &n in &sorted[1..] {
        if n == prev + 1 {
            prev = n;
            continue;
        }
        parts.push(if start == prev {
            start.to_string()
        } else {
            format!("{start}-{prev}")
        });
        start = n;
        prev = n;
    }
    parts.push(if start == prev {
        start.to_string()
    } else {
        format!("{start}-{prev}")
    });
    parts.join(", ")
}

/// 前 n 集。
pub(crate) fn first_n(total: u32, n: u32) -> Vec<u32> {
    let count = n.min(total);
    (1..=count).collect()
}

/// 后 n 集。
pub(crate) fn last_n(total: u32, n: u32) -> Vec<u32> {
    let count = n.min(total);
    (total - count + 1..=total).collect()
}
