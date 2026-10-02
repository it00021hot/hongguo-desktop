//! 密钥派生：`spade_a` → AES-128。
//!
//! 平台返回的 `spade_a` 是一段自定义编码的字符串：先按变体 base64 解码，
//! 再用前 3 字节算出的两个长度参数取出子串做逐字节变换，最后从变换结果里
//! 读出一个 32 位的十六进制串（即 16 字节密钥）。
//!
//! ⚠️ 任何一步都不能改。**错一个字节不会报错**，只会让整集解密成噪声——
//!    MP4 结构仍然合法、播放器仍然能打开，但画面是雪花。
//!    唯一的信号是「解出来的样本不是合法 NAL」。

use super::av_base64;

use crate::error::{AppError, AppResult};

/// 逐字节变换用的三个初值。
const W10: u8 = 0xeb;
const W11_INIT: u8 = 0x55;
const W12_INIT: u8 = 0xfa;

/// 从 `spade_a` 派生 AES-128 密钥。
///
/// `spade_a` 是接口原样返回的字符串字节，不是解码后的内容。
pub fn derive_key(spade_a: &[u8]) -> AppResult<[u8; 16]> {
    if spade_a.is_empty() {
        return Err(AppError::Decrypt("密钥材料为空".into()));
    }
    let raw = av_base64::decode(spade_a);
    let len = raw.len();
    if len < 3 {
        return Err(AppError::Decrypt("密钥材料解码后太短".into()));
    }

    let key0 = raw[0] ^ raw[1] ^ raw[2];
    let x27 = i32::from(key0) - 0x30;
    let w22 = len as i32 - i32::from(key0) + 0x2f;
    if x27 < 2 || w22 < 2 || w22 > len as i32 - 1 {
        return Err(AppError::Decrypt(format!(
            "密钥材料长度参数非法（x27={x27} w22={w22} len={len}）"
        )));
    }

    // 逐字节变换：偶数位用 w12、奇数位用 w11，取变换前的原值异或
    let w22u = w22 as usize;
    let mut x21: Vec<u8> = raw[1..1 + w22u].to_vec();
    let (mut w11, mut w12) = (W11_INIT, W12_INIT);
    for (i, slot) in x21.iter_mut().enumerate() {
        let w13 = *slot;
        let pc = (i as u32).count_ones() as u8;
        let w14 = if i & 1 == 0 { w12 } else { w11 };
        if i & 1 == 1 {
            w11 = w13;
        } else {
            w12 = w13;
        }
        *slot = (W10.wrapping_sub(pc)).wrapping_add(w14 ^ w13);
    }

    // 首字节给出要取的十六进制串长度
    let c0 = x21[0];
    let hval = match c0 {
        0x30..=0x39 => c0 - 0x30,
        0x61..=0x7a => c0 - 0x57,
        _ => return Err(AppError::Decrypt("密钥材料首字节非法".into())),
    } as usize;
    let w9 = w22 - hval as i32;
    if w9 < 2 {
        return Err(AppError::Decrypt("密钥材料有效长度不足".into()));
    }

    let hex_part: String = x21[1..w9 as usize]
        .iter()
        .map(|b| *b as char)
        .collect::<String>();
    if hex_part.len() != 32 {
        return Err(AppError::Decrypt(format!(
            "密钥十六进制串长度应为 32，实际 {}",
            hex_part.len()
        )));
    }

    let mut key = [0u8; 16];
    for (i, b) in key.iter_mut().enumerate() {
        let hi = (hex_part.as_bytes()[i * 2] as char)
            .to_digit(16)
            .ok_or_else(|| AppError::Decrypt("密钥含非十六进制字符".into()))?;
        let lo = (hex_part.as_bytes()[i * 2 + 1] as char)
            .to_digit(16)
            .ok_or_else(|| AppError::Decrypt("密钥含非十六进制字符".into()))?;
        *b = (hi * 16 + lo) as u8;
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_key_rejects_empty_and_short_material() {
        assert!(derive_key(&[]).is_err());
        // 变体 base64 只有 1 个有效字符时解出 0 字节，必须报错而不是补零
        assert!(derive_key(b"A").is_err());
    }

    /// 黄金向量：取自现版 JS 对同一 `spade_a` 的输出。
    ///
    /// ⚠️ 这条向量曾经**被一个错误实现"锁死"过**：当时的 `derive_key` 只是
    ///    base64 解码后取前 16 字节，测试还断言「不足补 0」——于是所有下载
    ///    产物都是噪声，MP4 结构却合法，直到实机播放才发现。
    ///    密钥派生错位不会抛任何错，只能靠这类向量拦住。
    #[test]
    fn derive_key_matches_js_golden_vectors() {
        let cases: &[(&str, &str)] = &[(
            "kLwey2WDLs9ViRL1W48W9lm6HvZRux/wVqAt8GC8Ku1kpCy5uQ==",
            "5ad245c00e1c5a6605766309c8441843",
        )];
        for (spade, want) in cases {
            let key =
                derive_key(spade.as_bytes()).unwrap_or_else(|e| panic!("{spade} 应能派生: {e}"));
            assert_eq!(hex_of(&key), *want, "spade = {spade}");
        }
    }

    fn hex_of(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
}
