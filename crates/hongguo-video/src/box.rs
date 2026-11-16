//! MP4 box 树解析：每个 box 是 `[4 字节长度][4 字节类型][载荷]`；
//! 长度为 1 表示真实长度是 64 位（largesize）、为 0 表示「到文件末尾」，两种都必须正确处理否则解析错位。

/// 一个 box 的头部。
#[derive(Debug, Clone, PartialEq)]
pub struct BoxHeader {
    /// box 类型，如 `moov` / `trak`
    pub kind: [u8; 4],
    /// 载荷在文件中的绝对偏移
    pub start: usize,
    /// 载荷长度
    pub size: usize,
    /// 整个 box（含头部）的长度
    pub total_size: usize,
    /// 头部本身的长度：常规 box 是 8，largesize 是 16——把载荷连头带尾原样取出时需要它，直接减 8 在 largesize box 上会错位。
    pub header_size: usize,
}

impl BoxHeader {
    /// 类型转成字符串，便于比较。
    pub fn kind_str(&self) -> String {
        String::from_utf8_lossy(&self.kind).to_string()
    }

    /// 是否是指定类型。
    pub fn is(&self, kind: &str) -> bool {
        self.kind == kind.as_bytes()
    }
}

/// 解析 `[start, end)` 区间内的 box 列表。
pub fn parse_boxes(data: &[u8], start: usize, end: usize) -> Vec<BoxHeader> {
    let mut out = Vec::new();
    let mut pos = start;

    while pos + 8 <= end {
        let size32 =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let kind = [data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]];

        let (payload_size, header_size) = match size32 {
            // size == 1：真实长度在紧跟其后的 8 字节
            1 => {
                if pos + 16 > end {
                    break;
                }
                let large = u64::from_be_bytes([
                    data[pos + 8],
                    data[pos + 9],
                    data[pos + 10],
                    data[pos + 11],
                    data[pos + 12],
                    data[pos + 13],
                    data[pos + 14],
                    data[pos + 15],
                ]) as usize;
                (large.saturating_sub(16), 16)
            }
            // size == 0：延伸到区间末尾
            0 => (end - pos - 8, 8),
            n => (n.saturating_sub(8), 8),
        };

        let payload_start = pos + header_size;
        // 载荷越界说明文件被截断，停止解析而不是读越界
        if payload_start > end || payload_start + payload_size > end {
            break;
        }

        out.push(BoxHeader {
            kind,
            start: payload_start,
            size: payload_size,
            total_size: header_size + payload_size,
            header_size,
        });

        pos = payload_start + payload_size;
        // 不需要「防停滞」守卫：header_size ≥ 8，pos 每次至少前进 8（size==0 分支也把 pos 推到 end）；
        // 曾放过守卫，把载荷为空的合法 box（恰好 8 字节的 free）误判成停滞、其后所有 box 消失
    }

    out
}

/// 递归查找第一个指定类型的 box。
pub fn find_box(data: &[u8], start: usize, end: usize, kind: &str) -> Option<BoxHeader> {
    for b in parse_boxes(data, start, end) {
        if b.is(kind) {
            return Some(b);
        }
        if matches!(
            b.kind_str().as_str(),
            "moov" | "trak" | "mdia" | "minf" | "stbl" | "moof" | "traf" | "edts"
        ) && let Some(found) = find_box(data, b.start, b.start + b.size, kind)
        {
            return Some(found);
        }
    }
    None
}

/// 构造一个 box 的字节表示。
pub fn build_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    out
}
