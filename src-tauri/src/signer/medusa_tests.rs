//! Medusa 签名的单元测试。
//!
//! 以 `#[path]` 挂到 `medusa.rs` 下。

use super::*;

#[test]
fn medusa_output_is_base64_and_non_trivial() {
    let out = build_medusa(
        "https://api5-normal-sinfonlineb.fqnovel.com/novel/player/multi_video_detail/v1/?aid=8662",
        Some(b"{\"series_id\":123}"),
        1_700_000_000,
        "1905892595378490",
        "7.1.3.32",
    );
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&out)
        .expect("必须是合法 base64");
    // 20 字节版本前缀 + 4 字节随机种子 + AES 变体输出。
    // 密文长度不固定：AES 变体的位打包固定读源数据前 248 字节，
    // 但源数据（protobuf message）里含随机数，zigzag 编码长度随数值变化，
    // 因此 message 长度会在一定范围内浮动。
    assert!(
        decoded.len() > 20 + 4,
        "至少应包含前缀与种子，实际 {} 字节",
        decoded.len()
    );
    // 前缀首字节固定为 0x03
    assert_eq!(decoded[0], 0x03);
    // 随机种子的低两字节在 payload 里（第 20..24 位）
    assert_eq!(decoded[23], 1, "种子末字节固定标记为 1");
}

#[test]
fn xmxor_preserves_length() {
    let data = vec![1u8; 248];
    let key = vec![2u8; 32];
    assert_eq!(xmxor(&data, &key).len(), 248);
}

// ============================================================================
// 黄金向量
//
// 下面几组期望值来自现版 JS（`src/native/signer`），在**固定随机源**下导出。
// 之所以要固定随机量：medusa 内嵌 5 个随机数，直接比对每次都不一样，
// 只能靠「喂固定熵 → 比对固定输出」把整条流水线钉死。
// x-helios 的黄金向量在 `helios.rs` 里（它已拆成独立文件）。
//
// ⚠️ 服务端签名失效时只返回 HTTP 200 + 0 字节，不给任何错误码。
//    这类失配无法靠运行时错误发现，只能靠这里的向量在 CI 里拦住。
// ============================================================================

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
    let got = assemble_medusa(
        GOLDEN_URL,
        Some(GOLDEN_BODY),
        GOLDEN_KHRONOS,
        "1905892595378490",
        "7.1.3.32",
        golden_entropy(),
    );
    assert!(
        got.starts_with("A/FTZfcZDJ/XJo9e1tubFVc+Mn3NSAABdRXCpV+OAcMGGGZAjidbZnk"),
        "x-medusa 与现版不一致，实际：{got}"
    );
}
