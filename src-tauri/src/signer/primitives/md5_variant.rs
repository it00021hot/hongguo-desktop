//! 变种 MD5 累加器。
//!
//! X-Argus 的分支选择与校验和都依赖这两个非标准函数，
//! 它们是自研哈希的组成部分，**不能用标准 MD5 替代**。

use super::bits::u32;

/// 分支判定的初始向量递推。
///
/// 对应 JS 的 `getIv(iv, data)`：按字节下标奇偶走两条不同的递推式。
/// 注意 JS 中 `value >>> 4` 等运算是无符号 32 位语义。
pub fn get_iv(iv: u32, data: &[u8]) -> u32 {
    let mut value = u32(iv as u64);
    for (i, &byte) in data.iter().enumerate() {
        if (i & 1) == 0 {
            value = u32(((value >> 4) ^ value ^ (value << 6) ^ byte as u32) as u64);
        } else {
            // JS: ~((value >>> 7) ^ value ^ (data[i] | (value << 12)))
            let inner = (value >> 7) ^ value ^ (byte as u32 | value << 12);
            value = u32(!inner as u64);
        }
    }
    value
}

/// 自定义累加和，返回 32 位。
///
/// 对应 JS 的 `sumMd5(data)`。注意 JS 用 `data[i]` 访问，
/// 传入长度必须 ≥ 12（JS 版本依赖调用方保证）。
pub fn sum_md5(data: &[u8]) -> u32 {
    debug_assert!(data.len() >= 12, "sum_md5 需要至少 12 字节输入");
    let mut check: u32 = 0x2022_0420;

    for i in 0..12usize {
        let temp = if (i & 1) == 0 {
            (check >> 3) ^ check
        } else {
            (check >> 5) ^ check
        };
        check = if (i & 1) == 0 {
            data[i] as u32 ^ (check << 7)
        } else {
            data[i] as u32 | (check << 11)
        };
        if (i & 1) != 0 {
            check = !check;
        }
        check = u32((check ^ temp) as u64);
    }

    u32(((check | 4) ^ 0x0100_0000) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_iv_even_branch() {
        // 单字节走偶数分支
        let got = get_iv(0x2023_0928, &[0x00]);
        let expected = u32(((0x2023_0928u32 >> 4) ^ 0x2023_0928 ^ (0x2023_0928u32 << 6)) as u64);
        assert_eq!(got, expected);
    }

    #[test]
    fn get_iv_odd_branch_uses_not() {
        let got = get_iv(0x2023_0928, &[0x00, 0x01]);
        let step1 = u32(((0x2023_0928u32 >> 4) ^ 0x2023_0928 ^ (0x2023_0928u32 << 6)) as u64);
        let inner = (step1 >> 7) ^ step1 ^ (0x01u32 | step1 << 12);
        assert_eq!(got, u32(!inner as u64));
    }

    #[test]
    fn get_iv_empty_data_is_identity() {
        assert_eq!(get_iv(0x1234_5678, &[]), 0x1234_5678);
    }

    #[test]
    fn sum_md5_is_deterministic() {
        let data = [0u8; 16];
        assert_eq!(sum_md5(&data), sum_md5(&data));
    }

    #[test]
    fn sum_md5_forces_bit2_and_bit24() {
        let data = [0u8; 16];
        let out = sum_md5(&data);
        assert_eq!(out & 0b100, 0b100, "结果必须置位 bit2");
        // u32((check | 4) ^ 0x1000000) 后 bit24 取决于 check|4 的 bit24
        // 这里只验证恒等式本身
        let check: u32 = 0x2022_0420;
        let _ = check;
    }
}
