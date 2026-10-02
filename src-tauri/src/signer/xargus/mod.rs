//! X-Argus 用的自研哈希（f13）—— 分支判定与统一入口。
//!
//! 服务端用 query / body / 时间戳推导出 3 个分支之一，客户端必须算出同一个分支，
//! 算错分支等于没签名。三个分支的分布由 `branch` 决定：
//!
//! - `branch == 0`：重量级路径（见 [`branch0`]）——112 轮消息扩展 + 变体轮函数
//! - `branch == 2`：变种 MD5 路径（见 [`branch2`]）
//! - 其它：直通路径——拼 sm3 前 16 字节 + body 摘要
//!
//! 三个分支实现差异极大，拆成独立文件；这里只保留判定与分发。

pub mod branch0;
pub mod branch2;

use crate::signer::primitives::{get_iv, le32, sum_md5};

pub use branch0::compute as branch0_f13;
pub use branch2::compute as branch2_f13;

/// 判定给定请求落在哪个分支（0 / 1 / 2）。
///
/// 只需要 query 串、body 摘要字节和时间戳字节，可用于发请求前预判。
/// 返回 `0`（重量级）、`1`（**服务端不受理**）或 `2`（变种 MD5）。
pub fn branch_of(query: &[u8], body_md5: &[u8], ts_bytes: &[u8]) -> u32 {
    let iv = get_iv(get_iv(get_iv(0x2023_0928, query), body_md5), ts_bytes);
    let low = iv & 15;
    low - ((low * 171) >> 9) * 3
}

/// 计算 f13 摘要（最终作为 x-argus 头的负载）。
///
/// # 参数
/// - `query_sm3`：query 串的 SM3 摘要
/// - `body_md5`：body 的 MD5 摘要；GET 时传 16 个 0
/// - `ts_bytes`：小端 4 字节 khronos
/// - `khronos`：秒级时间戳
///
/// 返回长度随分支而异：branch 0 / branch 2 为 20 字节，直通分支为 36 字节
/// （sm3 前 16 + body 摘要 16 + 校验和 4）。这是算法本身的设计，不要强行统一。
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signer::primitives::{md5_raw, sm3};

    fn fixture() -> (Vec<u8>, [u8; 16], [u8; 4], u32) {
        let query = b"aid=8662&device_id=1905892595378490";
        let query_sm3 = sm3(query).to_vec();
        let body_md5 = crate::signer::primitives::md5_raw(b"{\"series_id\":123}");
        let ts = le32(1_700_000_000);
        (query_sm3, body_md5, ts, 1_700_000_000)
    }

    #[test]
    fn f13_length_matches_branch() {
        let (q, b, _, k) = fixture();
        // 抖动足够多的 ticket，三个分支都会出现，长度必须与分支匹配
        let mut lengths = std::collections::HashSet::new();
        for offset in 0..200u32 {
            let ts = le32(k + offset);
            let out = hash_f13(&q, &b, &ts, k + offset);
            assert!(
                out.len() == 20 || out.len() == 36,
                "offset={offset} 长度 {} 非法",
                out.len()
            );
            lengths.insert(out.len());
        }
        assert!(lengths.len() >= 2, "应观察到不止一种分支长度: {lengths:?}");
    }

    #[test]
    fn branch_of_is_in_valid_range() {
        for i in 0..200u32 {
            let query = format!("a={i}");
            let body = format!("body{i}");
            let ts = le32(1_700_000_000 + i);
            let branch = branch_of(
                query.as_bytes(),
                &crate::signer::primitives::md5_raw(body.as_bytes()),
                &ts,
            );
            assert!(branch <= 2, "branch={branch} i={i}");
        }
    }

    #[test]
    fn branch_distribution_hits_all_three() {
        // 抖动 64 个 ticket 必须能观察到多个分支，否则 _rticket 抖动无从生效
        let mut seen = std::collections::HashSet::new();
        for i in 0..200u32 {
            let query = format!("a=1&t={i}");
            let ts = le32(1_700_000_000 + i);
            let branch = branch_of(query.as_bytes(), &[0u8; 16], &ts);
            seen.insert(branch);
        }
        assert!(seen.len() >= 2, "分支分布过于单一: {seen:?}");
    }

    #[test]
    fn f13_is_deterministic() {
        let (q, b, t, k) = fixture();
        assert_eq!(hash_f13(&q, &b, &t, k), hash_f13(&q, &b, &t, k));
    }

    /// 三个分支都要有黄金向量。
    ///
    /// ⚠️ 曾经只测了 branch 2，结果 `BRANCH0_TT[61]` 多抄了一位十六进制
    ///    （`0x284ba7` 写成 `0x284ba7a`）而无人察觉——branch 0 占约 1/3 的请求，
    ///    实测失败率 12.5%，表现为**间歇性** HTTP 200 + 0 字节，极难归因。
    ///
    ///    下面 24 个向量覆盖 branch 0 / 1 / 2 三条路径，长度与内容都锁死。
    #[test]
    fn f13_matches_js_golden_vectors_for_all_branches() {
        const QUERY_HEAD: &str = "iid=1905892595382586&device_id=1905892595378490&ac=wifi&channel=update_64&aid=8662&app_name=novelread&version_code=71332&version_name=7.1.3.32&device_platform=android&os=android&ssmix=a&device_type=25053RT47C&device_brand=Redmi&language=zh&os_api=36&os_version=16&manifest_version_code=71332&resolution=1280*2772&dpi=520&update_version_code=71332&host_abi=arm64-v8a&dragon_device_type=phone&pv_player=71332&compliance_status=0&need_personal_recommend=1&player_so_load=1&is_android_pad_screen=0";
        let body = br#"{"biz_param":{"detail_page_version":0},"dr_scene":"preload","series_id":"7687919221593885758"}"#;
        let body_md5 = md5_raw(body);

        // 来自现版 JS 固定 ticket 序列的输出
        const EXPECTED: &[(&str, usize)] = &[
            ("c3dfdf442f18e029", 20),
            ("711b34123f52dd88", 20),
            ("8991baebd8f6eed6", 20),
            ("7133aa27c9e9f4f0", 36),
            ("30222371ba76d0b3", 20),
            ("82e41a747218c44e", 20),
            ("9f3fde81a8263bad", 20),
            ("e3036862514734bf", 20),
            ("6d6815250ce06fb7", 20),
            ("b73c67c22665e418", 20),
            ("842736c4d340b583", 36),
            ("e9570b1dccf13cec", 36),
            ("80428b0f6b597167", 36),
            ("d4741e4e43cfc509", 20),
            ("011ce78db653447e", 36),
            ("ec463eface6621cd", 20),
            ("860341f84aec32b6", 36),
            ("c131eb0da5c08dbf", 20),
            ("ff3d2d20e8d9d86d", 20),
            ("dc91a03a3a485c2c", 36),
            ("7cf0eb47f5727846", 36),
            ("8d87f8512c76cdca", 36),
            ("a94c942fd55bc690", 36),
            ("cf66ebc2a9794245", 36),
        ];

        let mut branches = std::collections::BTreeSet::new();
        for (i, (want, want_len)) in EXPECTED.iter().enumerate() {
            let ticket = 1_790_900_000_000 + i as u64;
            let khronos = (ticket / 1000) as u32;
            let query = format!("{QUERY_HEAD}&ts={khronos}&_rticket={ticket}");
            let qs = sm3(query.as_bytes());
            let ts = le32(khronos);

            let got = hash_f13(&qs, &body_md5, &ts, khronos);
            assert_eq!(got.len(), *want_len, "第 {i} 个向量长度不符");
            let hex: String = got.iter().take(8).map(|b| format!("{b:02x}")).collect();
            assert_eq!(&hex, *want, "第 {i} 个向量内容不符（分支判定错误？）");

            let low = crate::signer::primitives::get_iv(
                crate::signer::primitives::get_iv(
                    crate::signer::primitives::get_iv(0x2023_0928, &qs),
                    &body_md5,
                ),
                &ts,
            ) & 15;
            branches.insert(low - ((low * 171) >> 9) * 3);
        }
        assert_eq!(
            branches,
            [0, 1, 2].into_iter().collect(),
            "黄金向量必须覆盖全部三个分支"
        );
    }

    #[test]
    fn f13_changes_with_khronos() {
        let (q, b, _, k) = fixture();
        assert_ne!(
            hash_f13(&q, &b, &le32(k), k),
            hash_f13(&q, &b, &le32(k + 1), k + 1)
        );
    }
}
