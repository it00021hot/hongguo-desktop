//! 设备注册：`POST log5-applog-lf.fqnovel.com/service/2/device_register/`。
//! 服务端按加密 body 的指纹发放 device_id/install_id（播放取流需要服务端「认识」的设备）；
//! TT-Encrypt V5 线上格式与 applog 裸形态详见 docs/hongguo-api-endpoints.md。

mod attempt;
mod model;

pub use attempt::{
    RegisterAttemptError, RegisterBlock, RegisterMaterial, block_from_retry_after,
    load_reuse_profile, register_material, reuse_json_path,
};
pub use model::RegisterResult;

use serde_json::{Value, json};

use hongguo_common::{AppError, AppResult};
use hongguo_signer::device::DeviceProfile;

pub const REGISTER_ORIGIN: &str = "https://log5-applog.fqnovel.com";
pub const REGISTER_PATH: &str = "/service/2/device_register/";

/// 注册请求超时（秒）：applog 发号正常 ~300ms，8 秒容忍慢网
/// （hongguo-drama 同款上限）。
const REGISTER_TIMEOUT_SECS: u64 = 8;

/// TT-Encrypt V5 的 magic（"tc" + 版本 5 + 固定 3 字节）。
const TT_MAGIC: [u8; 6] = [0x74, 0x63, 0x05, 0x10, 0x00, 0x00];

/// Java `GZIPOutputStream` 同款 gzip：mtime=0，XFL/OS 覆写 00/00
///（flate2 默认 02/ff 会被服务端按 zlib 家族头校验静默拒绝）。
fn gzip_java_flavor(plaintext: &[u8]) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use std::io::Write;

    let mut gz = {
        let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(plaintext).expect("gzip 写入内存不可能失败");
        enc.finish().expect("gzip 收尾不可能失败")
    };
    gz[8] = 0;
    gz[9] = 0;
    gz
}

/// TT-Encrypt V5 加密：gzip → 前置完整性哈希 → PKCS7 → AES-CBC → 拼头。
pub fn tt_encrypt_v5(plaintext: &[u8]) -> Vec<u8> {
    tt_encrypt_v5_with_salt(plaintext, &rand::random())
}

/// 同 [`tt_encrypt_v5`]，但盐由调用方指定（对拍/复现用）。
pub fn tt_encrypt_v5_with_salt(plaintext: &[u8], salt: &[u8; 32]) -> Vec<u8> {
    tt_encrypt_v5_full(&gzip_java_flavor(plaintext), salt)
}

/// 加密核心：gz 字节由调用方指定（逐字节锁定测试用）；`iv` 不上线，与 key 一样从 salt 派生。
pub fn tt_encrypt_v5_full(gz: &[u8], salt: &[u8; 32]) -> Vec<u8> {
    use aes::Aes128;
    use aes::cipher::{BlockCipherEncrypt, KeyInit};

    use hongguo_signer::tt_hash::{TT_ORD_LIST, TtHashCore};

    let mut core = TtHashCore::new();
    let h1 = core.calculate(salt);
    let mut seed = h1;
    seed.extend_from_slice(&TT_ORD_LIST);
    let key_iv = core.calculate(&seed);
    let key_arr: [u8; 16] = key_iv[..16].try_into().unwrap();
    let iv_arr: [u8; 16] = key_iv[16..32].try_into().unwrap();

    // payload = gz 的完整 64 字节摘要 ‖ gz（服务端按前 64 字节做完整性校验），再 PKCS7
    let mut payload = core.calculate(gz).to_vec();
    payload.extend_from_slice(gz);
    let pad = 16 - payload.len() % 16;
    payload.extend(std::iter::repeat_n(pad as u8, pad));

    let cipher = Aes128::new((&key_arr).into());
    let mut prev = iv_arr;
    let mut out = Vec::with_capacity(payload.len());
    for chunk in payload.chunks_mut(16) {
        for (b, p) in chunk.iter_mut().zip(&prev) {
            *b ^= p;
        }
        cipher.encrypt_block((&mut *chunk).try_into().unwrap());
        out.extend_from_slice(chunk);
        prev.copy_from_slice(chunk);
    }

    let mut result = Vec::with_capacity(6 + 32 + out.len());
    result.extend_from_slice(&TT_MAGIC);
    result.extend_from_slice(salt);
    result.extend_from_slice(&out);
    result
}

/// 解析注册响应（gzip 由 reqwest 解掉，这里只管 JSON）。
pub(crate) fn parse_register(bytes: &[u8]) -> AppResult<RegisterResult> {
    let v: Value = serde_json::from_slice(bytes)
        .map_err(|e| AppError::Media(format!("注册响应不是 JSON: {e}")))?;
    if let Some(code) = v.get("code").and_then(Value::as_i64)
        && code != 0
    {
        let msg = v
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("未知错误");
        return Err(AppError::Media(format!("注册失败 {code}: {msg}")));
    }
    let get_str = |k: &str| {
        v.get(k)
            .map(|x| match x {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => String::new(),
            })
            .unwrap_or_default()
    };
    let device_id = get_str("device_id_str");
    let device_id = if device_id.is_empty() {
        get_str("device_id")
    } else {
        device_id
    };
    let install_id = get_str("install_id_str");
    let install_id = if install_id.is_empty() {
        get_str("install_id")
    } else {
        install_id
    };
    if device_id == "0" {
        return Err(AppError::Media(
            "注册被服务端静默拒绝（device_id=0）".into(),
        ));
    }
    for (field, value) in [("device_id", &device_id), ("install_id", &install_id)] {
        if !is_registered_id(value) {
            return Err(AppError::Media(format!("注册响应缺少有效的 {field}")));
        }
    }
    Ok(RegisterResult {
        device_id,
        install_id,
        device_token: get_str("device_token"),
        klink_egdi: v
            .get("klink_egdi")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        server_time: v.get("server_time").and_then(Value::as_i64).unwrap_or(0),
        new_user: match v.get("new_user") {
            Some(Value::Bool(b)) => *b,
            Some(Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
            _ => false,
        },
        // 指纹与票由 register_device 补（parse 只有响应体，看不到请求侧身份）
        cdid: String::new(),
        openudid: String::new(),
        ttreq: String::new(),
    })
}

pub fn has_registered_ids(profile: &DeviceProfile) -> bool {
    is_registered_id(profile.get("device_id")) && is_registered_id(profile.get("iid"))
}

fn is_registered_id(value: &str) -> bool {
    value.bytes().all(|byte| byte.is_ascii_digit()) && !value.trim_start_matches('0').is_empty()
}

fn register_query(req_id: &str) -> String {
    hongguo_signer::ticket::encode_query(&[
        ("req_id", req_id.into()),
        ("tt_data", "a".into()),
        ("cronet_version", "8d40f833_2026-03-03".into()),
        ("ttnet_version", "4.2.243.31-douyin".into()),
        ("use_store_region_cookie", "1".into()),
    ])
}

/// 首次安装时间回拨天数上限：真实安装都是过去时（pm clear 不清 firstInstallTime），按 cdid 确定性取 1~30 天。
const FIRST_INSTALL_BACK_MAX_DAYS: u64 = 30;

/// 官方 App 身份字段（7.4.3.32 APK 实值；`sig_hash` = `md5(cert_der)`）。
/// bdinstall：`h90.j`（`sdk_version`/`git_hash`/`sdk_flavor`/`sdk_target_version`/
/// `sdk_version_code`）+ `assets/ss.properties` 的 `release_build`。
const OFFICIAL_DISPLAY_NAME: &str = "红果免费短剧";
const OFFICIAL_SIG_HASH: &str = "56a962410c494bbaf0b58dba20cae56f";
const OFFICIAL_GIT_HASH: &str = "27660d0";
const OFFICIAL_SDK_VERSION: &str = "4.3.2-rc.44";
const OFFICIAL_SDK_VERSION_CODE: i64 = 4_030_294;
const OFFICIAL_SDK_TARGET_VERSION: i64 = 30;
const OFFICIAL_SDK_FLAVOR: &str = "cnInner";
const OFFICIAL_RELEASE_BUILD: &str = "652bb85_20261028";

/// 注册代际：`version_code` + bdinstall SDK 指纹。SDK 常量变更时 bump，
/// 触发库内档案按全新安装语义迁移（旧指纹注册的设备会被静默拒发货）。
pub const REGISTER_REGIME: &str = "74332:4.3.2-rc.44";

/// 构造 AppLog 注册 JSON 明文（官方实流密文体逐字段对齐）。设备耦合字段
/// 取池指纹；App 身份字段钉官方实值；`ipv6_list` 与首次安装时间按 cdid 确定性生成。
fn register_json_str(
    fp: &hongguo_signer::device::RegisterFingerprint,
    id: &RegisterMaterial,
    now_ms: u64,
) -> String {
    let seed = hongguo_signer::md5_hex_upper(id.cdid.as_bytes()).into_bytes();
    let seed_u64 = |a: u64, b: u64| -> u64 {
        u64::from_be_bytes([
            seed[(a % 16) as usize],
            seed[(b % 16) as usize],
            seed[((a + 7) % 16) as usize],
            seed[((b + 5) % 16) as usize],
            seed[((a + 3) % 16) as usize],
            seed[((b + 11) % 16) as usize],
            seed[((a + 1) % 16) as usize],
            seed[((b + 9) % 16) as usize],
        ])
    };
    let back_ms = (seed_u64(0, 1) % (FIRST_INSTALL_BACK_MAX_DAYS * 24 * 3600) + 24 * 3600) * 1000;
    let hex4 = |a: u64, b: u64| format!("{:04X}", (seed_u64(a, b) & 0xffff) as u16);
    let ipv6 = json!([
        {"type": "client_anpi", "value": format!("FE80::{}:{}:{}:{}", hex4(2, 3), hex4(4, 5), hex4(6, 7), hex4(8, 9))},
        {"type": "client_anpi", "value": format!("FD17:625C:F037:2:{}:{}:{}:{}", hex4(10, 11), hex4(12, 13), hex4(14, 15), hex4(0, 5))},
    ]);

    let mut h = serde_json::Map::new();
    h.insert("access".into(), json!("wifi"));
    h.insert("aid".into(), json!(8662));
    h.insert(
        "apk_first_install_time".into(),
        json!(now_ms.saturating_sub(back_ms)),
    );
    h.insert(
        "app_version".into(),
        json!(hongguo_signer::REGISTER_VERSION_NAME),
    );
    h.insert(
        "app_version_minor".into(),
        json!(hongguo_signer::REGISTER_VERSION_NAME),
    );
    h.insert("carrier".into(), json!("CHINA MOBILE"));
    h.insert("cdid".into(), json!(id.cdid));
    h.insert(
        "channel".into(),
        json!(hongguo_signer::device::BUILD_CHANNEL),
    );
    h.insert("clientudid".into(), json!(id.clientudid));
    h.insert("cpu_abi".into(), json!("arm64-v8a"));
    h.insert(
        "custom".into(),
        json!({
            "account_region_src": "fallback",
            "host_bit": 64,
            "account_region": "cn",
            "dragon_device_type": "phone",
        }),
    );
    h.insert("density_dpi".into(), json!(fp.dpi));
    h.insert("device_brand".into(), json!(fp.brand));
    h.insert("device_manufacturer".into(), json!(fp.manufacturer));
    h.insert("device_model".into(), json!(fp.model));
    h.insert("device_platform".into(), json!("android"));
    h.insert("display_density".into(), json!(fp.display_density()));
    h.insert("display_name".into(), json!(OFFICIAL_DISPLAY_NAME));
    h.insert("git_hash".into(), json!(OFFICIAL_GIT_HASH));
    h.insert("guest_mode".into(), json!(0));
    h.insert("ipv6_list".into(), ipv6);
    h.insert("is_system_app".into(), json!(0));
    h.insert("language".into(), json!("zh"));
    h.insert(
        "manifest_version_code".into(),
        json!(hongguo_signer::REGISTER_VERSION_CODE),
    );
    h.insert("mcc_mnc".into(), json!("46000"));
    // h90.g：`not_request_sender = 1`（bdinstall 默认）
    h.insert("not_request_sender".into(), json!(1));
    h.insert("oaid_may_support".into(), json!(false));
    h.insert("openudid".into(), json!(id.openudid));
    h.insert("os".into(), json!("Android"));
    h.insert("os_api".into(), json!(fp.api.parse::<u32>().unwrap_or(34)));
    h.insert("os_version".into(), json!(fp.os));
    h.insert("package".into(), json!("com.phoenix.read"));
    h.insert("pre_installed_channel".into(), json!(""));
    h.insert("region".into(), json!("CN"));
    h.insert("release_build".into(), json!(OFFICIAL_RELEASE_BUILD));
    h.insert("req_id".into(), json!(id.req_id));
    h.insert("resolution".into(), json!(fp.resolution_hxw()));
    h.insert("rom".into(), json!(fp.rom));
    h.insert("rom_version".into(), json!(fp.rom_version));
    h.insert("sdk_flavor".into(), json!(OFFICIAL_SDK_FLAVOR));
    h.insert(
        "sdk_target_version".into(),
        json!(OFFICIAL_SDK_TARGET_VERSION),
    );
    h.insert("sdk_version".into(), json!(OFFICIAL_SDK_VERSION));
    h.insert("sdk_version_code".into(), json!(OFFICIAL_SDK_VERSION_CODE));
    h.insert("sig_hash".into(), json!(OFFICIAL_SIG_HASH));
    h.insert("sim_region".into(), json!("cn"));
    h.insert("sim_serial_number".into(), json!([]));
    h.insert("timezone".into(), json!(8));
    h.insert("tz_name".into(), json!("Asia/Shanghai"));
    h.insert("tz_offset".into(), json!(28800));
    h.insert(
        "update_version_code".into(),
        json!(hongguo_signer::REGISTER_VERSION_CODE),
    );
    h.insert(
        "version_code".into(),
        json!(hongguo_signer::REGISTER_VERSION_CODE),
    );
    let mut o = serde_json::Map::new();
    o.insert("magic_tag".into(), json!("ss_app_log"));
    o.insert("header".into(), Value::Object(h));
    o.insert("_gen_time".into(), json!(now_ms));
    Value::Object(o).to_string()
}
