//! `avBase64Decode`：平台自定义的 base64 变体，解码前需要先做字符重映射。
//! 字符表错了会解出完全不同的字节，且不会报错——静默产生错误密钥。

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
