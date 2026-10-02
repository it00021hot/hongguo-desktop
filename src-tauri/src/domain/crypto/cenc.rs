//! CENC-AES-CTR 流式解密。
//!
//! 与现版 JS 完全一致的实现方式：每个 16 字节块用
//! `IV 前 8 字节 || 块序号(8 字节大端)` 作为 AES-128-ECB 的输入，
//! 加密结果即该块的 keystream，与密文异或。
//!
//! ⚠️ 平台封装时 `senc` 里的 IV 只有 **8 字节**（不是规范里的 16）。
//!    所以 [`new_decryptor`] 收 8 字节，块计数器从 0 重新起算——
//!    沿用 IV 的后半段会让 keystream 整体偏移，解出来全是噪声。

use aes::cipher::{KeyIvInit, StreamCipher};
use aes::Aes128;
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

        // 用同一 IV 从头再跑一次即还原
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
        // 不应 panic，且产生了非零输出
        assert!(buf.iter().any(|b| *b != 0));
    }
}
