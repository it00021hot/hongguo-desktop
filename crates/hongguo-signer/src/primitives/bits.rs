//! 字节 / 位运算原语：X-Gorgon、X-Argus、Medusa 三个签名的公共底座，JS 1:1 移植禁改写法。
//! 所有函数保持与原始 JS 完全一致的位宽与环绕语义，不要用 `wrapping_*` 之外的方式简化——会改 32 位溢出行为。

/// 归一为无符号 32 位。
#[inline]
pub fn u32(v: impl Into<u64>) -> u32 {
    v.into() as u32
}

/// 32 位循环左移。
#[inline]
pub fn rol32(value: u32, count: u32) -> u32 {
    let n = count & 31;
    let x = value;
    if n == 0 {
        return x;
    }
    (x << n) | (x >> (32 - n))
}

/// 32 位循环右移。
#[inline]
pub fn ror32(value: u32, count: u32) -> u32 {
    let n = count & 31;
    let x = value;
    if n == 0 {
        return x;
    }
    (x >> n) | (x << (32 - n))
}

/// 64 位循环右移（模 2^64）。
#[inline]
pub fn ror64(value: u64, count: u64) -> u64 {
    let n = count & 63;
    if n == 0 {
        return value;
    }
    (value >> n) | (value << (64 - n))
}

/// ZigZag 编码（protobuf sint32）；JS 版用 BigInt，此处对 `i64` 直接运算，语义等价。
#[inline]
pub fn zigzag(value: i64) -> i64 {
    if value < 0 {
        // -n * 2 - 1：i64::MIN 取绝对值会溢出，wrapping 保持与 JS BigInt 相同的位模式
        value.wrapping_neg().wrapping_mul(2).wrapping_sub(1)
    } else {
        value.wrapping_mul(2)
    }
}

/// 8 位比特序反转，对应 JS 的 `reverseBits(value)`。
#[inline]
pub fn reverse_bits(value: u8) -> u8 {
    let mut result: u8 = 0;
    for i in 0..8 {
        result = (result << 1) | ((value >> i) & 1);
    }
    result
}
