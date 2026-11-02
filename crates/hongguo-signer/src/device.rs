//! 设备指纹。
//!
//! 签名头与 query 参数必须来自同一份设备档案：Medusa 会把 device_id 和
//! version_name 打进密文，服务端两边比对，不一致直接判为伪造。
//!
//! 档案有 owned 形态 [`DeviceProfile`]（可从数据库装载、可被设备注册流程
//! 改写字段）；游客身份走 [`anonymous_profile`]——当前官方版本起
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

/// 注册指纹钉定的版本身份（2026-11-01 竞品实流抓包逐字节对齐：
/// 注册请求、注册产物档案、取流链全程 73932/7.3.9.32——注册 JSON 里
/// `sdk_version 3.7.3-rc.116-douyin` 等.bdinstall 代际与该版本成套，
/// 服务端按「版本 + SDK 代际自洽」给设备做信任评分，身份与代际错配的
/// 注册会被静默标记、后续取流不发货）。注册请求与注册产物档案
/// **终生钉这套身份**，不随 [`APP_VERSION_CODE`] 走——升当前版本与注册
/// 指纹不同源，取流即挂。
pub const REGISTER_VERSION_CODE: u64 = 73932;
pub const REGISTER_VERSION_CODE_STR: &str = "73932";
pub const REGISTER_VERSION_NAME: &str = "7.3.9.32";

/// 与设备档案配套的 UA（App 版本段引用 [`APP_VERSION_CODE_STR`]，
/// 机型段与档案指纹一致，改机型要整组同换）。
///
/// UA 尾段为官方统一的 `tt-ok/3.12.13.20` 形态，
/// 匿名请求的机型段会在服务端过风控名单，跟着换。
pub fn video_ua() -> String {
    format!(
        "com.phoenix.read/{} (Linux; U; Android 14; zh_CN; Xiaomi 14; \
         Build/UKQ1.230804.001;tt-ok/3.12.13.20)",
        APP_VERSION_CODE_STR
    )
}

// ---------------------------------------------------------------- 注册机型指纹池

/// 注册机型指纹：注册 query / AppLog JSON / 产物档案 / UA 里**机型耦合**
/// 的字段组（品牌/型号/市场名/制造商/Build/OS/dpi/分辨率/ROM），整组同源。
///
/// 2026-11-02 多机型池化：此前全量用户都顶着同一套「Xiaomi 14」指纹，
/// 共享暴露面是注册抽奖（发货资格在注册时抽签发放，见 service 取流健康
/// 守护的取证）的候选权重，池化把共享面摊开。池内全部是公开市场参数的
/// **真实机型组合**；首项 = 2026-11-01 逐字节取证的小米 14 原形态
/// （cdid 哈希命中即与历史行为完全一致）。
///
/// 指纹由 **cdid 确定性派生**（[`fingerprint_for_cdid`]）：注册请求、注册
/// 产物档案、后续重建从同一 cdid 推出同一指纹，两侧天然同源；cdid 本身是
/// 随机 UUID，注册自然散布全池。传感器读数的 protobuf（[`device_proto`]）
/// **不在池内**——它本就是另一台 Redmi 真机的采集值，与小米 14 档案混搭
/// 发货正常（错配被服务端容忍），且无其他机型的 proto 抓包，不盲猜。
pub struct RegisterFingerprint {
    /// device_brand 与渠道品牌段（渠道 = `{brand}_8662_64`）
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

/// release_build 的时间戳尾巴（注册 JSON 实流形态）
const REGISTER_BUILD_STAMP: &str = "1701234567";

impl RegisterFingerprint {
    /// 渠道号：品牌段 + aid + abi 位（`xiaomi_8662_64` 同构）
    pub fn channel(&self) -> String {
        format!("{}_8662_64", self.brand)
    }

    /// 注册口径 UA（Cronet 尾段 + 注册版本身份）
    pub fn register_ua(&self) -> String {
        format!(
            "com.phoenix.read/{REGISTER_VERSION_CODE_STR} (Linux; U; Android {}; zh_CN; {}; \
             Build/{}; Cronet/TTNetVersion:8d40f833 QuicVersion:462f352c 2026-08-31)",
            self.os, self.display, self.build
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

    /// AppLog `custom.web_ua`（WebView 形态）
    pub fn web_ua(&self) -> String {
        format!(
            "Mozilla/5.0 (Linux; Android {}; {} Build/{}; wv) AppleWebKit/537.36 \
             (KHTML, like Gecko) Version/4.0 Chrome/120.0.0.0 Mobile Safari/537.36",
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

    /// AppLog release_build（Build 号 + 实流时间戳尾巴）
    pub fn release_build(&self) -> String {
        format!("{}_{}", self.build, REGISTER_BUILD_STAMP)
    }
}

/// 十台常用机型（小米系 3 / vivo 系 3 / OPPO 一加 2 / 荣耀 / 三星），
/// 全部 Android 14（os_api 34）——与注册口径的 SDK 代际取证同版。
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

    /// 设备注册用的全新档案：版本身份**原生钉注册口径**
    /// （[`REGISTER_VERSION_CODE_STR`]），UA 为注册口径 Cronet 尾段，
    /// 机型指纹由 cdid 从 [`REGISTER_FINGERPRINTS`] 池确定性抽取
    /// （[`fingerprint_for_cdid`]），**不带** device_id/iid（首次注册 query
    /// 里不能有空值形态的字段）。cdid/openudid 由调用方生成后填入——注册
    /// 请求与注册产物档案共用本构造，指纹天然同源；匿名档案经
    /// [`anonymous_profile`] 对齐到当前客户端口径。
    ///
    /// 不 `#[cfg(test)]` 门控：下游 crate（domain/api/register）的测试要用，
    /// 而依赖 crate 对下游以非 test 方式编译，门控项会消失。
    pub fn fresh_register_profile(cdid: &str, openudid: &str) -> Self {
        Self::register_profile_with(fingerprint_for_cdid(cdid), cdid, openudid)
    }

    /// 用指定机型指纹构造注册档案（[`fresh_register_profile`] 的显式指纹
    /// 形态；注册请求与产物档案对同一 cdid 必须取同一指纹，由
    /// [`fingerprint_for_cdid`] 的确定性派生保证）。
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

/// 匿名游客档案（官方匿名形态）：注册指纹基底去 `openudid`，版本
/// 身份与 UA 对齐当前客户端（[`align_app_version`] + 指纹自身的
/// tt-ok 尾段 UA），`pv_player` 随版本身份同值——
/// **不带 iid/device_id/openudid、零 cookie**，服务端对匿名读播全量放行。
/// `cdid` 由调用方生成并落库，安装期内保持稳定
/// （官方同款行为：pm clear 后身份不变，就是 cdid/指纹没变）。
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
        // 匿名形态：无 iid/device_id/openudid（官方 virgin 形态）
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

    /// 双口径构造钉死：注册档案原生钉注册口径（Cronet 尾段，2026-11-01
    /// 实流取证形态），匿名档案对齐当前客户端口径（tt-ok 尾段）——两套
    /// 身份各自成套，改口径只动常量与构造器，不许在调用点散写。
    /// 机型耦合字段与 UA 必须来自同一预设（cdid 确定性派生）。
    #[test]
    fn register_and_anonymous_profiles_pin_their_regimes() {
        let fp = fingerprint_for_cdid("cdid-uuid");
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
        assert_eq!(reg.user_agent(), fp.register_ua());
        assert!(
            reg.user_agent().contains("Cronet/TTNetVersion:"),
            "注册口径 UA 尾段必须是 Cronet/TTNet 形态（实流取证）"
        );
        // 机型组同源：brand/model/channel 与指纹预设一一对应
        assert_eq!(reg.get("device_brand"), fp.brand);
        assert_eq!(reg.get("device_type"), fp.model);
        assert_eq!(reg.get("channel"), fp.channel());

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
        assert_eq!(anon.user_agent(), fp.video_ua());
        assert!(anon.user_agent().contains("tt-ok/"));
    }

    /// 指纹池与派生：池非空且预设互不相同；同一 cdid 恒得同一指纹，
    /// 不同 cdid 有分散度（不允许全池塌缩到单项）。
    #[test]
    fn fingerprint_pool_is_diverse_and_derivation_is_deterministic() {
        assert!(REGISTER_FINGERPRINTS.len() >= 8, "池太小就失去池化意义");
        for (i, a) in REGISTER_FINGERPRINTS.iter().enumerate() {
            assert!(
                !a.model.is_empty() && !a.brand.is_empty(),
                "预设 {i} 字段不全"
            );
            for b in REGISTER_FINGERPRINTS.iter().skip(i + 1) {
                assert_ne!(a.model, b.model, "池内型号代码必须互不相同");
            }
        }
        let a = fingerprint_for_cdid("cdid-one");
        let b = fingerprint_for_cdid("cdid-one");
        assert!(std::ptr::eq(a, b), "同一 cdid 派生必须确定");
        // 分散度：10 个不同 cdid 应命中多个预设（md5 均匀性）。判等用
        // model（池内互异已断言），不用指针——const 引用重物化下指针
        // 相等不可靠。
        let hits: std::collections::HashSet<&str> = (0..10)
            .map(|i| fingerprint_for_cdid(&format!("cdid-{i}")).model)
            .collect();
        assert!(
            hits.len() >= 3,
            "10 个 cdid 只命中 {} 个预设，分散度不足",
            hits.len()
        );
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
                ("version_code", "60000"),
                ("version_name", "6.0.0.0"),
                ("manifest_version_code", "60000"),
                ("update_version_code", "60000"),
            ],
            "com.phoenix.read/60000 test",
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
