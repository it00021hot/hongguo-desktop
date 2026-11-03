//! hex 编解码（protocol 封面缓存与 login 测试共用的成对原语）。

/// 字节序列 → 小写 hex 串。
pub fn encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// hex 串 → 字节序列（大小写均可）。非法输入（奇数长度/非 hex 字符）直接 panic——消费者是测试与内部可信输入，不需要 Result 化。
pub fn decode(hex: &str) -> Vec<u8> {
    assert!(hex.len().is_multiple_of(2), "hex 长度必须为偶数: {hex}");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("非法 hex 字符"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_lowercase_hex() {
        assert_eq!(encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
        assert_eq!(encode(&[0x00, 0x0f]), "000f");
        assert_eq!(encode(&[]), "");
    }

    #[test]
    fn decodes_either_case() {
        assert_eq!(decode("deadbeef"), vec![0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(decode("DEADbeef"), vec![0xde, 0xad, 0xbe, 0xef]);
        assert!(decode("").is_empty());
    }

    #[test]
    fn encode_decode_roundtrip() {
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(&encode(&bytes)), bytes);
    }

    #[test]
    #[should_panic(expected = "偶数")]
    fn decode_rejects_odd_length() {
        decode("abc");
    }

    #[test]
    #[should_panic(expected = "非法 hex 字符")]
    fn decode_rejects_non_hex_chars() {
        decode("zz");
    }
}
