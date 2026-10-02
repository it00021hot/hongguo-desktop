//! X-Argus f13 的分支 2：变种 MD5 路径。
//!
//! 四组不同的非线性函数 / 移位表依次跑 16 轮，末尾附一个自定义累加和。

use crate::signer::constants::{branch2_orders, BRANCH2_SV};
use crate::signer::primitives::{le32, rol32, ror32, sum_md5, u32};

fn md5_v3_step(
    kind: u32,
    a: u32,
    b: u32,
    c: u32,
    d: u32,
    m: u32,
    shift: u32,
    constant: u32,
) -> u32 {
    let f = match kind {
        0 => (b & c) | (!b & d),
        1 => (b & d) | (c & !d),
        2 => b ^ c ^ d,
        _ => c ^ (b | !d),
    };
    u32((b as u64).wrapping_add(rol32(
        u32((a as u64)
            .wrapping_add(f as u64)
            .wrapping_add(m as u64)
            .wrapping_add(constant as u64)),
        shift,
    ) as u64))
}

/// 变种 MD5：四组不同非线性函数 / 移位表依次跑 16 轮，末尾附加一个自定义累加和。
fn md5_sum_v3(message: &[u8], count_v2: u32, orders: &[u8], count_v1: u32) -> Vec<u8> {
    let sv: Vec<u32> = BRANCH2_SV.iter().map(|&v| ror32(v, count_v1)).collect();
    let start = [
        ror32(0x79e0_f2fb, count_v2),
        ror32(0xc8b5_2570, count_v2),
        ror32(0xebc2_f8cd, count_v2),
        ror32(0x7c10_4d93, count_v2),
    ];
    let end_count = (count_v2 + 6) & 255;
    let end = [
        ror32(0x19be_4866, end_count),
        ror32(0xe859_86b4, end_count),
        ror32(0xe19b_326e, end_count),
        ror32(0x71d1_d7d4, end_count),
    ];

    let mut m = [0u32; 16];
    for (i, item) in m.iter_mut().enumerate() {
        *item = u32::from_le_bytes([
            message[i * 4],
            message[i * 4 + 1],
            message[i * 4 + 2],
            message[i * 4 + 3],
        ]);
    }

    let (mut a, mut b, mut c, mut d) = (start[0], start[1], start[2], start[3]);

    let run = |kind: u32,
               shifts: [u32; 16],
               offset: usize,
               a: &mut u32,
               b: &mut u32,
               c: &mut u32,
               d: &mut u32| {
        for i in 0..16usize {
            let phase = i & 3;
            let w = m[orders[offset + i] as usize];
            let k = sv[offset + i];
            let next = match phase {
                0 => md5_v3_step(kind, *a, *b, *c, *d, w, shifts[i], k),
                1 => md5_v3_step(kind, *d, *a, *b, *c, w, shifts[i], k),
                2 => md5_v3_step(kind, *c, *d, *a, *b, w, shifts[i], k),
                _ => md5_v3_step(kind, *b, *c, *d, *a, w, shifts[i], k),
            };
            match phase {
                0 => *a = next,
                1 => *d = next,
                2 => *c = next,
                _ => *b = next,
            }
        }
    };

    run(
        0,
        [7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22],
        0,
        &mut a,
        &mut b,
        &mut c,
        &mut d,
    );
    run(
        1,
        [5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20],
        16,
        &mut a,
        &mut b,
        &mut c,
        &mut d,
    );
    run(
        2,
        [4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23],
        32,
        &mut a,
        &mut b,
        &mut c,
        &mut d,
    );
    run(
        3,
        [6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21],
        48,
        &mut a,
        &mut b,
        &mut c,
        &mut d,
    );

    let mut ret = Vec::with_capacity(20);
    ret.extend_from_slice(
        &(u32((start[0] as u64).wrapping_add(a as u64) ^ end[0] as u64)).to_le_bytes(),
    );
    ret.extend_from_slice(
        &(u32((start[1] as u64).wrapping_add(b as u64) ^ end[1] as u64)).to_le_bytes(),
    );
    ret.extend_from_slice(
        &(u32((start[2] as u64).wrapping_add(c as u64) ^ end[2] as u64)).to_le_bytes(),
    );
    ret.extend_from_slice(
        &(u32((start[3] as u64).wrapping_add(d as u64) ^ end[3] as u64)).to_le_bytes(),
    );
    ret.extend_from_slice(&le32(sum_md5(&ret)));
    ret
}

fn branch2_f13(
    iv: u32,
    query_sm3: &[u8],
    body_md5: &[u8],
    ts_bytes: &[u8],
    khronos: u32,
) -> Vec<u8> {
    let v = (iv & 13) * 86;
    let iv_v0 = ((v >> 15) & 255) + ((v >> 8) & 255);
    const N1: [u32; 5] = [
        0x8980_f29b,
        0xeb54_9c7f,
        0xb087_26db,
        0xd40c_b5e6,
        0xe8f5_59e4,
    ];
    let n1 = N1[(iv_v0 as usize).min(N1.len() - 1)];

    let count_v1 = (n1.wrapping_add(khronos).wrapping_add(1)) & 255;
    let count_v2 = u32(n1 as u64 + khronos as u64);
    let shift = (count_v2 + 5) & 7;

    let seed = [0x84u8, 0x96, 0x77, 0x9d, 0xd4, 0x15, 0x0b, 0xf8];
    let pad: Vec<u8> = seed
        .iter()
        .map(|&v| {
            // JS: (value | (value << 8)) >> shift & 255
            // 先在 u16 里拼成双字节再右移，避免 u8 左移溢出
            let combined = u16::from(v) | (u16::from(v) << 8);
            ((combined >> shift) & 255) as u8
        })
        .collect();

    let mut input = Vec::new();
    input.extend_from_slice(query_sm3);
    input.extend_from_slice(body_md5);
    input.extend_from_slice(ts_bytes);
    input.extend_from_slice(&pad);
    input.extend_from_slice(&hex::decode("a0010000").expect("固定常量"));

    let all_orders = branch2_orders();
    let begin = (iv_v0 as usize) << 6;
    let orders = &all_orders[begin..begin + 64];

    md5_sum_v3(&input, count_v2, orders, count_v1)
}

/// 供上层 `hash_f13` 调用。
pub fn compute(
    iv: u32,
    query_sm3: &[u8],
    body_md5: &[u8],
    ts_bytes: &[u8],
    khronos: u32,
) -> Vec<u8> {
    branch2_f13(iv, query_sm3, body_md5, ts_bytes, khronos)
}
