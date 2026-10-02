//! 字节序与缓冲操作原语。

/// 小端 4 字节。
#[inline]
pub fn le32(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

/// 大端 4 字节。
#[inline]
pub fn be32(value: u32) -> [u8; 4] {
    value.to_be_bytes()
}

/// 逐字节异或，长度取较短者。
pub fn bxor(a: &[u8], b: &[u8]) -> Vec<u8> {
    let n = a.len().min(b.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(a[i] ^ b[i]);
    }
    out
}

/// 逐字节异或，长度取 a，密钥循环使用。
pub fn xor_bytes(a: &[u8], b: &[u8]) -> Vec<u8> {
    debug_assert!(!b.is_empty(), "xor_bytes 的密钥不能为空");
    let mut out = a.to_vec();
    for (i, byte) in out.iter_mut().enumerate() {
        *byte ^= b[i % b.len()];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le32_matches_js() {
        assert_eq!(le32(0x1234_5678), [0x78, 0x56, 0x34, 0x12]);
        assert_eq!(be32(0x1234_5678), [0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn bxor_takes_shorter_length() {
        assert_eq!(bxor(&[1, 2, 3], &[0xff, 0xff]), vec![1 ^ 0xff, 2 ^ 0xff]);
        assert!(bxor(&[], &[1]).is_empty());
    }

    #[test]
    fn xor_bytes_cycles_key() {
        // 长度取 a，密钥 2 字节循环：0^aa, 1^bb, 2^aa, 3^bb, 4^aa
        let out = xor_bytes(&[0, 1, 2, 3, 4], &[0xaa, 0xbb]);
        assert_eq!(out, vec![0xaa, 0xba, 0xa8, 0xb8, 0xae]);
    }

    #[test]
    fn u32_normalizes() {
        assert_eq!(super::super::bits::u32(0x1_0000_0000u64), 0);
    }
}
