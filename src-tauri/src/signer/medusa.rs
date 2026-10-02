//! Medusa：当代主力签名。
//!
//! 流程：
//! 1. 把「设备指纹 + query/body 摘要 + 时间戳」编成 protobuf message
//! 2. 用 SM3 派生的 key 逐字节变换
//! 3. 整体反序 + 掩码异或
//! 4. 自研 AES 变体加密（见 `aes_v3.rs`）
//! 5. 拼 20 字节版本前缀，base64 输出

use base64::Engine;

use crate::signer::aes_v3::AesV3;
use crate::signer::constants::{medusa_aes_iv, medusa_aes_key, medusa_sign_key};
use crate::signer::device::{device_proto, APP_ID, CHANNEL_ID};
use crate::signer::primitives::{le32, md5_raw, sm3, u32};
use crate::signer::protobuf::{proto, FieldType, FieldValue};
use crate::signer::xargus::hash_f13;

use rand::Rng;

/// 用 SM3 派生的 key 对 message 做逐字节置换。
pub fn xmxor(data: &[u8], key: &[u8]) -> Vec<u8> {
    let n = data.len();
    let mut encoded = vec![0u8; n];

    for i in 0..n {
        let at = (i * 4) & 28;
        let d0 = key[at];
        let d1 = key[at + 1];
        // JS 的 d2 是被截断到 8 位的中间值，注意 & 255 的位置
        let mut d2 = ((((data[i] << 4) | (data[i] >> 4)) & 255) as u32 + d0 as u32) as u8;
        d2 = !d2 ^ d1;
        d2 = ((d2 << 3) | (d2 >> 5)) as u8;
        d2 = d2.wrapping_add(d1);
        d2 ^= d0;
        encoded[n - i - 1] = !d2;
    }

    let last = encoded[n - 1] ^ encoded[n - 2];
    let first = encoded[0];
    encoded[0] = (!last).wrapping_add(first);
    encoded[1] = ((encoded[0] ^ encoded[n - 1] ^ 254).wrapping_add(encoded[1])) as u8;
    // 注意运算顺序：异或先算，再与 encoded[2] 相加
    let shifted = ((encoded[1] << 3) | (encoded[1] >> 5)) as u8;
    let fix2 = (last.wrapping_sub(first)) ^ (shifted ^ 2);
    encoded[2] = encoded[2].wrapping_add(fix2);

    for i in 0..n.saturating_sub(4) {
        let shifted = ((encoded[i + 2] << 3) | (encoded[i + 2] >> 5)) as u8;
        let temp = shifted ^ encoded[i + 1] ^ ((i + 3) as u8);
        // ⚠️ `!` 必须加括号：写成 `!temp.wrapping_add(..)` 会被解析成 `!(temp + ..)`，
        //    与 JS 的 `~temp + ..` 差一个取反位置，结果完全不同。
        encoded[i + 3] = (!temp).wrapping_add(encoded[i + 3]);
    }

    encoded[n - 1] ^= encoded[n - 2];
    // JS 累加的是 encoded[1..n]，即**跳过第 0 个**、取到最后一个。
    // 写成 take(n - 1) 会从 encoded[0] 开始，差一位，x-medusa 整体失配。
    let mut sum: u32 = 0;
    for item in encoded.iter().skip(1) {
        sum = sum.wrapping_add(*item as u32);
    }
    encoded[0] = (encoded[0] ^ encoded[1]).wrapping_add(sum as u8);

    encoded
}

/// 由签名主密钥 + 随机数派生 (hash, seed)。
pub fn key_hash(sign_key: &[u8], random: u32) -> ([u8; 32], [u8; 4]) {
    let mut buf = Vec::with_capacity(sign_key.len() * 2 + 4);
    buf.extend_from_slice(sign_key);
    buf.extend_from_slice(&le32(random));
    buf.extend_from_slice(sign_key);
    let hash = sm3(&buf);

    let d1 = ((random >> 16) & 255) as u8;
    let mut d2 = ((d1 as u32) << 11 | (random >> 24)) ^ ((d1 as u32) >> 5) ^ (d1 as u32);
    d2 = u32(!d2 as u64);

    (hash, le32(d2))
}

pub(crate) fn rand32() -> u32 {
    rand::rng().random()
}

/// Medusa 需要的一次性随机量。
///
/// 全部集中在这里而不是散在 `build_medusa` 里，是为了让整条流水线可以在
/// 测试里喂固定值、与现版 JS 的输出逐字节对齐——这类签名一旦被改错，
/// 服务端只会静默返回空响应，没有报错可查，只能靠黄金向量兜住。
#[derive(Debug, Clone, Copy)]
pub struct Entropy {
    /// 进程启动次数
    pub env_launch: i64,
    /// 进程 pid
    pub env_pid: i64,
    /// 当前秒级时间戳
    pub now_secs: i64,
    /// message 内的随机字段
    pub message_random: u32,
    /// 主随机数：派生 keyHash 密钥
    pub random: u32,
    /// packed 内的填充随机数
    pub packed_random: u32,
}

impl Entropy {
    /// 按调用顺序抽取一次完整随机量。
    ///
    /// 顺序与 JS 版一致：envLaunch → envPid → message[3] → random → packed[1..4]
    fn draw() -> Self {
        let mut rng = rand::rng();
        Self {
            env_launch: rng.random_range(100..=120),
            env_pid: rng.random_range(10_001..=12_000),
            now_secs: chrono::Utc::now().timestamp(),
            message_random: rng.random(),
            random: rng.random(),
            packed_random: rng.random(),
        }
    }
}

/// 构造 x-medusa 头。
///
/// # 参数
/// - `url`：完整 URL（含 query），只取 `'?'` 之后的部分参与签名
/// - `body`：请求体；GET 传 `None`
/// - `khronos`：秒级时间戳
/// - `device_id`：设备档案的 device_id
/// - `version_name`：设备档案的 version_name
pub fn build_medusa(
    url: &str,
    body: Option<&[u8]>,
    khronos: u32,
    device_id: &str,
    version_name: &str,
) -> String {
    assemble_medusa(url, body, khronos, device_id, version_name, Entropy::draw())
}

/// 用给定随机量装配 x-medusa。测试用固定 `entropy` 对齐 JS 黄金向量。
pub fn assemble_medusa(
    url: &str,
    body: Option<&[u8]>,
    khronos: u32,
    device_id: &str,
    version_name: &str,
    entropy: Entropy,
) -> String {
    let body_md5 = body.map(md5_raw).unwrap_or([0u8; 16]);
    let query_raw = url.split('?').nth(1).unwrap_or("");
    let query_sm3 = sm3(query_raw.as_bytes());
    let ts = le32(khronos);

    let query_body_ts = hash_f13(&query_sm3, &body_md5, &ts, khronos);
    // check 依赖 query_body_ts[0]，必须在 query_body_ts 被移入 message 之前算好
    let check = (((query_sm3[0] & 63) as u32) << 14)
        | 0x1800_0001
        | (((query_body_ts[0] & 63) as u32) << 8);

    let nested = proto(&[
        (1, FieldValue::SInt(111), FieldType::SInt),
        (2, FieldValue::SInt(10), FieldType::SInt),
        (3, FieldValue::SInt(694_367), FieldType::SInt),
        (5, FieldValue::SInt(586_952_199), FieldType::SInt),
    ]);

    // 运行环境描述：进程启动次数 / 运行时长 / 设备信息
    let nested13 = proto(&[
        (1, FieldValue::SInt(entropy.now_secs), FieldType::SInt),
        (2, FieldValue::SInt(-2), FieldType::SInt),
        (4, FieldValue::SInt(200), FieldType::SInt),
    ]);
    let env = proto(&[
        (1, FieldValue::SInt(entropy.env_launch), FieldType::SInt),
        (2, FieldValue::SInt(146_331_399), FieldType::SInt),
        (3, FieldValue::SInt(146_331_396), FieldType::SInt),
        (5, FieldValue::SInt(7), FieldType::SInt),
        (
            6,
            FieldValue::Str("v04.06.04.03-bugfix".into()),
            FieldType::Str,
        ),
        (7, FieldValue::SInt(entropy.env_pid), FieldType::SInt),
        (
            12,
            FieldValue::Bytes(device_proto(device_id, version_name)),
            FieldType::Message,
        ),
        (13, FieldValue::Bytes(nested13), FieldType::Message),
        (
            14,
            FieldValue::Str(version_name.to_string()),
            FieldType::Str,
        ),
    ]);

    let mut qh = Vec::new();
    qh.extend_from_slice(query_raw.as_bytes());
    qh.extend_from_slice(&body_md5);
    qh.extend_from_slice(b"none");
    let query_hash = sm3(&qh);

    let message = proto(&[
        (
            1,
            FieldValue::Bytes(hex::decode("f7e85ffad7d7dc3bd62ac87057cf6118").expect("固定常量")),
            FieldType::Bytes,
        ),
        (2, FieldValue::SInt(3), FieldType::SInt),
        (
            3,
            FieldValue::SInt(i64::from(entropy.message_random)),
            FieldType::SInt,
        ),
        (4, FieldValue::Str(APP_ID.to_string()), FieldType::Str),
        (5, FieldValue::Str(device_id.to_string()), FieldType::Str),
        (6, FieldValue::Str(CHANNEL_ID.to_string()), FieldType::Str),
        (7, FieldValue::Str(version_name.to_string()), FieldType::Str),
        (8, FieldValue::Str("v04.06.04-ml-android".into()), FieldType::Str),
        (9, FieldValue::SInt(67_503_104), FieldType::SInt),
        (
            10,
            FieldValue::Bytes(hex::decode("4001000000000000").expect("固定常量")),
            FieldType::Bytes,
        ),
        (12, FieldValue::SInt(khronos as i64), FieldType::SInt),
        (13, FieldValue::Bytes(query_body_ts), FieldType::Bytes),
        (14, FieldValue::Bytes(query_sm3[0..6].to_vec()), FieldType::Bytes),
        (15, FieldValue::Bytes(nested), FieldType::Message),
        (16, FieldValue::Str("AXYQOS6n2m60x1fVZHIrH3iol".into()), FieldType::Str),
        (17, FieldValue::SInt(khronos as i64), FieldType::SInt),
        (19, FieldValue::Bytes(query_hash.to_vec()), FieldType::Bytes),
        (20, FieldValue::Str("none".into()), FieldType::Str),
        (21, FieldValue::SInt(312), FieldType::SInt),
        (23, FieldValue::Bytes(env), FieldType::Message),
        (
            24,
            FieldValue::Str(
                "{\"cmr\":16777216,\"cmr2\":16777216,\"un_h\":1879194040,\"vpn\":0,\"kd\":0,\"fkd\":3672518972,\"pd\":-1872573247,\"dyn\":\"\",\"do\":0,\"tk\":true}"
                    .into(),
            ),
            FieldType::Str,
        ),
    ]);

    let random = entropy.random;
    let (hash, seed) = key_hash(&medusa_sign_key(), random);

    let mut transformed = xmxor(&message, &hash);
    let mut prefix_bytes = hex::decode("4001000000000000").expect("固定常量");
    prefix_bytes.extend_from_slice(&transformed);
    transformed = prefix_bytes;
    transformed.reverse();
    for (i, item) in transformed.iter_mut().enumerate() {
        *item ^= seed[(!i) & 3];
    }

    let mut packed = Vec::with_capacity(13 + transformed.len());
    packed.push(0x35);
    packed.extend_from_slice(&le32(entropy.packed_random));
    packed.extend_from_slice(&le32(check));
    packed.extend_from_slice(&transformed);
    packed.push((random >> 16) as u8);
    packed.push((random >> 24) as u8);

    let encrypted = AesV3::new(&medusa_aes_key(), khronos).encrypt(&packed, &medusa_aes_iv());

    // 版本前缀：每 4 字节与 khronos 异或
    let version = hex::decode("03000000f7e85ffad7d7dc3bd62ac87057cf6118").expect("固定常量");
    let mut prefix = [0u8; 20];
    for i in 0..5usize {
        let v = u32::from_le_bytes([
            version[i * 4],
            version[i * 4 + 1],
            version[i * 4 + 2],
            version[i * 4 + 3],
        ]);
        prefix[i * 4..i * 4 + 4].copy_from_slice(&(v ^ khronos).to_le_bytes());
    }

    let mut out = Vec::with_capacity(20 + 4 + encrypted.len());
    out.extend_from_slice(&prefix);
    out.push((random & 255) as u8);
    out.push(((random >> 8) & 255) as u8);
    out.push(0);
    out.push(1);
    out.extend_from_slice(&encrypted);

    base64::engine::general_purpose::STANDARD.encode(out)
}

#[cfg(test)]
#[path = "medusa_tests.rs"]
mod tests;
