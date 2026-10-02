//! X-Argus f13 的分支 0：重量级路径。
//!
//! 112 轮消息扩展 + 变体轮函数。轮数与槽位下标来自固定的「指令表」
//! `BRANCH0_ROUNDS`，**不要改成顺序取用**——那是签名匹配的关键。

use crate::signer::primitives::{bxor, le32, rol32, ror32, sum_md5, u32};

/// 分支 0 的 6 组轮常量与轮数/索引表，按 iv_v0 选择。
const BRANCH0_IV: [u32; 6] = [
    0xc4a7_8580,
    0xb3c0_fd39,
    0xc58c_5686,
    0xc9aa_3ba7,
    0xf5a7_adf2,
    0x963c_2ed1,
];

const BRANCH0_TT: [u32; 64] = [
    0xebb6_4faf,
    0x07aa_dcc2,
    0xcf31_87bf,
    0xe011_38ff,
    0x6d0b_fcff,
    0x5a30_a3be,
    0xb41a_d638,
    0x3418_0eb8,
    0xf233_eb6f,
    0xb1a5_84cc,
    0xccc3_0dc7,
    0x47d1_db51,
    0xd556_53de,
    0x70a8_4fa1,
    0x5747_3c12,
    0xf76f_0288,
    0x2c07_7f0a,
    0xda0d_cad0,
    0xfbb8_6f6c,
    0xfdc4_cf00,
    0x688a_020d,
    0xe676_c6a6,
    0x8cd6_338b,
    0x1a3c_8d0e,
    0xcce8_b06b,
    0x6ad0_ed0b,
    0xa052_2717,
    0xdc71_ac83,
    0x2285_db71,
    0xd5b4_dda6,
    0x736f_8650,
    0x6560_306c,
    0x617c_e2a6,
    0xe423_417e,
    0x0a40_e143,
    0x544e_4032,
    0x88df_fb2a,
    0x716c_1ae0,
    0x4c46_7a88,
    0x05b2_3bb3,
    0xe1d0_b866,
    0xbaa3_dcb8,
    0xae33_74d3,
    0xc338_1a50,
    0x1702_f75b,
    0xfe6d_a368,
    0xf0b4_cf48,
    0x4e0f_fbb8,
    0x72aa_d10d,
    0x26c5_3a3d,
    0xf2bc_e0f6,
    0xb455_7581,
    0x4a25_7fdd,
    0x8c31_82a2,
    0xab0b_3b86,
    0x3d5d_fb14,
    0x4f10_3634,
    0xd37b_52d7,
    0x444e_ff16,
    0xeb0a_33d1,
    0x6ca8_6f6e,
    0x0028_4ba7,
    0x0838_7cfa,
    0x5fb3_7586,
];

const BRANCH0_INIT: [u32; 8] = [
    0x7aba_4fc8,
    0x6716_6507,
    0x6403_fa00,
    0x340f_512f,
    984_304_912,
    3_005_047_866,
    2_874_125_293,
    2_152_413_264,
];

/// 每档的 `[轮数, …8 个操作数下标]`。
///
/// 这些下标是一张固定的「指令表」，决定轮函数读取 state 的哪几个槽位。
const BRANCH0_ROUNDS: [[u32; 11]; 6] = [
    [101, 5, 7, 6, 3, 2, 1, 0, 5, 4, 3],
    [96, 0, 6, 7, 5, 3, 2, 1, 5, 4, 4],
    [96, 7, 6, 2, 1, 4, 0, 5, 4, 3, 5],
    [99, 3, 6, 2, 4, 5, 1, 0, 0, 7, 6],
    [96, 0, 5, 6, 7, 3, 1, 2, 5, 4, 4],
    [100, 2, 0, 3, 5, 4, 6, 7, 2, 1, 5],
];

/// 112 轮消息扩展。
fn expand_words(data: &[u8]) -> Vec<u32> {
    let mut di: Vec<u32> = Vec::new();
    for chunk in data.chunks_exact(4) {
        di.push(u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }

    let mut di0 = di[0];
    for i in 0..112usize {
        let di1 = di[i + 1];
        let di14 = di[i + 14];
        let r1 = rol32(di1, 14) ^ rol32(di1, 25) ^ (di1 >> 3);
        let r2 = rol32(di14, 13) ^ rol32(di14, 15) ^ (di14 >> 10);
        di.push(u32(di0 as u64 + di[i + 9] as u64 + r1 as u64 + r2 as u64));
        di0 = di1;
    }
    di
}

/// 变体轮函数。
///
/// 每一轮从 state 的固定槽位取值做混合，然后整体循环左移一格再写回两个槽位。
/// 下标全部来自 `BRANCH0_ROUNDS`，不要改成顺序取用。
fn mix_rounds(
    mut state: Vec<u32>,
    rounds: &[u32; 11],
    di: &[u32],
    tt: &[u32],
    iv_v1: u32,
) -> Vec<u32> {
    for i in 0..rounds[0] {
        let base = iv_v1.wrapping_add(i);
        let w = di[(base & 127) as usize];

        let idx = |k: usize| rounds[k] as usize;

        let n1 = ((state[idx(3)] ^ state[idx(4)]) & state[idx(1)]) ^ state[idx(3)];
        let n2 = rol32(state[idx(1)], 26) ^ rol32(state[idx(1)], 21) ^ rol32(state[idx(1)], 7);
        let n4 = u32(w as u64
            + n1 as u64
            + n2 as u64
            + tt[(base & 63) as usize] as u64
            + state[idx(5)] as u64);

        let n5 = rol32(state[idx(2)], 30) ^ rol32(state[idx(2)], 19) ^ rol32(state[idx(2)], 10);
        let n6 =
            (state[idx(2)] & state[idx(6)]) | ((state[idx(2)] | state[idx(6)]) & state[idx(7)]);
        let n7 = u32(n5 as u64 + n6 as u64);

        let old = state[idx(9)];
        // JS 的 d.unshift(d.pop())：末尾元素移到开头
        let last = state.pop().expect("state 至少 1 个元素");
        state.insert(0, last);
        state[idx(10)] = u32(n7 as u64 + n4 as u64);
        state[idx(8)] = u32(old as u64 + n4 as u64);
    }
    state
}

fn branch0_f13(
    query_sm3: &[u8],
    body_md5: &[u8],
    ts_bytes: &[u8],
    khronos: u32,
    iv_v0: u32,
) -> Vec<u8> {
    let iv_v1 = BRANCH0_IV[(iv_v0 as usize).min(BRANCH0_IV.len() - 1)];
    let count_v1 = (iv_v1.wrapping_add(khronos).wrapping_add(1)) & 255;
    let count_v2 = u32(iv_v1 as u64 + khronos as u64);

    let tt: Vec<u32> = BRANCH0_TT.iter().map(|&v| ror32(v, count_v1)).collect();

    let n0 = (count_v2 + 2) & 7;
    let seed = [0xfau8, 0x45, 0x61, 0xd7];
    let pad: Vec<u8> = seed
        .iter()
        .map(|&v| {
            // 同 branch2：先在 u16 里拼双字节再右移
            let combined = u16::from(v) | (u16::from(v) << 8);
            ((combined >> n0) & 255) as u8
        })
        .collect();

    let mut data = Vec::new();
    data.extend_from_slice(query_sm3);
    data.extend_from_slice(body_md5);
    data.extend_from_slice(ts_bytes);
    data.extend_from_slice(&pad);
    data.extend_from_slice(&hex::decode("00000000000001a0").expect("固定常量"));

    let di = expand_words(&data);
    let init: Vec<u32> = BRANCH0_INIT
        .iter()
        .map(|&v| ror32(v, count_v2 & 31))
        .collect();
    let d = mix_rounds(
        init.clone(),
        &BRANCH0_ROUNDS[iv_v0 as usize],
        &di,
        &tt,
        iv_v1,
    );

    let mut ret = Vec::with_capacity(32);
    for i in 0..8usize {
        ret.extend_from_slice(&u32(d[i] as u64 + init[i] as u64).to_be_bytes());
    }

    // 高低 16 字节折叠，再附累加和
    let folded = bxor(&ret[0..16], &ret[16..32]);
    ret[0..16].copy_from_slice(&folded);
    let mut out = ret[0..16].to_vec();
    out.extend_from_slice(&le32(sum_md5(&ret[0..16])));
    out
}

/// 供上层 `hash_f13` 调用。
pub fn compute(
    _iv: u32,
    query_sm3: &[u8],
    body_md5: &[u8],
    ts_bytes: &[u8],
    khronos: u32,
    iv_v0: u32,
) -> Vec<u8> {
    branch0_f13(query_sm3, body_md5, ts_bytes, khronos, iv_v0)
}
