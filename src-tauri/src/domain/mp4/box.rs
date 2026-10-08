//! MP4 box 树解析。
//!
//! MP4 由嵌套的 box 组成，每个 box 是 `[4 字节长度][4 字节类型][载荷]`。
//! 长度占 4 字节且为 1 时表示真实长度是 64 位（largesize），为 0 表示
//! 「到文件末尾」。两种都必须正确处理，否则会解析错位。

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
    /// 头部本身的长度：常规 box 是 8，用 largesize 时是 16。
    /// 要把 `[start, start+size]` 连头带尾原样取出来时需要它——
    /// 直接减 8 会在 largesize box 上错位。
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
        // 不需要「防停滞」守卫：header_size ≥ 8，每次迭代 pos 至少前进 8，
        // 唯一的特殊分支（size == 0 延伸到末尾）也会把 pos 推到 end。
        // 曾经在这里放 `if pos <= payload_start { break }`，结果把**载荷为空
        // 的合法 box**（比如恰好 8 字节的 `free`）误判成停滞，后续所有 box
        // 直接消失——实测 ffmpeg 产出的文件就带这种 box。
    }

    out
}

/// 递归查找第一个指定类型的 box。
pub fn find_box(data: &[u8], start: usize, end: usize, kind: &str) -> Option<BoxHeader> {
    for b in parse_boxes(data, start, end) {
        if b.is(kind) {
            return Some(b);
        }
        // 容器类 box 向下找
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_box_does_not_stop_the_scan() {
        // 回归钉住：恰好 8 字节的空 `free` box 曾被「防停滞」守卫误判，
        // 其后的 mdat/moov 全部消失。ffmpeg 产出的文件就带这种 box。
        let data: Vec<u8> = [
            box_of("ftyp", b"xxxx"),
            box_of("free", b""),
            box_of("moov", b"real"),
        ]
        .concat();
        let boxes = parse_boxes(&data, 0, data.len());
        let kinds: Vec<String> = boxes.iter().map(|b| b.kind_str()).collect();
        assert_eq!(kinds, ["ftyp", "free", "moov"], "实际: {kinds:?}");
        assert!(find_box(&data, 0, data.len(), "moov").is_some());
    }

    fn box_of(kind: &str, payload: &[u8]) -> Vec<u8> {
        let mut k = [0u8; 4];
        k.copy_from_slice(kind.as_bytes());
        build_box(&k, payload)
    }

    #[test]
    fn parses_single_box() {
        let data = box_of("free", &[1, 2, 3, 4]);
        let boxes = parse_boxes(&data, 0, data.len());
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].kind_str(), "free");
        assert_eq!(boxes[0].size, 4);
        assert_eq!(boxes[0].start, 8);
    }

    #[test]
    fn parses_siblings() {
        let mut data = box_of("ftyp", &[0; 8]);
        data.extend(box_of("mdat", &[0; 16]));
        data.extend(box_of("moov", &[0; 32]));

        let boxes = parse_boxes(&data, 0, data.len());
        let kinds: Vec<String> = boxes.iter().map(|b| b.kind_str()).collect();
        assert_eq!(kinds, vec!["ftyp", "mdat", "moov"]);
        assert_eq!(boxes[1].size, 16);
    }

    #[test]
    fn handles_size_zero_to_end() {
        // size == 0 表示延伸到末尾
        let mut data = Vec::new();
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(b"mdat");
        data.extend_from_slice(&[9u8; 10]);

        let boxes = parse_boxes(&data, 0, data.len());
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].size, 10);
    }

    #[test]
    fn handles_64bit_size() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(b"mdat");
        data.extend_from_slice(&16u64.to_be_bytes()); // 真实总长 16
        data.extend_from_slice(&[7u8; 0]); // 无载荷

        let boxes = parse_boxes(&data, 0, data.len());
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].size, 0);
        assert_eq!(boxes[0].total_size, 16);
    }

    #[test]
    fn truncated_input_stops_cleanly() {
        let mut data = box_of("ftyp", &[0; 8]);
        data.extend_from_slice(&[0, 0, 0, 100]); // 声明 100 字节但数据不足
        data.extend_from_slice(b"mdat");
        data.extend_from_slice(&[1, 2]);

        // 不应 panic，也不应返回越界的 box
        let boxes = parse_boxes(&data, 0, data.len());
        assert_eq!(boxes.len(), 1);
    }

    #[test]
    fn find_box_recurses_containers() {
        let inner = box_of("stbl", &[0; 8]);
        let minf = box_of("minf", &inner);
        let mdia = box_of("mdia", &minf);
        let trak = box_of("trak", &mdia);
        let moov = box_of("moov", &trak);
        let data = moov;

        let found = find_box(&data, 0, data.len(), "stbl").expect("应能递归找到");
        assert_eq!(found.size, 8);
    }

    #[test]
    fn find_box_returns_none_when_absent() {
        let data = box_of("ftyp", &[0; 8]);
        assert!(find_box(&data, 0, data.len(), "moov").is_none());
    }

    #[test]
    fn roundtrip_build_and_parse() {
        for size in [0usize, 1, 100, 5000] {
            let payload = vec![0xabu8; size];
            let data = box_of("test", &payload);
            let boxes = parse_boxes(&data, 0, data.len());
            assert_eq!(boxes.len(), 1, "size = {size}");
            assert_eq!(boxes[0].size, size);
        }
    }

    #[test]
    fn empty_input_yields_no_boxes() {
        assert!(parse_boxes(&[], 0, 0).is_empty());
    }
}
