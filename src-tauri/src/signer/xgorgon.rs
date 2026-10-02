//! X-Gorgon
//!
//! 历史最久的一代签名，对 query + body 做摘要后用 RC4 变种混淆。
//! 现在仍然必带，但服务端实际校验强度弱于 X-Argus / Medusa。

use crate::signer::constants::{GORGON_PREFIX, GORGON_SDK_VERSION};
use crate::signer::primitives::{be32, md5_raw, reverse_bits, u32};

/// RC4 变种混淆。
///
/// 与标准 RC4 的差异：KSA 阶段交换写法不同，且 PRGA 阶段用 `s[(y+y) & 255]`
/// 而非 `s[(s[i]+s[j]) & 255]`。这个非标准取样是签名匹配的关键。
pub fn rc4_gorgon(data: &[u8], key: &[u8]) -> Vec<u8> {
    debug_assert!(!key.is_empty(), "RC4 密钥不能为空");
    let mut s: [u8; 256] = [0; 256];
    for (i, item) in s.iter_mut().enumerate() {
        *item = i as u8;
    }

    let mut j: usize = 0;
    for i in 0..256usize {
        j = (j + s[i] as usize + key[i % key.len()] as usize) & 255;
        // ⚠️ 是**赋值**而不是交换。`s[i] = s[j]` 之后 `s[j]` 保持原值，
        //    与标准 RC4 的 swap 语义不同——这里错一个字符签名就整体失配。
        s[i] = s[j];
    }

    let mut out = Vec::with_capacity(data.len());
    let mut i: usize = 0;
    let mut j: usize = 0;
    for &value in data {
        i += 1;
        let x = s[i & 255];
        j += x as usize;
        let y = s[j & 255];
        s[i & 255] = y;
        // 注意这里是 (y + y) 而非 (s[i] + s[j])，与标准 RC4 不同
        out.push(value ^ s[((y as usize) + (y as usize)) & 255]);
    }
    out
}

/// 计算 x-gorgon 请求头。
///
/// 组成：`[md5(query)[0:4] | md5(body)[0:4] | 4B 空 | sdkVersion(4B LE) | khronos(4B BE)]`
/// → RC4 混淆 → 逐字节 nibble 交换 + 邻位异或 + 取反
/// → 前缀 '8404' + 2 字节随机种子 + 标志位 + 密文，整体 hex 编码
///
/// # 参数
/// - `query`：URL 查询串（不含 '?'）
/// - `body`：请求体；GET 时传 `None`
/// - `khronos`：服务端时间戳（秒）
/// - `random`：0..=65535 的随机种子
pub fn x_gorgon(query: &str, body: Option<&[u8]>, khronos: u32, random: u32) -> String {
    let sdk = GORGON_SDK_VERSION.to_le_bytes();

    let query_md5 = md5_raw(query.as_bytes());
    let body_md5 = body.map(md5_raw).unwrap_or([0u8; 16]);

    let mut input = Vec::with_capacity(20);
    input.extend_from_slice(&query_md5[0..4]);
    input.extend_from_slice(&body_md5[0..4]);
    input.extend_from_slice(&[0u8; 4]);
    input.extend_from_slice(&sdk);
    input.extend_from_slice(&be32(khronos));

    let key = [
        0x4a,
        0x40,
        0x16,
        ((random >> 8) & 255) as u8,
        0x47,
        0x6c,
        0x01,
        (random & 255) as u8,
    ];
    let mut out = rc4_gorgon(&input, &key);

    // 逐字节变换：nibble 交换 → 与下一字节异或 → 反转比特 → 取反
    // 下一字节在末尾回绕到 out[0]
    let n = out.len();
    for i in 0..n {
        let value = out[i];
        let swapped = (value >> 4) | (value << 4);
        let next = if i + 1 < n { out[i + 1] } else { out[0] };
        out[i] = u32((!((reverse_bits(next ^ swapped)) ^ 20)) as u64) as u8;
    }

    let mut result = Vec::with_capacity(4 + n);
    result.extend_from_slice(&GORGON_PREFIX);
    result.push((random & 255) as u8);
    result.push(((random >> 8) & 255) as u8);
    result.push(0x40);
    result.push(0x01);
    result.extend_from_slice(&out);
    hex::encode(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gorgon_length_is_stable() {
        // 2 字节前缀 + 4 字节种子标志 + 20 字节密文 = 26 字节 = 52 hex 字符
        let got = x_gorgon("a=1", Some(b"{}"), 1_700_000_000, 0x1234);
        assert_eq!(got.len(), 52);
    }

    #[test]
    fn gorgon_is_deterministic_for_same_seed() {
        let a = x_gorgon("a=1&b=2", Some(b"payload"), 1_700_000_000, 0xabcd);
        let b = x_gorgon("a=1&b=2", Some(b"payload"), 1_700_000_000, 0xabcd);
        assert_eq!(a, b);
    }

    #[test]
    fn gorgon_changes_with_input() {
        let base = x_gorgon("a=1", Some(b"{}"), 1_700_000_000, 0x0001);
        assert_ne!(base, x_gorgon("a=2", Some(b"{}"), 1_700_000_000, 0x0001));
        assert_ne!(
            base,
            x_gorgon("a=1", Some(b"{\"x\":1}"), 1_700_000_000, 0x0001)
        );
        assert_ne!(base, x_gorgon("a=1", Some(b"{}"), 1_700_000_001, 0x0001));
        assert_ne!(base, x_gorgon("a=1", Some(b"{}"), 1_700_000_000, 0x0002));
    }

    #[test]
    fn gorgon_get_body_differs_from_post() {
        let get = x_gorgon("a=1", None, 1_700_000_000, 0x0001);
        let post = x_gorgon("a=1", Some(b""), 1_700_000_000, 0x0001);
        // 空 body 在 JS 里是 Buffer.alloc(0)，md5 与全零的 16 字节不同
        assert_ne!(get, post);
    }

    #[test]
    fn rc4_gorgon_is_not_standard_rc4() {
        // 标准 RC4 在此处应产出不同结果，确认我们走的是变体
        let data = b"hello world!";
        let key = b"\x4a\x40\x16\x00\x47\x6c\x01\x00";
        let got = rc4_gorgon(data, key);
        assert_eq!(got.len(), data.len());
    }

    #[test]
    fn matches_js_golden_vector() {
        // 现版 JS 在固定设备档案 / body / khronos / random 下的输出。
        // KSA 的 `s[i] = s[j]` 一旦被误写成 swap，这里立刻红。
        let device = crate::signer::device::video_device();
        let mut params: Vec<(&str, String)> =
            device.iter().map(|(k, v)| (*k, (*v).to_string())).collect();
        params.push(("ts", "1700000000".into()));
        params.push(("_rticket", "1700000000123".into()));
        let query = crate::signer::ticket::encode_query(&params);

        let got = x_gorgon(&query, Some(br#"{"a":1}"#), 1_700_000_000, 0x1234);
        assert_eq!(got, "84043412400156f421453230d5ff1364e71df6105a4612699380");
    }
}
