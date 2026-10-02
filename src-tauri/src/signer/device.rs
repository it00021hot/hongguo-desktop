//! 设备指纹。
//!
//! 签名头与 query 参数必须来自同一份设备档案：Medusa 会把 device_id 和
//! version_name 打进密文，服务端两边比对，不一致直接判为伪造。
//!
//! ⚠️ 这是一组固定的实测设备参数。改这里等于换设备，换了之后签名仍然自洽，
//!    但服务端可能对该档案做更严格的风控。建议整组一起替换，不要只改一两个字段。

use crate::signer::protobuf::{proto, FieldType, FieldValue};

/// 应用 id 与渠道号，Medusa 密文里硬编码。
pub const APP_ID: &str = "8662";
pub const CHANNEL_ID: &str = "1588093228";

/// 与设备档案配套的 UA，二者的系统版本号必须一致。
pub const VIDEO_UA: &str = concat!(
    "com.phoenix.read/71332 (Linux; U; Android 16; zh_CN; 25053RT47C; ",
    "Build/BP2A.250605.031.A3; Cronet/TTNetVersion:04657795 2026-01-23 ",
    "QuicVersion:c67e9834 2025-09-08)"
);

/// 短剧播放接口（video_model / video_detail）使用的设备档案。
///
/// 用 `BTreeMap` 而非 JS 的对象字面量，**是为了保证 query 参数顺序稳定**——
/// 签名是「URL 查询串 + body 字节 + 时间戳」的联合函数，参数顺序变化会导致失配。
/// JS 对象对整数键会重排为升序，这里用字符串键 + 显式顺序表避免该问题。
pub fn video_device() -> Vec<(&'static str, &'static str)> {
    vec![
        ("iid", "1905892595382586"),
        ("device_id", "1905892595378490"),
        ("ac", "wifi"),
        ("channel", "update_64"),
        ("aid", "8662"),
        ("app_name", "novelread"),
        ("version_code", "71332"),
        ("version_name", "7.1.3.32"),
        ("device_platform", "android"),
        ("os", "android"),
        ("ssmix", "a"),
        ("device_type", "25053RT47C"),
        ("device_brand", "Redmi"),
        ("language", "zh"),
        ("os_api", "36"),
        ("os_version", "16"),
        ("manifest_version_code", "71332"),
        ("resolution", "1280*2772"),
        ("dpi", "520"),
        ("update_version_code", "71332"),
        ("host_abi", "arm64-v8a"),
        ("dragon_device_type", "phone"),
        ("pv_player", "71332"),
        ("compliance_status", "0"),
        ("need_personal_recommend", "1"),
        ("player_so_load", "1"),
        ("is_android_pad_screen", "0"),
    ]
}

/// 构造设备信息 protobuf（Medusa message 的 field 12）。
///
/// 除了 device_id 之外全部为固定值——这些是采集自真机的传感器读数、
/// 区域设置与机型标识，签名侧只做搬运，不做计算。
///
/// `version_name` 保留但不使用：现版 JS 的 `deviceProto(deviceId, versionName)`
/// 同样收了它却没写进 body。去掉参数会让两边的调用形状对不上，
/// 以后有人照 JS 补字段时容易漏掉这一步，所以留着。
pub fn device_proto(device_id: &str, _version_name: &str) -> Vec<u8> {
    proto(&[
        (1, FieldValue::SInt(1), FieldType::SInt),
        (2, FieldValue::SInt(2), FieldType::SInt),
        (3, FieldValue::Str(APP_ID.to_string()), FieldType::Str),
        (4, FieldValue::Str(device_id.to_string()), FieldType::Str),
        (
            5,
            FieldValue::Str("Ai6svO3PyrwDOUSmO6ZcResxu".to_string()),
            FieldType::Str,
        ),
        (6, FieldValue::Str("!noperm!".to_string()), FieldType::Str),
        (7, FieldValue::SInt(-888_888), FieldType::SInt),
        (8, FieldValue::SInt(-888_888), FieldType::SInt),
        (9, FieldValue::SInt(3), FieldType::SInt),
        (10, FieldValue::SInt(-888_888), FieldType::SInt),
        (11, FieldValue::Str("!notset!".to_string()), FieldType::Str),
        (
            12,
            FieldValue::Str("Asia/Shanghai,8".to_string()),
            FieldType::Str,
        ),
        (13, FieldValue::Str("zh_CN".to_string()), FieldType::Str),
        (14, FieldValue::SInt(4), FieldType::SInt),
        (
            16,
            FieldValue::Float(255.249_938_964_843_75),
            FieldType::Float,
        ),
        (
            17,
            FieldValue::Float(35.585_990_905_761_72),
            FieldType::Float,
        ),
        (
            18,
            FieldValue::Float(3.467_449_188_232_422),
            FieldType::Float,
        ),
        (
            19,
            FieldValue::Float(3.467_449_188_232_422),
            FieldType::Float,
        ),
        (
            20,
            FieldValue::Float(255.175_491_333_007_8),
            FieldType::Float,
        ),
        (
            21,
            FieldValue::Float(42.175_441_741_943_36),
            FieldType::Float,
        ),
        (22, FieldValue::Str("16".to_string()), FieldType::Str),
        (23, FieldValue::SInt(41), FieldType::SInt),
        (24, FieldValue::SInt(36), FieldType::SInt),
        (25, FieldValue::SInt(1_728_388_016_635), FieldType::SInt),
        (26, FieldValue::SInt(1_728_388_016_635), FieldType::SInt),
        (27, FieldValue::SInt(1_728_388_016_635), FieldType::SInt),
        (28, FieldValue::SInt(1_728_388_016_637), FieldType::SInt),
        (29, FieldValue::SInt(-1), FieldType::SInt),
        (
            30,
            FieldValue::Str("25053RT47C".to_string()),
            FieldType::Str,
        ),
        (31, FieldValue::Str("Redmi".to_string()), FieldType::Str),
        (
            32,
            FieldValue::Str("25053RT47C".to_string()),
            FieldType::Str,
        ),
        (
            33,
            FieldValue::Str("25053RT47C".to_string()),
            FieldType::Str,
        ),
        (34, FieldValue::Str("Xiaomi".to_string()), FieldType::Str),
        (35, FieldValue::Str("Redmi".to_string()), FieldType::Str),
        (36, FieldValue::Str("Redmi".to_string()), FieldType::Str),
        (38, FieldValue::SInt(31), FieldType::SInt),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_device_profile_is_stable() {
        let device = video_device();
        assert_eq!(device.len(), 27);
        // 切片顺序就是 query 参数顺序，签名对它敏感，不能重排
        assert_eq!(device[0], ("iid", "1905892595382586"));
        assert_eq!(device[1], ("device_id", "1905892595378490"));

        let value_of = |key: &str| {
            device
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, value)| *value)
        };
        assert_eq!(value_of("version_name"), Some("7.1.3.32"));
        // UA 里的系统版本必须与设备档案的 os_version 一致
        assert_eq!(value_of("os_version"), Some("16"));
        assert!(VIDEO_UA.contains("Android 16"));

        let mut keys: Vec<&str> = device.iter().map(|(key, _)| *key).collect();
        let total = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), total, "设备档案不应出现重复 key");
    }

    #[test]
    fn device_proto_is_non_empty_and_stable() {
        let a = device_proto("1905892595378490", "7.1.3.32");
        let b = device_proto("1905892595378490", "7.1.3.32");
        assert_eq!(a, b);
        assert!(!a.is_empty());
    }
}
