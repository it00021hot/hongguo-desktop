//! Medusa 签名的单元测试（以 `#[path]` 挂到 `medusa.rs` 下）。

use super::*;
use crate::fingerprint_for_cdid;

#[test]
fn medusa_output_is_base64_and_non_trivial() {
    let out = build_medusa(
        "https://api5-normal-sinfonlineb.fqnovel.com/novel/player/multi_video_detail/v1/?aid=8662",
        Some(b"{\"series_id\":123}"),
        1_700_000_000,
        "cdid-uuid",
        "1905892595378490",
        "7.1.3.32",
    );
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&out)
        .expect("必须是合法 base64");
    // 密文长度不固定：message 含随机数，zigzag 编码长度随数值浮动
    assert!(
        decoded.len() > 20 + 4,
        "至少应包含前缀与种子，实际 {} 字节",
        decoded.len()
    );
    assert_eq!(decoded[0], 0x03);
    assert_eq!(decoded[23], 1, "种子末字节固定标记为 1");
}

#[test]
fn xmxor_preserves_length() {
    let data = vec![1u8; 248];
    let key = vec![2u8; 32];
    assert_eq!(xmxor(&data, &key).len(), 248);
}

// 黄金向量：期望值来自现版 JS（`src/native/signer`）固定随机源导出——medusa 内嵌 5 个随机数，
// 只能靠「喂固定熵 → 比对固定输出」钉死整条流水线；服务端失配只回 HTTP 200 + 0 字节，只能靠这些向量在 CI 拦住。

const GOLDEN_URL: &str = "https://api5-normal-sinfonlineb.fqnovel.com/x/v1?aid=8662&device_id=1905892595378490&ts=1700000000";
const GOLDEN_BODY: &[u8] = br#"{"a":1}"#;
const GOLDEN_KHRONOS: u32 = 1_700_000_000;

fn golden_entropy() -> Entropy {
    Entropy {
        env_launch: 103,
        env_pid: 11_691,
        now_secs: 1_700_000_000,
        message_random: 1_273_073_252,
        random: 2_378_451_149,
        packed_random: 2_253_366_658,
    }
}

#[test]
fn key_hash_matches_js_golden() {
    let (hash, seed) = key_hash(&medusa_sign_key(), 2_378_451_149);
    assert_eq!(
        hex::encode(hash),
        "1b35023431b99a949ab7b70fc50a30216076492a6229d102a7a33fa3a1104170"
    );
    assert_eq!(hex::encode(seed), "b0dff9ff");
}

#[test]
fn xmxor_matches_js_golden() {
    let (hash, _) = key_hash(&medusa_sign_key(), 2_378_451_149);
    assert_eq!(
        hex::encode(xmxor(b"0123456789abcdefghij", &hash)),
        "5c69c37694106e8cbdb2e074619a1dda6c5a5a65"
    );
}

#[test]
fn assemble_medusa_matches_js_golden() {
    // 黄金向量走 assemble_with_proto + JS 移植原始采样 proto，锚住「管线算法未漂移」；生产 device_proto 输入已变，不能再直接对 assemble_medusa
    let got = assemble_with_proto(
        GOLDEN_URL,
        Some(GOLDEN_BODY),
        GOLDEN_KHRONOS,
        "1905892595378490",
        "7.1.3.32",
        extracted_proto_fixed("1905892595378490"),
        golden_entropy(),
    );
    assert!(
        got.starts_with("A/FTZfcZDJ/XJo9e1tubFVc+Mn3NSAABdRXCpV+OAcMGGGZAjidbZnk"),
        "x-medusa 与现版不一致，实际：{got}"
    );
}

#[test]
fn device_proto_matches_fingerprint_and_varies_per_device() {
    let fp = fingerprint_for_cdid("cdid-uuid");
    let (a, b) = (
        device_proto("cdid-uuid", "111"),
        device_proto("cdid-uuid", "222"),
    );
    let fa = fields_debug(&a);
    let fb = fields_debug(&b);
    assert!(
        fa.contains(fp.brand) && fa.contains(fp.model),
        "机型组应取池指纹（brand={} model={}）: {fa}",
        fp.brand,
        fp.model
    );
    assert!(fa.contains(fp.manufacturer));
    assert_ne!(
        fa, fb,
        "不同 device_id 的 proto 应不同（传感器抖动/字段 5）"
    );
    assert_ne!(a, b);
}

/// 把 proto 字段拉成可读串（测试辅助）。
fn fields_debug(b: &[u8]) -> String {
    fn varint(b: &[u8], i: &mut usize) -> u64 {
        let mut v = 0u64;
        let mut s = 0u32;
        loop {
            let x = *b.get(*i).unwrap_or(&0);
            *i += 1;
            v |= u64::from(x & 0x7f) << s;
            if x & 0x80 == 0 {
                return v;
            }
            s += 7;
        }
    }
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        let tag = varint(b, &mut i);
        let (f, wt) = (tag >> 3, tag & 7);
        match wt {
            0 => {
                let v = varint(b, &mut i);
                out.push_str(&format!("{f}:{v} "));
            }
            2 => {
                let len = varint(b, &mut i) as usize;
                let end = (i + len).min(b.len());
                out.push_str(&format!(
                    "{f}:bytes[{}]={} ",
                    len,
                    String::from_utf8_lossy(&b[i..end])
                ));
                i = end;
            }
            5 => {
                out.push_str(&format!("{f}:f32 "));
                i += 4;
            }
            _ => break,
        }
    }
    out
}
