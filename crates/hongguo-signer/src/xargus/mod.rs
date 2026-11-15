//! X-Argus 用的自研哈希（f13）：分支判定与统一入口，JS 1:1 移植禁改写法。
//! 服务端按 query / body / 时间戳推导出 3 个分支之一，算错分支等于没签名：
//! `branch == 0` 重量级路径（[`branch0`]）、`branch == 2` 变种 MD5（[`branch2`]）、其它直通。

pub mod branch0;
pub mod branch2;

use crate::primitives::{get_iv, le32, sum_md5};

pub use branch0::compute as branch0_f13;
pub use branch2::compute as branch2_f13;

/// 判定给定请求落在哪个分支（0 / 1 / 2）：只需 query 串、body 摘要字节和时间戳字节，可发请求前预判。
/// 返回 `1` 表示**服务端不受理**（ticket 抖动要避开）。
pub fn branch_of(query: &[u8], body_md5: &[u8], ts_bytes: &[u8]) -> u32 {
    let iv = get_iv(get_iv(get_iv(0x2023_0928, query), body_md5), ts_bytes);
    let low = iv & 15;
    low - ((low * 171) >> 9) * 3
}

/// 计算 f13 摘要（x-argus 头的负载）：`query_sm3` 为 query 的 SM3 摘要，`body_md5` GET 传 16 个 0，`ts_bytes` 为小端 4 字节 khronos。
/// 返回长度随分支而异（branch 0/2 为 20 字节，直通为 36 字节 = sm3 前 16 + body 摘要 + 校验和 4）——算法本身的设计，不要强行统一。
pub fn hash_f13(query_sm3: &[u8], body_md5: &[u8], ts_bytes: &[u8], khronos: u32) -> Vec<u8> {
    let iv = get_iv(get_iv(get_iv(0x2023_0928, query_sm3), body_md5), ts_bytes);
    let low = iv & 15;
    let iv_v0 = (low * 171) >> 9;
    let branch = low - iv_v0 * 3;

    if branch == 2 {
        return branch2_f13(iv, query_sm3, body_md5, ts_bytes, khronos);
    }

    if branch != 0 {
        // 直通：sm3 前 16 + body 摘要 + 自定义校验和
        let mut out = Vec::with_capacity(36);
        out.extend_from_slice(&query_sm3[0..16]);
        out.extend_from_slice(body_md5);
        out.extend_from_slice(&le32(sum_md5(&[query_sm3, body_md5].concat())));
        return out;
    }

    branch0_f13(iv, query_sm3, body_md5, ts_bytes, khronos, iv_v0)
}
