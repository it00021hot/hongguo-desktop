//! `avBase64Decode`：平台自定义的 base64 变体。
//!
//! 与标准 base64 的字符表不同，解码前需要先做字符重映射。
//! 表错了会解出完全不同的字节，且不会报错——静默产生错误密钥。

/// 解码 `avBase64Decode`。
pub fn decode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;

    for &c in input {
        if c == b'=' {
            break;
        }
        let Some(v) = value_of(c) else {
            continue; // 跳过非法字符
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    out
}

/// 字符 → 6 位值。
fn value_of(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
use base64::Engine as _;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_standard_base64() {
        assert_eq!(decode(b"aGVsbG8="), b"hello");
        assert_eq!(decode(b""), b"");
    }

    #[test]
    fn handles_all_lengths() {
        for n in 0..40usize {
            let data: Vec<u8> = (0..n).map(|i| (i * 7 % 251) as u8).collect();
            let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
            assert_eq!(decode(encoded.as_bytes()), data, "n = {n}");
        }
    }

    #[test]
    fn skips_invalid_chars() {
        assert_eq!(decode(b"aGVs\r\nbG8="), b"hello");
    }

    #[test]
    fn stops_at_padding() {
        assert_eq!(decode(b"aGVsbG8=xYWJj"), b"hello");
    }
}
