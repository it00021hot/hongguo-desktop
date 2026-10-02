//! Medusa 的 AES 变体。
//!
//! 与标准 AES 的差异集中在三处：
//! - S-Box 表按 khronos 低 2 位整体平移（从 4 张表里取一段）
//! - 密钥扩展第 4/8/12 轮用改写的字节选择
//! - 加密前后各有一层手写的位打包/位解包，不是标准的 ShiftRows
//!
//! 所以不能直接换成 `aes` crate 的标准实现。

use crate::signer::constants::sbox;
use crate::signer::primitives::xor_bytes;

/// AES 变体按 khronos 低 2 位选择的参数组。
const CON2: [[usize; 4]; 4] = [[1, 0, 2, 3], [2, 0, 3, 1], [0, 1, 3, 2], [1, 0, 2, 3]];

const ORDER: [[usize; 16]; 4] = [
    [0, 9, 14, 11, 4, 13, 2, 7, 8, 1, 6, 15, 12, 5, 10, 3],
    [0, 9, 14, 15, 4, 13, 2, 7, 8, 1, 6, 3, 12, 5, 10, 11],
    [0, 9, 14, 7, 4, 13, 2, 11, 8, 1, 6, 3, 12, 5, 10, 15],
    [0, 9, 14, 11, 4, 13, 2, 7, 8, 1, 6, 15, 12, 5, 10, 3],
];

const INIT_WORD: [u32; 4] = [0xca02_5ddc, 0x823d_c546, 0xc942_0583, 0xc298_225f];

/// 字节流加密用的 AES 变体。
pub struct AesV3 {
    box_: Vec<u8>,
    con2: [usize; 4],
    order: [usize; 16],
    /// 12 组 4 字节轮密钥
    keys: [[u8; 4]; 12],
}

impl AesV3 {
    /// 用 key 与 khronos 构造一组参数。
    pub fn new(key: &[u8], khronos: u32) -> Self {
        let word_size = (khronos & 3) as usize;
        let all = sbox();
        let start = word_size * 256;
        let box_ = all[start..start + 256].to_vec();

        Self {
            box_,
            con2: CON2[word_size],
            order: ORDER[word_size],
            keys: Self::expand(key, word_size),
        }
    }

    /// 密钥扩展：12 组 4 字节轮密钥。
    fn expand(key: &[u8], word_size: usize) -> [[u8; 4]; 12] {
        let init = INIT_WORD[word_size];
        let sbox_ = {
            let all = sbox();
            let start = word_size * 256;
            all[start..start + 256].to_vec()
        };

        // JS: 4 个槽位写的都是同一个 init（writeUInt32LE(init, i * 4)）
        let mut initial = Vec::with_capacity(16);
        for _ in 0..4 {
            initial.extend_from_slice(&init.to_le_bytes());
        }

        let mut expanded = xor_bytes(&initial, key);
        expanded.extend_from_slice(&[0u8; 32]);

        let mut rounds: u32 = 8;
        for i in 4..12usize {
            let at = 4 * (i - 1);
            let mut a = expanded[at];
            let mut b = expanded[at + 1];
            let mut c = expanded[at + 2];
            let mut d = expanded[at + 3];

            if (i & 3) == 0 {
                let t = ((init >> (rounds & 24)) ^ sbox_[b as usize] as u32) & 255;
                b = sbox_[c as usize];
                c = sbox_[d as usize];
                d = sbox_[a as usize];
                a = t as u8;
            }
            rounds += 2;

            expanded[at + 4] = a ^ expanded[at - 12];
            expanded[at + 5] = b ^ expanded[at - 11];
            expanded[at + 6] = c ^ expanded[at - 10];
            expanded[at + 7] = d ^ expanded[at - 9];
        }

        let mut keys = [[0u8; 4]; 12];
        for (i, item) in keys.iter_mut().enumerate() {
            item.copy_from_slice(&expanded[i * 4..i * 4 + 4]);
        }
        keys
    }

    fn add_key(state: &mut [[u8; 4]; 4], key: &[[u8; 4]]) {
        for i in 0..4 {
            for j in 0..4 {
                state[i][j] ^= key[i][j];
            }
        }
    }

    fn add_con(state: &mut [[u8; 4]; 4], keys: &[[u8; 4]], con2: &[usize; 4]) {
        for i in 0..4 {
            for j in 0..4 {
                state[i][j] ^= keys[i][con2[j]];
            }
        }
    }
    fn sub(&self, state: &mut [[u8; 4]; 4]) {
        for row in state.iter_mut() {
            for cell in row.iter_mut() {
                *cell = self.box_[*cell as usize];
            }
        }
        let old = *state;
        for i in 0..4 {
            state[i] = old[self.con2[i]];
        }
    }

    fn shift(&self, state: &mut [[u8; 4]; 4]) {
        let mut flat = [0u8; 16];
        for i in 0..4 {
            for j in 0..4 {
                flat[i * 4 + j] = state[i][j];
            }
        }
        for i in 0..16usize {
            state[i / 4][i % 4] = flat[self.order[i]];
        }
    }

    fn shift_con(state: &mut [[u8; 4]; 4], con2: &[usize; 4]) {
        let old = *state;
        for i in 0..4 {
            for j in 0..4 {
                state[i][j] = old[i][con2[j]];
            }
        }
    }

    /// 单个 16 字节块。
    fn block(&self, value: &[u8]) -> Vec<u8> {
        let mut state = [[0u8; 4]; 4];
        for i in 0..4 {
            state[i].copy_from_slice(&value[i * 4..i * 4 + 4]);
        }

        Self::add_con(&mut state, &self.keys[0..4], &self.con2);
        for i in 1..3usize {
            self.sub(&mut state);
            self.shift(&mut state);
            if i == 1 {
                Self::shift_con(&mut state, &self.con2);
                mix_columns(&mut state);
            }
            Self::add_con(&mut state, &self.keys[i * 4..i * 4 + 4], &self.con2);
        }
        Self::add_key(&mut state, &self.keys[4..8]);

        let mut out = Vec::with_capacity(16);
        for row in state.iter() {
            out.extend_from_slice(row);
        }
        out
    }

    /// 加密任意长度输入。
    ///
    /// 先把源数据按 8 字节一组做位重排压成 32 字节，末尾补 0x01 标记，
    /// 再按 16 字节分块 CBC 加密（IV 来自调用方，且每块用上一块密文）。
    /// 最后把派生出的 32 字节 keystream 反向掩码回源数据的保留位。
    pub fn encrypt(&self, data: &[u8], iv: &[u8]) -> Vec<u8> {
        let source = data.to_vec();

        // 位打包：每 8 字节输入压成 1 字节有效负载
        let mut plaintext = [0u8; 32];
        for (i, item) in plaintext.iter_mut().enumerate().take(31) {
            let at = i * 8;
            let n0 = (source[at] >> 4) & 2;
            let n1 = n0 | (source[at + 1] & 64);
            let n2 = n1 | ((source[at + 2] >> 2) & 1);
            let n3 = n2 | ((source[at + 3] << 3) & 128);
            let n4 = n3 | ((source[at + 4] >> 1) & 4);
            let n5 = n4 | ((source[at + 5] << 3) & 16);
            let n6 = n5 | ((source[at + 6] << 5) & 32);
            *item = n6 | ((source[at + 7] >> 4) & 8);
        }
        plaintext[31] = 1;

        let mut blocks: Vec<u8> = Vec::with_capacity(32);
        let mut previous = iv.to_vec();
        for chunk in plaintext.chunks(16) {
            let value = self.block(&xor_bytes(chunk, &previous));
            blocks.extend_from_slice(&value);
            previous = value;
        }

        // 把 keystream 的字节拆回源数据的保留位
        let mut out = source;
        for i in 0..31usize {
            let at = i * 8;
            let k = blocks[i];
            out[at] &= 0xdf;
            out[at] |= (k << 4) & 32;
            out[at + 1] &= 0xbf;
            out[at + 1] |= k & 64;
            out[at + 2] &= 0xfb;
            out[at + 2] |= (k << 2) & 4;
            out[at + 3] &= 0xef;
            out[at + 3] |= (k >> 3) & 16;
            out[at + 4] &= 0xf7;
            out[at + 4] |= k.wrapping_add(k) & 8;
            out[at + 5] &= 0xfd;
            out[at + 5] |= (k >> 3) & 2;
            out[at + 6] &= 0xfe;
            out[at + 6] |= (k >> 5) & 1;
            out[at + 7] &= 0x7f;
            out[at + 7] |= (k << 4) & 128;
        }

        let mut result = Vec::with_capacity(1 + out.len());
        result.push(blocks[31]); // key.subarray(-1)
        result.extend_from_slice(&out);
        result
    }
}

/// GF(2^8) 混合列变换。
fn mix_columns(state: &mut [[u8; 4]; 4]) {
    for i in 0..4 {
        let t = state[0][i] ^ state[1][i] ^ state[2][i] ^ state[3][i];
        let u = state[0][i];
        let xt = |x: u8| ((x << 1) ^ if x & 128 != 0 { 0x1b } else { 0 }) & 255;

        state[0][i] ^= t ^ xt(state[0][i] ^ state[1][i]);
        state[1][i] ^= t ^ xt(state[1][i] ^ state[2][i]);
        state[2][i] ^= t ^ xt(state[2][i] ^ state[3][i]);
        state[3][i] ^= t ^ xt(state[3][i] ^ u);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_requires_248_bytes_of_source() {
        // JS 的位打包固定读 31*8 = 248 字节，调用方保证输入长度
        let key = crate::signer::constants::medusa_aes_key();
        let iv = crate::signer::constants::medusa_aes_iv();
        let aes = AesV3::new(&key, 1_700_000_000);
        let out = aes.encrypt(&[0u8; 248], &iv);
        // 1 字节 keystream 尾 + 248 字节掩码回写
        assert_eq!(out.len(), 249);
    }

    #[test]
    fn different_khronos_gives_different_ciphertext() {
        let key = crate::signer::constants::medusa_aes_key();
        let iv = crate::signer::constants::medusa_aes_iv();
        let data = [7u8; 248];
        let a = AesV3::new(&key, 1_700_000_000).encrypt(&data, &iv);
        let b = AesV3::new(&key, 1_700_000_001).encrypt(&data, &iv);
        assert_ne!(a, b, "khronos 低 2 位应切换 S-Box 组");
    }

    #[test]
    fn encrypt_is_deterministic() {
        let key = crate::signer::constants::medusa_aes_key();
        let iv = crate::signer::constants::medusa_aes_iv();
        let data = [0xa5u8; 248];
        let aes = AesV3::new(&key, 1_700_000_000);
        assert_eq!(aes.encrypt(&data, &iv), aes.encrypt(&data, &iv));
    }
}
