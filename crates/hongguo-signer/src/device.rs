//! 设备指纹：签名头与 query 参数必须来自同一份设备档案——Medusa 会把 device_id 和
//! version_name 写进密文，服务端两边比对，不一致直接判为伪造。
//! 游客身份走 [`anonymous_profile`]：当前官方版本未登录读播不需要设备注册。

use serde::{Deserialize, Serialize};

use crate::md5_hex_upper;
use crate::protobuf::{FieldType, FieldValue, proto};

/// 应用 id 与渠道号，Medusa 密文里硬编码。
pub const APP_ID: &str = "8662";
pub const CHANNEL_ID: &str = "1588093228";

/// 官方客户端版本身份（全仓唯一出处，query / 注册模板 / UA 一律引用这三常量，禁止散写魔法值）。
/// 7.4.3.32 `SingleAppContext.getVersionCode` 写死 `0x1225c`，`getVersion` 写死 `7.4.3.32`。
/// `NetUtil.putCommonParamsWithLevel` 的 `version_code` / `version_name` 走这两个方法，
/// 不读 manifest meta。`manifest_version_code` 是 `PackageInfo.versionCode`（同样 74332），
/// `update_version_code` 是 manifest meta `UPDATE_VERSION_CODE`（同样 74332）。
pub const APP_VERSION_CODE: u64 = 74332;
pub const APP_VERSION_CODE_STR: &str = "74332";
pub const APP_VERSION_NAME: &str = "7.4.3.32";

/// 官方 App 的构建渠道。7.4.3.32 `assets/ss.properties` 的 `meta_umeng_channel=update_64`，
/// `SingleAppContext.getChannel` 读它，`NetUtil` 写进 query 的 `channel`。
pub const BUILD_CHANNEL: &str = "update_64";

/// 注册指纹钉定的版本身份。这个包的注册和取流用同一套 `getVersion` / `getVersionCode`，
/// 两边必须同值：Medusa 把 `version_name` 写进密文，和 query 不一致会被判伪造。
pub const REGISTER_VERSION_CODE: u64 = 74332;
pub const REGISTER_VERSION_CODE_STR: &str = "74332";
pub const REGISTER_VERSION_NAME: &str = "7.4.3.32";

/// 与设备档案配套的 UA（App 版本段引用 [`APP_VERSION_CODE_STR`]，机型段与档案指纹一致，改机型要整组同换）。
/// UA 尾段为官方统一的 `tt-ok/3.12.13.20` 形态；匿名请求的机型段会在服务端过风控名单，跟着换。
pub fn video_ua() -> String {
    format!(
        "com.phoenix.read/{} (Linux; U; Android 14; zh_CN; Xiaomi 14; \
         Build/UKQ1.230804.001;tt-ok/3.12.13.20)",
        APP_VERSION_CODE_STR
    )
}

/// 注册机型指纹：注册 query / AppLog JSON / 产物档案 / UA 里**机型耦合**
/// 的字段组（品牌/型号/市场名/制造商/Build/OS/dpi/分辨率/ROM），整组同源；
/// 池内全部是公开市场参数的真实机型组合。
/// 指纹由 **cdid 确定性派生**（[`fingerprint_for_cdid`]）：注册请求、注册
/// 产物档案、后续重建从同一 cdid 推出同一指纹。传感器读数的 protobuf
/// （[`device_proto`]）不在池内——那是另一台 Redmi 真机的采集值，错配被服务端容忍。
pub struct RegisterFingerprint {
    /// device_brand（渠道已改为官方构建渠道，见 [`Self::channel`]）
    pub brand: &'static str,
    /// device_type：型号代码
    pub model: &'static str,
    /// UA 机型段与 AppLog device_model 的市场名
    pub display: &'static str,
    /// device_manufacturer
    pub manufacturer: &'static str,
    /// Build 号
    pub build: &'static str,
    /// os_version（Android 大版本）
    pub os: &'static str,
    /// os_api
    pub api: &'static str,
    /// dpi
    pub dpi: u32,
    /// 分辨率（query 形态 宽*高；AppLog 用 高x宽，见 [`Self::resolution_hxw`]）
    pub resolution: &'static str,
    /// ROM 标识（AppLog rom 字段）
    pub rom: &'static str,
    /// ROM 版本（AppLog rom_version 字段）
    pub rom_version: &'static str,
}

impl RegisterFingerprint {
    /// 渠道号：官方 App 的**构建渠道** [`BUILD_CHANNEL`]（注册体 / settings query / 取流 query 三处一致；品牌渠道形态是竞品）。
    pub fn channel(&self) -> String {
        BUILD_CHANNEL.to_string()
    }

    /// 注册口径 UA（tt-ok 尾段 + 注册版本身份 + 型号代码；此前 Cronet 尾段 + 市场名的形态是竞品 UA）。
    pub fn register_ua(&self) -> String {
        format!(
            "com.phoenix.read/{REGISTER_VERSION_CODE_STR} (Linux; U; Android {}; zh_CN_#Hans; {}; \
             Build/{};tt-ok/3.12.13.20)",
            self.os, self.model, self.build
        )
    }

    /// 当前客户端口径 UA（tt-ok 尾段 + 当前版本身份）
    pub fn video_ua(&self) -> String {
        format!(
            "com.phoenix.read/{APP_VERSION_CODE_STR} (Linux; U; Android {}; zh_CN; {}; \
             Build/{};tt-ok/3.12.13.20)",
            self.os, self.display, self.build
        )
    }

    /// display_density 按 dpi 档位映射（xxxhdpi 520+ / xxhdpi 400+ / hdpi）
    pub fn display_density(&self) -> &'static str {
        if self.dpi >= 520 {
            "xxxhdpi"
        } else if self.dpi >= 400 {
            "xxhdpi"
        } else {
            "hdpi"
        }
    }

    /// 分辨率的 AppLog 形态（高x宽）
    pub fn resolution_hxw(&self) -> String {
        let (w, h) = self
            .resolution
            .split_once('*')
            .unwrap_or((self.resolution, self.resolution));
        format!("{h}x{w}")
    }
}

/// 十台常用机型（小米系 3 / vivo 系 3 / OPPO 一加 2 / 荣耀 / 三星），全部 Android 14（os_api 34），与注册口径的 SDK 代际同版。
pub const REGISTER_FINGERPRINTS: &[RegisterFingerprint] = &[
    RegisterFingerprint {
        brand: "xiaomi",
        model: "23127PN0CC",
        display: "Xiaomi 14",
        manufacturer: "Xiaomi",
        build: "UKQ1.230804.001",
        os: "14",
        api: "34",
        dpi: 460,
        resolution: "1200*2670",
        rom: "hyperos",
        rom_version: "OS1.0.32.0.UNCCNXM",
    },
    RegisterFingerprint {
        brand: "redmi",
        model: "23113RKC6C",
        display: "Redmi K70",
        manufacturer: "Xiaomi",
        build: "UKQ1.230804.001",
        os: "14",
        api: "34",
        dpi: 560,
        resolution: "1440*3200",
        rom: "hyperos",
        rom_version: "OS1.0.30.0.VNCCNXM",
    },
    RegisterFingerprint {
        brand: "redmi",
        model: "2312DRA50C",
        display: "Redmi Note 13 Pro",
        manufacturer: "Xiaomi",
        build: "UKQ1.230804.001",
        os: "14",
        api: "34",
        dpi: 440,
        resolution: "1080*2400",
        rom: "hyperos",
        rom_version: "OS1.0.32.0.UNCCNXM",
    },
    RegisterFingerprint {
        brand: "iqoo",
        model: "V2310A",
        display: "iQOO Neo9",
        manufacturer: "vivo",
        build: "UKQ1.230917.001",
        os: "14",
        api: "34",
        dpi: 480,
        resolution: "1260*2800",
        rom: "originos",
        rom_version: "OriginOS 4.0.5",
    },
    RegisterFingerprint {
        brand: "vivo",
        model: "V2312A",
        display: "vivo X100",
        manufacturer: "vivo",
        build: "UKQ1.230917.001",
        os: "14",
        api: "34",
        dpi: 460,
        resolution: "1260*2800",
        rom: "originos",
        rom_version: "OriginOS 4.0.5",
    },
    RegisterFingerprint {
        brand: "vivo",
        model: "V2323A",
        display: "vivo S18",
        manufacturer: "vivo",
        build: "UKQ1.230917.001",
        os: "14",
        api: "34",
        dpi: 460,
        resolution: "1260*2800",
        rom: "originos",
        rom_version: "OriginOS 4.0.5",
    },
    RegisterFingerprint {
        brand: "oppo",
        model: "PHZ110",
        display: "OPPO Find X7",
        manufacturer: "OPPO",
        build: "UKQ1.230917.001",
        os: "14",
        api: "34",
        dpi: 480,
        resolution: "1264*2780",
        rom: "coloros",
        rom_version: "ColorOS 14.0.1",
    },
    RegisterFingerprint {
        brand: "oneplus",
        model: "PJD110",
        display: "OnePlus 12",
        manufacturer: "OnePlus",
        build: "UKQ1.230917.001",
        os: "14",
        api: "34",
        dpi: 520,
        resolution: "1440*3168",
        rom: "coloros",
        rom_version: "ColorOS 14.0.0.200",
    },
    RegisterFingerprint {
        brand: "honor",
        model: "BVL-AN00",
        display: "BVL-AN00",
        manufacturer: "HONOR",
        build: "UKQ1.230917.002",
        os: "14",
        api: "34",
        dpi: 460,
        resolution: "1280*2800",
        rom: "magicos",
        rom_version: "MagicOS 8.0.0.130",
    },
    RegisterFingerprint {
        brand: "samsung",
        model: "SM-S9210",
        display: "SM-S9210",
        manufacturer: "samsung",
        build: "UP1A.231005.007",
        os: "14",
        api: "34",
        dpi: 450,
        resolution: "1080*2340",
        rom: "oneui",
        rom_version: "One UI 6.1",
    },
];

/// cdid → 指纹的确定性派生（md5 前 8 位取模）：同一 cdid 恒得同一指纹，
/// 注册请求、注册产物档案、后续重建三处同源的机制保证。
pub fn fingerprint_for_cdid(cdid: &str) -> &'static RegisterFingerprint {
    let digest = crate::md5_hex_upper(cdid.as_bytes());
    let idx = usize::from_str_radix(&digest[..8], 16).unwrap_or(0) % REGISTER_FINGERPRINTS.len();
    &REGISTER_FINGERPRINTS[idx]
}

/// 视频 CDN 403 时补上的官网 Referer（直链被拒的最后手段）。
/// CDN 防盗链两类规则并存：主流边缘对**任何**带 Referer 的请求直接 403（首选裸 UA），
/// 个别节点却反过来要求 Referer——403 后补上它再试一次，两类都能过。
pub const VIDEO_REFERER: &str = "https://novelquickapp.com/";

/// 一份设备档案。
///
/// 字段是**保序**键值对：顺序就是签名 query 的参数顺序，签名对它敏感，绝不排序、不去重，只按构造顺序搬运；
/// `user_agent` 与字段必须成套（系统版本号两边一致），改机型要整组换。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceProfile {
    #[serde(default)]
    fields: Vec<(String, String)>,
    #[serde(default = "default_user_agent")]
    user_agent: String,
    /// 设备维度的 `ttreq` 票（注册响应 Set-Cookie 下发，按 install_id 发放）。
    /// None = 无票（匿名档案零 cookie；注册档案必有票）。不进 `fields`——那是参与签名 query 的，票只该出现在 Cookie。
    #[serde(default)]
    ttreq: Option<String>,
}

fn default_user_agent() -> String {
    video_ua()
}

impl DeviceProfile {
    /// 从动态键值对构造（注册指纹的渠道号等字段是运行时拼装的）。
    pub fn from_owned_fields(fields: Vec<(String, String)>, user_agent: String) -> Self {
        Self {
            fields,
            user_agent,
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

    /// 设备注册用的全新档案：版本身份与 UA 钉注册口径，机型指纹由 cdid 从
    /// [`REGISTER_FINGERPRINTS`] 池确定性抽取，**不带** device_id/iid（首次注册
    /// query 里不能有空值形态的字段）；cdid/openudid 由调用方生成后填入，注册
    /// 请求与产物档案共用本构造。
    pub fn fresh_register_profile(cdid: &str, openudid: &str) -> Self {
        Self::register_profile_with(fingerprint_for_cdid(cdid), cdid, openudid)
    }

    /// 用指定机型指纹构造注册档案（同一 cdid 两侧必须同指纹，由 [`fingerprint_for_cdid`] 的确定性派生保证）。
    pub fn register_profile_with(fp: &RegisterFingerprint, cdid: &str, openudid: &str) -> Self {
        let mut dev = Self::from_owned_fields(
            vec![
                ("ac".into(), "wifi".into()),
                ("aid".into(), "8662".into()),
                ("app_name".into(), "novelread".into()),
                ("channel".into(), fp.channel()),
                ("compliance_status".into(), "0".into()),
                ("device_brand".into(), fp.brand.into()),
                ("device_platform".into(), "android".into()),
                ("device_type".into(), fp.model.into()),
                ("dpi".into(), fp.dpi.to_string()),
                ("dragon_device_type".into(), "phone".into()),
                ("host_abi".into(), "arm64-v8a".into()),
                ("is_android_pad_screen".into(), "0".into()),
                ("language".into(), "zh".into()),
                (
                    "manifest_version_code".into(),
                    REGISTER_VERSION_CODE_STR.into(),
                ),
                ("os".into(), "android".into()),
                ("os_api".into(), fp.api.into()),
                ("os_version".into(), fp.os.into()),
                ("player_so_load".into(), "1".into()),
                ("pv_player".into(), REGISTER_VERSION_CODE_STR.into()),
                ("resolution".into(), fp.resolution.into()),
                ("ssmix".into(), "a".into()),
                (
                    "update_version_code".into(),
                    REGISTER_VERSION_CODE_STR.into(),
                ),
                ("version_code".into(), REGISTER_VERSION_CODE_STR.into()),
                ("version_name".into(), REGISTER_VERSION_NAME.into()),
            ],
            fp.register_ua(),
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

/// 匿名游客档案（官方匿名形态）：注册指纹基底去 `openudid`，版本身份与 UA 对齐当前客户端，
/// `pv_player` 随版本身份同值——**不带 iid/device_id/openudid、零 cookie**，服务端对匿名读播全量放行。
/// `cdid` 由调用方生成并落库，安装期内保持稳定（官方 pm clear 后身份不变，就是 cdid/指纹没变）。
pub fn anonymous_profile(cdid: &str) -> DeviceProfile {
    let fp = fingerprint_for_cdid(cdid);
    let mut p = DeviceProfile::register_profile_with(fp, cdid, "");
    p.remove("openudid");
    align_app_version(&mut p);
    p.set("pv_player", APP_VERSION_CODE_STR);
    p.set_user_agent(&fp.video_ua());
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

/// 未登录时的匿名 Cookie：reading 系按 `install_id` 风控——**匿名档案（无 iid）必须零 cookie**
/// （带未知 install_id 反而被静默拒）；注册档案带自己的 install_id + 票。
/// passport_csrf_token 空值字段只在有 install_id 的形态在场（登录发码语义需要）。
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

/// 当前客户端自报的版本身份（与档案 / 注册模板同源，见 [`APP_VERSION_CODE`]）。
const APP_VERSION_FIELDS: [(&str, &str); 4] = [
    ("version_code", APP_VERSION_CODE_STR),
    ("version_name", APP_VERSION_NAME),
    ("manifest_version_code", APP_VERSION_CODE_STR),
    ("update_version_code", APP_VERSION_CODE_STR),
];

/// 把设备档案的「App 版本身份」对齐到当前版本（device_id/iid 身份不动）：过旧的版本身份会让服务端
/// 下发退化的扁平 schema；渠道（`channel`）同为构建身份，存量旧渠道值在此归一到 [`BUILD_CHANNEL`]。
/// **注册产物档案不适用**（service 的 adopt 按轮换元数据跳过本函数走 `heal_registered_profile`）。
pub fn align_app_version(profile: &mut DeviceProfile) {
    for (k, v) in APP_VERSION_FIELDS {
        profile.set(k, v);
    }
    profile.set("channel", BUILD_CHANNEL);
}

/// 构造设备信息 protobuf（Medusa message 的 field 12）。
///
/// 机型组（30~36）按 cdid 确定性恢复**池指纹**，与注册档案/取流 query 同一套品牌型号
/// （官方风控可解出该 proto 与注册身份比对，同套才是同源）；传感器浮点（16~21）在已验证
/// 发货的量级上按 device_id 确定性抖动 ±3%；字段 5 的 26 位设备标识与 25~28 的
/// 安装/启动时间戳同为 device_id 确定性派生。
pub fn device_proto(cdid: &str, device_id: &str) -> Vec<u8> {
    let fp = fingerprint_for_cdid(cdid);
    let seed = md5_hex_upper(device_id.as_bytes()).into_bytes();
    let jitter = |i: usize, base: f32| -> f32 {
        let n = (f32::from(seed[i % 32]) - 128.0) / 128.0;
        base * (1.0 + n * 0.03)
    };
    const ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut id26 = String::with_capacity(26);
    for i in 0..26 {
        let k = (usize::from(seed[i % 32]) ^ (i * 31)) % ALNUM.len();
        id26.push(ALNUM[k] as char);
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    // 安装时间：1~30 天前，确定性（与注册体 apk_first_install_time 同语义）
    let days_back = (i64::from(seed[4] % 30) + 1) * 86_400_000;
    let first = now_ms - days_back;

    proto(&[
        (1, FieldValue::SInt(1), FieldType::SInt),
        (2, FieldValue::SInt(2), FieldType::SInt),
        (3, FieldValue::Str(APP_ID.to_string()), FieldType::Str),
        (4, FieldValue::Str(device_id.to_string()), FieldType::Str),
        (5, FieldValue::Str(id26), FieldType::Str),
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
            FieldValue::Float(jitter(0, 255.249_94)),
            FieldType::Float,
        ),
        (
            17,
            FieldValue::Float(jitter(1, 35.585_99)),
            FieldType::Float,
        ),
        (
            18,
            FieldValue::Float(jitter(2, 3.467_449_2)),
            FieldType::Float,
        ),
        (
            19,
            FieldValue::Float(jitter(3, 3.467_449_2)),
            FieldType::Float,
        ),
        (
            20,
            FieldValue::Float(jitter(4, 255.175_49)),
            FieldType::Float,
        ),
        (
            21,
            FieldValue::Float(jitter(5, 42.175_44)),
            FieldType::Float,
        ),
        (22, FieldValue::Str("16".to_string()), FieldType::Str),
        (23, FieldValue::SInt(41), FieldType::SInt),
        (24, FieldValue::SInt(36), FieldType::SInt),
        (25, FieldValue::SInt(first), FieldType::SInt),
        (26, FieldValue::SInt(first), FieldType::SInt),
        (27, FieldValue::SInt(first), FieldType::SInt),
        (28, FieldValue::SInt(first + 2), FieldType::SInt),
        (29, FieldValue::SInt(-1), FieldType::SInt),
        (30, FieldValue::Str(fp.model.to_string()), FieldType::Str),
        (31, FieldValue::Str(fp.brand.to_string()), FieldType::Str),
        (32, FieldValue::Str(fp.model.to_string()), FieldType::Str),
        (33, FieldValue::Str(fp.model.to_string()), FieldType::Str),
        (
            34,
            FieldValue::Str(fp.manufacturer.to_string()),
            FieldType::Str,
        ),
        (35, FieldValue::Str(fp.brand.to_string()), FieldType::Str),
        (36, FieldValue::Str(fp.brand.to_string()), FieldType::Str),
        (38, FieldValue::SInt(31), FieldType::SInt),
    ])
}
