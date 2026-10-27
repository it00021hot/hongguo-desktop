//! 设备指纹。
//!
//! 签名头与 query 参数必须来自同一份设备档案：Medusa 会把 device_id 和
//! version_name 打进密文，服务端两边比对，不一致直接判为伪造。
//!
//! 档案有 owned 形态 [`DeviceProfile`]（可从数据库装载、可被设备注册流程
//! 改写字段）；游客身份走 [`anonymous_profile`]——官方 7.4.2.32 起
//! 未登录读播**不再需要设备注册**，仅凭 cdid + 签名即可，
//! 共享的静态兜底档案已废除。

use serde::{Deserialize, Serialize};

use crate::protobuf::{FieldType, FieldValue, proto};

/// 应用 id 与渠道号，Medusa 密文里硬编码。
pub const APP_ID: &str = "8662";
pub const CHANNEL_ID: &str = "1588093228";

/// 官方客户端版本身份（全仓唯一出处）。
/// query / 注册模板 / UA 一律引用这三常量，禁止再散写魔法值。
pub const APP_VERSION_CODE: u64 = 74232;
pub const APP_VERSION_CODE_STR: &str = "74232";
pub const APP_VERSION_NAME: &str = "7.4.2.32";

/// 注册指纹钉定的版本身份（74232 活形态）。注册请求
/// 与注册产物档案**终生钉这套身份**，不随 [`APP_VERSION_CODE`] 走——
/// 升当前版本与注册指纹不同源，取流即挂。
pub const REGISTER_VERSION_CODE: u64 = 74232;
pub const REGISTER_VERSION_CODE_STR: &str = "74232";
pub const REGISTER_VERSION_NAME: &str = "7.4.2.32";

/// 与设备档案配套的 UA（App 版本段引用 [`APP_VERSION_CODE_STR`]，
/// 机型段与档案指纹一致，改机型要整组同换）。
///
/// UA 尾段为 7.4.2.32 起官方统一的 `tt-ok/3.12.13.20` 形态，
/// 匿名请求的机型段会在服务端过风控名单，跟着换。
pub fn video_ua() -> String {
    format!(
        "com.phoenix.read/{} (Linux; U; Android 14; zh_CN; Xiaomi 14; \
         Build/UKQ1.230804.001;tt-ok/3.12.13.20)",
        APP_VERSION_CODE_STR
    )
}

/// 与注册指纹配套的 UA：版本段引用 [`REGISTER_VERSION_CODE_STR`]，尾段
/// 为 7.4.2.32 起官方统一的 tt-ok 形态（旧 Cronet 尾段在 74232 匿名
/// 请求上已被服务端按旧指纹拒）。与 [`video_ua`] 各自和所属版本身份
/// 成套（注册口径 vs 当前客户端口径），不可混搭。
pub fn register_ua() -> String {
    format!(
        "com.phoenix.read/{} (Linux; U; Android 14; zh_CN; Xiaomi 14; \
         Build/UKQ1.230804.001;tt-ok/3.12.13.20)",
        REGISTER_VERSION_CODE_STR
    )
}

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
    /// 设备维度的 `ttreq` 票（注册响应 Set-Cookie 下发，按 install_id 发放）。
    /// None = 无票（匿名档案零 cookie；注册档案必有票）。不进 `fields`——
    /// 那是要参与签名 query 的，票只该出现在 Cookie。
    #[serde(default)]
    ttreq: Option<String>,
}

fn default_user_agent() -> String {
    video_ua()
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
            ttreq: None,
        }
    }

    /// 挂上注册响应下发的本设备 `ttreq` 票（覆盖语义，None 清除回兜底）。
    pub fn set_ttreq(&mut self, ttreq: Option<String>) {
        self.ttreq = ttreq.filter(|t| !t.is_empty());
    }

    /// 本设备 `ttreq` 票（None = 无票）。重建档案时原样搬运。
    pub fn ttreq(&self) -> Option<&str> {
        self.ttreq.as_deref()
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
    pub fn set(&mut self, key: &str, value: &str) {
        if let Some(slot) = self.fields.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value.to_string();
        } else {
            self.fields.push((key.to_string(), value.to_string()));
        }
    }

    /// 设备注册用的全新档案：版本身份**原生钉注册口径**（
    /// [`REGISTER_VERSION_CODE_STR`] 74232 活形态），UA 为 7.4.2.32
    /// tt-ok 尾段（[`register_ua`]），Xiaomi 14
    /// 指纹，**不带** device_id/iid（首次注册 query 里不能有空值形态的
    /// 字段）。cdid/openudid 由调用方生成后填入——注册请求与注册产物
    /// 档案共用本构造，指纹天然同源；匿名档案经 [`anonymous_profile`]
    /// 对齐到当前客户端口径。
    ///
    /// 不 `#[cfg(test)]` 门控：下游 crate（domain/api/register）的测试要用，
    /// 而依赖 crate 对下游以非 test 方式编译，门控项会消失。
    pub fn fresh_register_profile(cdid: &str, openudid: &str) -> Self {
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
                ("manifest_version_code", REGISTER_VERSION_CODE_STR),
                ("os", "android"),
                ("os_api", "34"),
                ("os_version", "14"),
                ("player_so_load", "1"),
                ("pv_player", REGISTER_VERSION_CODE_STR),
                ("resolution", "1200*2670"),
                ("ssmix", "a"),
                ("update_version_code", REGISTER_VERSION_CODE_STR),
                ("version_code", REGISTER_VERSION_CODE_STR),
                ("version_name", REGISTER_VERSION_NAME),
            ],
            &register_ua(),
        );
        dev.set("cdid", cdid);
        dev.set("openudid", openudid);
        dev
    }

    /// 移除字段（设备注册用：首次注册的 query 里不能带 `device_id=`/
    /// `iid=` 空值形态，字段必须**不存在**而不是空值）。
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

    /// 覆盖档案 UA（注册探针换口径对照用；UA 与字段
    /// 必须成套的约束由调用方保证）。
    pub fn set_user_agent(&mut self, ua: &str) {
        self.user_agent = ua.to_string();
    }
}

/// 匿名游客档案（官方 7.4.2.32 形态）：注册指纹基底去 `openudid`，版本
/// 身份与 UA 对齐当前客户端（[`align_app_version`] + [`video_ua`] 的
/// tt-ok 尾段），`pv_player` 随版本身份同值——
/// **不带 iid/device_id/openudid、零 cookie**，服务端对匿名读播全量放行。
/// `cdid` 由调用方生成并落库，安装期内保持稳定
/// （官方同款行为：pm clear 后身份不变，就是 cdid/指纹没变）。
pub fn anonymous_profile(cdid: &str) -> DeviceProfile {
    let mut p = DeviceProfile::fresh_register_profile(cdid, "");
    p.remove("openudid");
    align_app_version(&mut p);
    p.set("pv_player", APP_VERSION_CODE_STR);
    p.set_user_agent(&video_ua());
    p
}

/// 生成新的客户端 cdid（UUID v4，匿名档案与注册流程共用）。
pub fn fresh_cdid() -> String {
    let b: [u8; 16] = rand::random();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6] & 0x0f | 0x40,
        b[7],
        b[8] & 0x3f | 0x80,
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}

/// 未登录时的匿名 Cookie：reading 系按 `install_id` 风控——**匿名档案
/// （无 iid）必须零 cookie**（官方 virgin 形态，带未知 install_id 反而
/// 被静默拒）；注册档案带自己的 install_id + 票。passport_csrf_token
/// 空值字段只在有 install_id 的形态在场（登录发码语义需要）。
pub fn anonymous_cookie(device: &DeviceProfile) -> String {
    let iid = device.get("iid");
    if iid.is_empty() {
        return String::new();
    }
    let mut cookie = format!(
        "passport_csrf_token=; passport_csrf_token_default=; store-region=cn-gd; store-region-src=did; install_id={iid}"
    );
    if let Some(ttreq) = &device.ttreq {
        cookie.push_str(&format!("; ttreq={ttreq}"));
    }
    cookie
}

/// 当前客户端自报的版本身份（与档案 / 注册模板同源：
/// 见顶部 [`APP_VERSION_CODE`] 常量）。
const APP_VERSION_FIELDS: [(&str, &str); 4] = [
    ("version_code", APP_VERSION_CODE_STR),
    ("version_name", APP_VERSION_NAME),
    ("manifest_version_code", APP_VERSION_CODE_STR),
    ("update_version_code", APP_VERSION_CODE_STR),
];

/// 把设备档案的「App 版本身份」对齐到当前版本（device_id/iid 身份不动）。
///
/// 过旧的版本身份会让服务端下发退化的扁平 schema；本函数把自报版本
/// 对齐到当前客户端。真实设备升级 app 后同样只是自报新版本号，
/// 档案其余字段原样保留。
///
/// **注册产物档案不适用**：注册口径终生钉 [`REGISTER_VERSION_CODE_STR`]，
/// 升当前版本与注册指纹不同源、取流即挂——service 的 adopt 按轮换元数据
/// 跳过本函数。
pub fn align_app_version(profile: &mut DeviceProfile) {
    for (k, v) in APP_VERSION_FIELDS {
        profile.set(k, v);
    }
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
    fn anonymous_profile_has_no_device_identity() {
        let device = anonymous_profile("0f0f0f0f-0000-4000-8000-000000000000");
        assert_eq!(device.get("cdid"), "0f0f0f0f-0000-4000-8000-000000000000");
        // 匿名形态：无 iid/device_id/openudid（官方 7.4.2.32 virgin 形态）
        assert_eq!(device.get("iid"), "");
        assert_eq!(device.get("device_id"), "");
        assert_eq!(device.get("openudid"), "");
        assert_eq!(device.get("version_name"), APP_VERSION_NAME);
        // UA 里的系统版本必须与设备档案的 os_version 一致
        assert_eq!(device.get("os_version"), "14");
        assert!(device.user_agent().contains("Android 14"));
        assert!(device.user_agent().contains(APP_VERSION_CODE_STR));

        let mut keys: Vec<&str> = device.iter().map(|(key, _)| key).collect();
        let total = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), total, "设备档案不应出现重复 key");
    }

    /// 双口径构造钉死：注册档案原生钉注册口径（74232 + tt-ok 尾段），
    /// 匿名档案对齐当前客户端口径（74232 + tt-ok 尾段）——两套身份各自
    /// 成套，改口径只动常量与构造器，不许在调用点散写。
    #[test]
    fn register_and_anonymous_profiles_pin_their_regimes() {
        let reg = DeviceProfile::fresh_register_profile("cdid-uuid", "openudid-hex");
        for k in [
            "version_code",
            "update_version_code",
            "manifest_version_code",
            "pv_player",
        ] {
            assert_eq!(reg.get(k), REGISTER_VERSION_CODE_STR, "{k} 应钉注册口径");
        }
        assert_eq!(reg.get("version_name"), REGISTER_VERSION_NAME);
        assert_eq!(reg.user_agent(), register_ua());
        assert!(reg.user_agent().contains("tt-ok/3.12.13.20"));
        assert_eq!(reg.get("device_id"), "", "首次注册不带 device_id");
        assert_eq!(reg.get("iid"), "");

        let anon = anonymous_profile("cdid-uuid");
        for k in [
            "version_code",
            "update_version_code",
            "manifest_version_code",
            "pv_player",
        ] {
            assert_eq!(anon.get(k), APP_VERSION_CODE_STR, "{k} 应对齐当前客户端");
        }
        assert_eq!(anon.get("version_name"), APP_VERSION_NAME);
        assert_eq!(anon.user_agent(), video_ua());
        assert!(anon.user_agent().contains("tt-ok/"));
    }

    #[test]
    fn fresh_cdid_is_uuid_v4() {
        let cdid = fresh_cdid();
        assert_eq!(cdid.len(), 36);
        assert_eq!(cdid.as_bytes()[14], b'4', "版本号位应为 4");
    }

    #[test]
    fn anonymous_cookie_matches_profile_iid() {
        let cdid = fresh_cdid();
        let mut device = anonymous_profile(&cdid);
        device.set("iid", "4175317900247834");
        let cookie = anonymous_cookie(&device);
        // install_id 必须与档案 iid 一致，否则服务端判为两个设备
        assert!(cookie.contains("install_id=4175317900247834"));
        assert!(
            cookie.contains("passport_csrf_token=;"),
            "空值 csrf 字段必须在场（缺失时 send_code 走换绑分支报 1001）"
        );
        assert!(cookie.contains("store-region=cn-gd"));
        // 匿名档案（无 iid）：官方 virgin 形态零 cookie——带未知 install_id
        // 反而会被静默拒
        let bare = anonymous_profile(&fresh_cdid());
        assert_eq!(anonymous_cookie(&bare), "");
    }

    #[test]
    fn set_overrides_in_place_and_appends_new_keys() {
        let cdid = fresh_cdid();
        let mut device = anonymous_profile(&cdid);
        device.set("iid", "42");
        let idx = device.iter().position(|(k, _)| k == "iid").unwrap();
        device.set("iid", "43");
        // 覆盖发生在原位置，不改变字段顺序
        assert_eq!(device.iter().nth(idx), Some(("iid", "43")));
        // 已存在的 key（cdid 已进匿名档案）走覆盖，不动顺序
        let cdid_pos = device.iter().position(|(k, _)| k == "cdid").unwrap();
        device.set("cdid", "abc");
        assert_eq!(device.iter().nth(cdid_pos), Some(("cdid", "abc")));
        // 档案外的新 key 追加在尾部（注册流程补 openudid 等字段）
        device.set("openudid", "xyz");
        assert_eq!(device.iter().last(), Some(("openudid", "xyz")));
    }

    /// 旧档案启动时对齐版本身份：四个版本字段
    /// 原位覆盖，设备身份（device_id/iid/cdid）与其余字段不动——服务端
    /// 按自报版本号分发功能 schema（过旧版本下排行榜选项表退化为扁平）。
    #[test]
    fn align_app_version_bumps_only_version_fields() {
        let mut legacy = DeviceProfile::from_pairs(
            &[
                ("iid", "1905892595382586"),
                ("device_id", "1905892595378490"),
                ("cdid", "abc-uuid"),
                ("version_code", "71332"),
                ("version_name", "7.1.3.32"),
                ("manifest_version_code", "71332"),
                ("update_version_code", "71332"),
            ],
            "com.phoenix.read/71332 test",
        );
        align_app_version(&mut legacy);
        assert_eq!(legacy.get("version_code"), APP_VERSION_CODE_STR);
        assert_eq!(legacy.get("version_name"), APP_VERSION_NAME);
        assert_eq!(legacy.get("manifest_version_code"), APP_VERSION_CODE_STR);
        assert_eq!(legacy.get("update_version_code"), APP_VERSION_CODE_STR);
        // 身份字段原样
        assert_eq!(legacy.get("iid"), "1905892595382586");
        assert_eq!(legacy.get("device_id"), "1905892595378490");
        assert_eq!(legacy.get("cdid"), "abc-uuid");
        // 字段顺序不受影响（签名 query 顺序敏感的注释契约）
        let mut keys: Vec<&str> = legacy.iter().map(|(k, _)| k).collect();
        keys.truncate(3);
        assert_eq!(keys, vec!["iid", "device_id", "cdid"]);
    }

    #[test]
    fn profile_roundtrips_through_serde() {
        // 设备注册后档案要落库，serde 往返必须无损（含字段顺序）
        let cdid = fresh_cdid();
        let device = anonymous_profile(&cdid);
        let json = serde_json::to_string(&device).unwrap();
        let back: DeviceProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(device, back);
        let order: Vec<String> = back.iter().map(|(k, _)| k.to_string()).collect();
        let order_before: Vec<String> = device.iter().map(|(k, _)| k.to_string()).collect();
        assert_eq!(order, order_before);
    }

    #[test]
    fn legacy_json_without_user_agent_falls_back() {
        // schema 演进容错：旧档没有 user_agent 字段时回落到 video_ua()
        let json = r#"{"fields":[["aid","8662"]]}"#;
        let back: DeviceProfile = serde_json::from_str(json).unwrap();
        assert_eq!(back.user_agent(), video_ua());
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
