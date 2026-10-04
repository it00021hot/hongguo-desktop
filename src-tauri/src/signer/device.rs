//! 设备指纹。
//!
//! 签名头与 query 参数必须来自同一份设备档案：Medusa 会把 device_id 和
//! version_name 打进密文，服务端两边比对，不一致直接判为伪造。
//!
//! 档案有 owned 形态 [`DeviceProfile`]（可从数据库装载、可被设备注册流程
//! 改写字段）；`video_device()` 返回的是**实测可用的静态兜底档案**——
//! 注册失败或未注册时用它，保证签名链路永远有自洽的设备可用。

use serde::{Deserialize, Serialize};

use crate::signer::protobuf::{proto, FieldType, FieldValue};

/// 应用 id 与渠道号，Medusa 密文里硬编码。
pub const APP_ID: &str = "8662";
pub const CHANNEL_ID: &str = "1588093228";

/// 与设备档案配套的 UA，二者的系统版本号必须一致。
pub const VIDEO_UA: &str = concat!(
    "com.phoenix.read/73932 (Linux; U; Android 14; zh_CN; Xiaomi 14; ",
    "Build/UKQ1.230804.001; Cronet/TTNetVersion:8d40f833 ",
    "QuicVersion:462f352c 2026-08-31)"
);

/// 视频 CDN 403 时补上的官网 Referer（直链被拒的最后手段）。
///
/// CDN 的防盗链是两类规则并存：主流边缘对**任何**带 Referer 的请求直接 403
/// （所以首选裸 UA），个别节点却反过来要求 Referer。遇到 403 补上它再试一次，
/// 两类都能过。
pub const VIDEO_REFERER: &str = "https://novelquickapp.com/";

/// 一份设备档案。
///
/// 字段是**保序**键值对：顺序就是签名 query 的参数顺序，签名对它敏感，
/// 所以绝不排序、不去重，只按构造时的顺序搬运。
/// `user_agent` 与字段必须成套（系统版本号两边一致），改机型要整组换。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceProfile {
    #[serde(default)]
    fields: Vec<(String, String)>,
    #[serde(default = "default_user_agent")]
    user_agent: String,
}

fn default_user_agent() -> String {
    VIDEO_UA.to_string()
}

impl DeviceProfile {
    /// 从静态键值对构造（测试与静态兜底档案用）。
    pub fn from_pairs(pairs: &[(&'static str, &'static str)], user_agent: &str) -> Self {
        Self {
            fields: pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            user_agent: user_agent.to_string(),
        }
    }

    /// 按 key 取字段值，缺 key 给空串（与旧 `device_field` 行为一致）。
    pub fn get(&self, key: &str) -> &str {
        self.fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .unwrap_or_default()
    }

    /// 覆盖已有 key 的值。key 不存在时**追加在尾部**（注册流程补
    /// cdid / openudid 等新字段用）；不排序。
    // M2b 设备注册落位前的脚手架（当前仅测试引用）。
    #[allow(dead_code)]
    pub fn set(&mut self, key: &str, value: &str) {
        if let Some(slot) = self.fields.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value.to_string();
        } else {
            self.fields.push((key.to_string(), value.to_string()));
        }
    }

    /// 设备注册用的全新档案：73932 / Xiaomi 14 指纹，**不带** device_id/iid
    ///（首次注册 query 里不能有空值形态的字段），cdid/openudid 由调用方
    /// 生成后填入，与注册 body 的 JSON 指纹保持同一套。
    #[allow(dead_code)]
    pub fn fresh_register_profile(cdid: &str, openudid: &str) -> Self {
        let ua = "com.phoenix.read/73932 (Linux; U; Android 14; zh_CN; Xiaomi 14; Build/UKQ1.230804.001; Cronet/TTNetVersion:8d40f833 QuicVersion:462f352c 2026-08-31)";
        let mut dev = Self::from_pairs(
            &[
                ("ac", "wifi"),
                ("aid", "8662"),
                ("app_name", "novelread"),
                ("channel", "xiaomi_8662_64"),
                ("compliance_status", "0"),
                ("device_brand", "xiaomi"),
                ("device_platform", "android"),
                ("device_type", "23127PN0CC"),
                ("dpi", "460"),
                ("dragon_device_type", "phone"),
                ("host_abi", "arm64-v8a"),
                ("is_android_pad_screen", "0"),
                ("language", "zh"),
                ("manifest_version_code", "73932"),
                ("os", "android"),
                ("os_api", "34"),
                ("os_version", "14"),
                ("player_so_load", "1"),
                ("pv_player", "73932"),
                ("resolution", "1200*2670"),
                ("ssmix", "a"),
                ("update_version_code", "73932"),
                ("version_code", "73932"),
                ("version_name", "7.3.9.32"),
            ],
            ua,
        );
        dev.set("cdid", cdid);
        dev.set("openudid", openudid);
        dev
    }

    /// 移除字段（设备注册用：首次注册的 query 里不能带 `device_id=`/
    /// `iid=` 空值形态，字段必须**不存在**而不是空值）。
    #[allow(dead_code)]
    pub fn remove(&mut self, key: &str) {
        self.fields.retain(|(k, _)| k != key);
    }

    /// 字段迭代（保序）。签名 query 与 Medusa 都从这里取值。
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.fields.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// 与档案配套的 UA。
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }
}

/// 短剧播放接口（video_model / video_detail）使用的静态兜底设备档案。
///
/// 这是一组固定的实测设备参数：换掉之后签名仍然自洽，但服务端可能对
/// 新档案做更严格的风控。字段顺序就是 query 顺序，**不要重排**——
/// 签名是「URL 查询串 + body 字节 + 时间戳」的联合函数。
pub fn video_device() -> DeviceProfile {
    // 2026-10-04 抓包刷新（captures/flows-20261004.jsonl）：71332 旧档案的
    // install_id 已被服务端风控清理——reading 系（搜索等）整段 0 字节拒。
    // 与下方 ANON_TTREQ 同源，失效时重抓 hgplayer 并整组同换。
    DeviceProfile::from_pairs(
        &[
            ("iid", "2715158266282762"),
            ("device_id", "2715158266032906"),
            ("ac", "wifi"),
            ("channel", "xiaomi_8662_64"),
            ("aid", "8662"),
            ("app_name", "novelread"),
            ("cdid", "babf8a0e-8586-40ca-bb75-20a1a698b43f"),
            ("version_code", "73932"),
            ("version_name", "7.3.9.32"),
            ("device_platform", "android"),
            ("os", "android"),
            ("ssmix", "a"),
            ("device_type", "23127PN0CC"),
            ("device_brand", "xiaomi"),
            ("language", "zh"),
            ("os_api", "34"),
            ("os_version", "14"),
            ("manifest_version_code", "73932"),
            ("resolution", "1200*2670"),
            ("dpi", "460"),
            ("update_version_code", "73932"),
            ("host_abi", "arm64-v8a"),
            ("dragon_device_type", "phone"),
            ("pv_player", "73932"),
            ("compliance_status", "0"),
            ("need_personal_recommend", "1"),
            ("player_so_load", "1"),
            ("is_android_pad_screen", "0"),
        ],
        VIDEO_UA,
    )
}

/// 兜底档案配套的 `ttreq`（服务端按 install_id 发放的票）。
///
/// 与 [`video_device`] 同一次抓包而来：Cookie 里的 `install_id` 必须与档案
/// 的 `iid` 一致，`ttreq` 也是设备维度的，所以两者要整组同换。设备注册
/// 链路打通后，应改从注册响应的 Set-Cookie 里取自己的票。
pub const ANON_TTREQ: &str = "1$5c6c7c7cd605c0a9f176a0533d9c64755b5df874";

/// 未登录时的匿名 Cookie：reading 系接口按 `install_id` 风控，缺它会被
/// 静默拒（HTTP 200 + 0 字节）。`install_id` 从档案的 `iid` 派生，保证
/// 换档案时始终一致；`store-region` 是实测的属地标记。
pub fn anonymous_cookie(device: &DeviceProfile) -> String {
    format!(
        "store-region=cn-gd; store-region-src=did; install_id={}; ttreq={}",
        device.get("iid"),
        ANON_TTREQ
    )
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
        assert_eq!(device.iter().count(), 28);
        // 迭代顺序就是 query 参数顺序，签名对它敏感，不能重排
        let first: Vec<(&str, &str)> = device.iter().take(2).collect();
        assert_eq!(
            first,
            vec![("iid", "2715158266282762"), ("device_id", "2715158266032906")]
        );

        assert_eq!(device.get("version_name"), "7.3.9.32");
        // UA 里的系统版本必须与设备档案的 os_version 一致
        assert_eq!(device.get("os_version"), "14");
        assert!(device.user_agent().contains("Android 14"));

        let mut keys: Vec<&str> = device.iter().map(|(key, _)| key).collect();
        let total = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), total, "设备档案不应出现重复 key");
    }

    #[test]
    fn anonymous_cookie_matches_profile_iid() {
        let device = video_device();
        let cookie = anonymous_cookie(&device);
        // install_id 必须与档案 iid 一致，否则服务端判为两个设备
        assert!(cookie.contains(&format!("install_id={}", device.get("iid"))));
        assert!(cookie.starts_with("store-region=cn-gd"));
        assert!(cookie.contains(&format!("ttreq={ANON_TTREQ}")));
    }

    #[test]
    fn set_overrides_in_place_and_appends_new_keys() {
        let mut device = video_device();
        let idx = device.iter().position(|(k, _)| k == "device_id").unwrap();
        device.set("device_id", "42");
        // 覆盖发生在原位置，不改变字段顺序
        assert_eq!(device.iter().nth(idx), Some(("device_id", "42")));
        // 已存在的 key（cdid 已进兜底档案）走覆盖，不动顺序
        let cdid_pos = device.iter().position(|(k, _)| k == "cdid").unwrap();
        device.set("cdid", "abc");
        assert_eq!(device.iter().nth(cdid_pos), Some(("cdid", "abc")));
        // 档案外的新 key 追加在尾部（注册流程补 openudid 等字段）
        device.set("openudid", "xyz");
        assert_eq!(device.iter().last(), Some(("openudid", "xyz")));
        assert_eq!(device.iter().count(), 29);
    }

    #[test]
    fn profile_roundtrips_through_serde() {
        // 设备注册后档案要落库，serde 往返必须无损（含字段顺序）
        let device = video_device();
        let json = serde_json::to_string(&device).unwrap();
        let back: DeviceProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(device, back);
        let order: Vec<String> = back.iter().map(|(k, _)| k.to_string()).collect();
        let order_before: Vec<String> = device.iter().map(|(k, _)| k.to_string()).collect();
        assert_eq!(order, order_before);
    }

    #[test]
    fn legacy_json_without_user_agent_falls_back() {
        // schema 演进容错：旧档没有 user_agent 字段时回落到 VIDEO_UA
        let json = r#"{"fields":[["aid","8662"]]}"#;
        let back: DeviceProfile = serde_json::from_str(json).unwrap();
        assert_eq!(back.user_agent(), VIDEO_UA);
        assert_eq!(back.get("aid"), "8662");
    }

    #[test]
    fn device_proto_is_non_empty_and_stable() {
        let a = device_proto("1905892595378490", "7.1.3.32");
        let b = device_proto("1905892595378490", "7.1.3.32");
        assert_eq!(a, b);
        assert!(!a.is_empty());
    }
}
