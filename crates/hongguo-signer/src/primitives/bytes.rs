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
