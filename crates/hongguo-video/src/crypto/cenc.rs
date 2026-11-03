//! CENC-AES-CTR 流式解密，与现版 JS 完全一致：每个 16 字节块用 `IV 前 8 字节 || 块序号(8 字节大端)`
//! 作为 AES-128-ECB 的输入，加密结果即该块的 keystream，与密文异或。
//! ⚠️ 平台封装 `senc` 里的 IV 只有 **8 字节**（非规范的 16）；[`new_decryptor`] 收 8 字节、块计数器从 0 起算——沿用 IV 后半段会让 keystream 整体偏移。

use aes::Aes128;
use aes::cipher::{KeyIvInit, StreamCipher};
use ctr::Ctr128BE;

type Aes128Ctr = Ctr128BE<Aes128>;

/// 创建一个样本解密器。
///
/// `iv` 是 `senc` 里的 8 字节 IV；低 8 位计数器固定从 0 开始。
pub fn new_decryptor(key: &[u8; 16], iv: &[u8; 8]) -> Aes128Ctr {
    let mut counter_block = [0u8; 16];
    counter_block[..8].copy_from_slice(iv);
    Aes128Ctr::new(key.into(), &counter_block.into())
}

/// 解密单个样本（原地）。
pub fn decrypt_sample(cipher: &mut Aes128Ctr, data: &mut [u8]) {
    cipher.apply_keystream(data);
}

/// 样本内第 `block` 块的计数器块：`IV 前 8 字节 || 块序号(8 字节大端)`，与 [`new_decryptor`] 的初始块自洽（块 0 = 初始块本身）。
fn counter_block(iv: &[u8; 8], block: u64) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(iv);
    b[8..].copy_from_slice(&block.to_be_bytes());
    b
}

/// 从样本内第 `skip` 字节起解密一段（流式播放按 Range 惰性解密用）：CTR 任意子区间可独立解——
/// 先算起点落在第几个 16 字节块、块内偏移多少，再逐块生成 keystream 异或，**不需要样本前缀**。
/// 正确性与 [`decrypt_sample`] 的对齐由测试锁住（skip=0 与整样本逐字节一致，skip=k 等于整样本解密的对应切片）。
pub fn decrypt_range(key: &[u8; 16], iv: &[u8; 8], skip: usize, data: &mut [u8]) {
    use aes::cipher::{BlockCipherEncrypt, KeyInit};
    let cipher = Aes128::new(key.into());
    let mut block = (skip / 16) as u64;
    let lead = skip % 16;

    let mut pos = 0usize;
    if lead > 0 {
        // 首块只用到第 lead 字节之后的部分
        let mut ks = aes::Block::from(counter_block(iv, block));
        cipher.encrypt_block(&mut ks);
        let take = (16 - lead).min(data.len());
        for i in 0..take {
            data[i] ^= ks[lead + i];
        }
        pos = take;
        block += 1;
    }
    while pos < data.len() {
        let mut ks = aes::Block::from(counter_block(iv, block));
        cipher.encrypt_block(&mut ks);
        let take = (data.len() - pos).min(16);
        for i in 0..take {
            data[pos + i] ^= ks[i];
        }
        pos += take;
        block += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypt_twice_is_identity() {
        let key = [0x42u8; 16];
        let iv = [0x24u8; 8];
        let original = b"the quick brown fox jumps!!".to_vec();

        let mut buf = original.clone();
        let mut d = new_decryptor(&key, &iv);
        decrypt_sample(&mut d, &mut buf);
        assert_ne!(buf, original, "应被加密改变");

        let mut d2 = new_decryptor(&key, &iv);
        decrypt_sample(&mut d2, &mut buf);
        assert_eq!(buf, original);
    }

    #[test]
    fn different_iv_gives_different_result() {
        let key = [0x42u8; 16];
        let data = vec![0u8; 32];
        let mut a = data.clone();
        let mut b = data.clone();
        decrypt_sample(&mut new_decryptor(&key, &[0u8; 8]), &mut a);
        decrypt_sample(&mut new_decryptor(&key, &[1u8; 8]), &mut b);
        assert_ne!(a, b);
    }

    #[test]
    fn partial_blocks_are_handled() {
        let key = [1u8; 16];
        let iv = [2u8; 8];
        let mut buf = vec![0u8; 37];
        decrypt_sample(&mut new_decryptor(&key, &iv), &mut buf);
        assert!(buf.iter().any(|b| *b != 0));
    }

    /// 整样本解密一次，作为对照基准。
    fn whole_decrypt(key: &[u8; 16], iv: &[u8; 8], plain: &[u8]) -> Vec<u8> {
        let mut buf = plain.to_vec();
        decrypt_sample(&mut new_decryptor(key, iv), &mut buf);
        buf
    }

    #[test]
    fn range_decrypt_from_zero_matches_whole_sample() {
        // 交叉验证：手工计数器块构造与 Ctr128BE 整样本解密必须逐字节一致，否则流式与落盘路径产出两种明文
        let key = [0x42u8; 16];
        let iv = [0x24u8; 8];
        let plain: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let ciphered = whole_decrypt(&key, &iv, &plain);

        let mut buf = ciphered.clone();
        decrypt_range(&key, &iv, 0, &mut buf);
        assert_eq!(buf, plain, "skip=0 应与整样本解密完全一致");
    }

    #[test]
    fn range_decrypt_from_any_offset_matches_a_slice_of_whole() {
        let key = [7u8; 16];
        let iv = [9u8; 8];
        let plain: Vec<u8> = (0..1234u32).map(|i| (i * 31 % 256) as u8).collect();
        let ciphered = whole_decrypt(&key, &iv, &plain);

        for skip in [1usize, 15, 16, 17, 31, 100, 999, 1233] {
            let mut buf = ciphered[skip..].to_vec();
            decrypt_range(&key, &iv, skip, &mut buf);
            assert_eq!(
                buf,
                plain[skip..],
                "skip={skip} 应等于整样本解密结果的对应切片"
            );
        }
    }

    #[test]
    fn range_decrypt_across_many_blocks() {
        let key = [3u8; 16];
        let iv = [4u8; 8];
        let plain: Vec<u8> = (0..4096u32).map(|i| (i % 199) as u8).collect();
        let ciphered = whole_decrypt(&key, &iv, &plain);

        let mut buf = ciphered[123..3000].to_vec();
        decrypt_range(&key, &iv, 123, &mut buf);
        assert_eq!(buf, plain[123..3000]);
    }

    #[test]
    fn range_decrypt_handles_empty_slice() {
        let mut buf: Vec<u8> = Vec::new();
        decrypt_range(&[0u8; 16], &[0u8; 8], 0, &mut buf);
        assert!(buf.is_empty(), "空区间不应 panic");
    }
}
