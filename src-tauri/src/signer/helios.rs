//! X-Helios：时间戳派生的 34 轮扩散哈希。
//!
//! 一次 Merkle–Damgård 风格的双分支扩散：先把 `le32(random) || APP_ID` 的
//! MD5 摘要拆成 4 个 u64 种子，扩散 34 轮生成 `table`；再用
//! `"<khronos>-<CHANNEL_ID>-<APP_ID>"` 逐块喂进 `table`，每块跑 34 轮出 16 字节。
//!
//! 与 [`super::medusa`] 分文件：两者都内嵌随机数，但扩散结构完全不同，
//! 放一起会让「哪段算错了」很难定位。

use std::collections::VecDeque;

use base64::Engine;

use crate::signer::medusa::rand32;
use crate::signer::primitives::{le32, md5_raw, ror64};
use crate::signer::{APP_ID, CHANNEL_ID};

/// 扩散轮数。
const ROUNDS: usize = 34;

/// 构造 x-helios 头。
pub fn helios(khronos: u32) -> String {
    assemble_helios(khronos, rand32())
}

/// 用给定随机数装配 x-helios。
///
/// 抽出来是为了让测试能喂固定值、和现版 JS 的输出逐字节对齐——
/// 这里的随机数内嵌在结果里，不固定就没法比对。
pub fn assemble_helios(khronos: u32, random: u32) -> String {
    let mut seed_input = Vec::with_capacity(4 + APP_ID.len());
    seed_input.extend_from_slice(&le32(random));
    seed_input.extend_from_slice(APP_ID.as_bytes());
    let digest = md5_raw(&seed_input);
    // 注意：取的是摘要的**十六进制文本**的字节，不是摘要本身（与现版一致）
    let ascii = hex::encode(digest).into_bytes();

    let mut words: Vec<u64> = (0..4)
        .map(|i| {
            let mut b = [0u8; 8];
            b.copy_from_slice(&ascii[i * 8..i * 8 + 8]);
            u64::from_le_bytes(b)
        })
        .collect();

    let table = diffuse(&mut words);

    let mut output: Vec<u8> = Vec::new();
    for chunk in pad(&khronos).chunks(16) {
        let mut a = u64::from_le_bytes(chunk[..8].try_into().expect("8 字节"));
        let mut b = u64::from_le_bytes(chunk[8..].try_into().expect("8 字节"));
        // 只取前 34 项：`diffuse` 会算出 35 个，现版同样只用前 34 个
        for t in table.iter().take(ROUNDS) {
            b = t ^ a.wrapping_add(ror64(b, 8));
            a = b ^ ror64(a, 61);
        }
        output.extend_from_slice(&a.to_le_bytes());
        output.extend_from_slice(&b.to_le_bytes());
    }

    let mut result = le32(random).to_vec();
    result.extend_from_slice(&output);
    base64::engine::general_purpose::STANDARD.encode(result)
}

/// 34 轮扩散，产出每块要用的 34 个 u64。
///
/// ⚠️ 队列装的是**去掉前两个之后剩下的** w2 / w3。
///    写成 `drain(0..2)` 拿到的是被摘掉的 w0 / w1，正好错位一轮，
///    表现为 x-helios 从第 4 个字节起与服务端对不上。
fn diffuse(words: &mut Vec<u64>) -> Vec<u64> {
    let mut table = vec![words[0]];
    let mut b0 = words[0];
    let mut b8 = words[1];
    let mut queue: VecDeque<u64> = words.split_off(2).into();

    for i in 0..ROUNDS as u64 {
        let mut x8 = ror64(b8, 8).wrapping_add(b0);
        x8 ^= i;
        queue.push_back(x8);
        x8 ^= ror64(b0, 61);
        table.push(x8);
        b0 = x8;
        b8 = queue.pop_front().expect("队列非空");
    }
    table
}

/// 把 `khronos-CHANNEL_ID-APP_ID` 补齐到 16 的倍数。
///
/// 填充字节取 `16 - (len % 16)`，所以长度刚好是 16 倍数时会填 0x10——
/// 这是现版的做法，不要「顺手修正」成 1。
fn pad(khronos: &u32) -> Vec<u8> {
    let text = format!("{khronos}-{CHANNEL_ID}-{APP_ID}").into_bytes();
    let mut out = vec![(16 - (text.len() % 16)) as u8; (text.len() + 1).div_ceil(16) * 16];
    out[..text.len()].copy_from_slice(&text);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_base64_of_4_plus_16n_bytes() {
        let out = helios(1_700_000_000);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&out)
            .expect("必须是合法 base64");
        // 4 字节 random + 至少一个 16 字节 block
        assert!(bytes.len() >= 20);
        assert_eq!(bytes.len() % 4, 0);
    }

    #[test]
    fn varies_per_call() {
        // 每次带随机数，输出应不同
        assert_ne!(helios(1_700_000_000), helios(1_700_000_000));
    }

    #[test]
    fn pad_fills_to_16_boundary() {
        // "1700000000-1588093228-8662" 是 26 字节 → 补到 32
        let p = pad(&1_700_000_000);
        assert_eq!(p.len(), 32);
        assert_eq!(&p[..26], b"1700000000-1588093228-8662");
        assert!(p[26..].iter().all(|b| *b == 6));
    }

    #[test]
    fn matches_js_golden_vector() {
        // JS: helios(1700000000)，random = 固定 LCG 的第 6 项。
        // 队列错位（drain vs split_off）会从这里开始分叉。
        let got = assemble_helios(1_700_000_000, 2_028_160_147);
        assert_eq!(
            got, "k0TjeBWFFnRXfkeKlh3Hmx1jLjrfhgCasqsF4XvB1+0eB1Mw",
            "x-helios 与现版不一致"
        );
    }
}
