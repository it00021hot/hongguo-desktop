// 探测期的形态告警统一压制（启动接线后随清理移除）
#![allow(
    dead_code,
    unused_mut,
    clippy::needless_range_loop,
    clippy::double_parens,
    clippy::manual_repeat_n,
    clippy::needless_borrows_for_generic_args,
    clippy::range_plus_one
)]

//! 设备注册：`POST log.snssdk.com/service/2/device_register/`。
//!
//! 注册是整个 App API 身份链的起点：服务端按 query 里的 cdid/openudid
//! 指纹发放 `device_id` / `install_id`，之后所有业务请求的设备参数与
//! cookie（install_id/ttreq）都从这来。没有注册，bookmall 系接口在
//! 静态档案上直接 ILLEGAL_ACCESS 110。
//!
//! ## body 形态（抓包 + 逆向确认）
//!
//! 明文是 AppLog 的注册 JSON：`{"magic_tag":"ss_app_log","header":{…47 个
//! 指纹字段…},"_gen_time":ms}`，gzip 压缩后套 **TT-Encrypt V5**
//! （`content-type: application/octet-stream;tt-data=a`）：
//!
//! ```text
//! 输出 = magic(6, "tc"+v5) ‖ salt(32 随机) ‖ iv(16 随机，服务端按 header
//!        里的 iv 解密，不校验派生) ‖ AES-128-CBC(payload + PKCS7)
//! key = 魔改哈希( 魔改哈希(salt) ‖ ORD_LIST(64) )[0..16]；
//! payload = 魔改哈希(gzip)[16..64]（48B 完整性哈希，**不是**前 48 字节，
//! 取错窗口服务端重算不符即静默拒绝 device_id=0）‖ gzip(明文)；
//! gzip 头的 XFL/OS 必须覆写为 00/00（zlib 家族头），flate2 默认的 02/ff 会被拒
//! ```
//!
//! 算法经逆向考证（Go 符号表 `crypto.TTEncryptV5`）及
//! TikTok 固件逆向的公开实现互证；`register_req_real.bin` 是真机
//! 抓包的注册 body，`tt_v5_real_capture_roundtrip` 用它锁死解密链。
//! 历史教训：明文 form / gzip form 会被服务端静默降级成 `device_id:0`，
//! 必须走加密形态。

mod model;

pub use model::RegisterResult;

use serde_json::{Value, json};

#[cfg(test)]
use super::client::api_call_full;
use super::client::{ApiEnv, api_call_full_response};
use hongguo_common::{AppError, AppResult};

pub const REGISTER_ORIGIN: &str = "https://log.snssdk.com";
pub const REGISTER_PATH: &str = "/service/2/device_register/";

/// TT-Encrypt V5 的 magic（"tc" + 版本 5 + 固定 3 字节）。
const TT_MAGIC: [u8; 6] = [0x74, 0x63, 0x05, 0x10, 0x00, 0x00];

/// Java `GZIPOutputStream` 同款 gzip：mtime=0，XFL/OS 两字节覆写 00/00
/// （真机抓包样本的 gzip 头就是 00/00；flate2 默认写 02/ff，不覆写会被
/// 服务端按 zlib 家族头校验静默拒绝）。
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
    // IV 随机并原样放进 header（服务端按 header 里的 IV 解密，不校验其
    // 与 salt 的派生关系——真机抓包与历史 Python 对拍的 IV 均非派生值）
    let iv: [u8; 16] = rand::random();
    tt_encrypt_v5_full(&gzip_java_flavor(plaintext), salt, &iv)
}

/// 加密核心：gz 字节与 IV 均由调用方指定（对拍矩阵用——隔离 gz 字节流
/// 与 IV 派生这两个独立变量）。
pub fn tt_encrypt_v5_full(gz: &[u8], salt: &[u8; 32], iv: &[u8; 16]) -> Vec<u8> {
    use aes::Aes128;
    use aes::cipher::{BlockCipherEncrypt, KeyInit};

    use hongguo_signer::tt_hash::{TT_ORD_LIST, TtHashCore};

    // 盐 + 密钥派生。三次 calculate 共用一个 TtHashCore 只是沿用参考
    // 实现的调用形态——CF 进位在每次 calculate 内部都先写后读，跨调用
    // 不残留，共享核与独立核结果一致（tt_hash 的
    // shared_core_matches_fresh_cores 测试锁定）。
    let mut core = TtHashCore::new();
    let h1 = core.calculate(salt);
    let mut seed = h1;
    seed.extend_from_slice(&TT_ORD_LIST);
    let key_iv = core.calculate(&seed);
    let key_arr: [u8; 16] = key_iv[..16].try_into().unwrap();
    let iv_arr: [u8; 16] = *iv;

    // payload = 魔改哈希第 16..64 字节（48B）‖ gzip，再 PKCS7 填充。
    // calculate 输出 64 字节，真机抓包实证完整性哈希取的是 **digest[16..64]**
    // 而非前 48 字节（register_req_real.bin 对拍锁定）——取错窗口服务端
    // 重算不匹配即静默拒绝（device_id=0）。
    let mut payload = core.calculate(gz)[16..64].to_vec();
    payload.extend_from_slice(gz);
    let pad = 16 - payload.len() % 16;
    payload.extend(std::iter::repeat(pad as u8).take(pad));

    // 标准 AES-128-CBC（与上游 Python 参考实现等价，由
    // py_enc_accepted.hex 逐字节复现测试锁定——样本为服务端发号成功的
    // Python 加密 body，见 tests/fixtures/README.md）
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

    let mut result = Vec::with_capacity(6 + 32 + 16 + out.len());
    result.extend_from_slice(&TT_MAGIC);
    result.extend_from_slice(salt);
    result.extend_from_slice(&iv_arr);
    result.extend_from_slice(&out);
    result
}

/// TT-Encrypt V5 解密（对拍/取证用）：返回 (明文字节, 抓包里的 gz 字节)。
///
/// 解密链与加密完全对称：按 salt → 派生 → gz 的顺序重放三次 calculate
/// （与加密同一调用形态），并用 payload 前 48 字节校验完整性哈希。
pub fn tt_decrypt_v5(body: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    use aes::Aes128;
    use aes::cipher::{BlockCipherDecrypt, KeyInit};
    use flate2::read::GzDecoder;
    use std::io::Read;

    use hongguo_signer::tt_hash::{TT_ORD_LIST, TtHashCore};

    if body.len() < 6 + 32 + 16 + 16 || body[..6] != TT_MAGIC {
        return Err(format!(
            "不是 TT-Encrypt V5 包：len={} magic={:02x?}",
            body.len(),
            &body[..6.min(body.len())]
        ));
    }
    let salt = &body[6..38];
    let iv_arr: [u8; 16] = body[38..54].try_into().unwrap();
    let ct = &body[54..];
    if !ct.len().is_multiple_of(16) {
        return Err(format!("密文长度 {} 不是 16 的倍数", ct.len()));
    }

    // 派生链重放（顺序必须与加密一致，CF 链跨调用）
    let mut core = TtHashCore::new();
    let h1 = core.calculate(salt);
    let mut seed = h1;
    seed.extend_from_slice(&TT_ORD_LIST);
    let key_iv = core.calculate(&seed);
    let key_arr: [u8; 16] = key_iv[..16].try_into().unwrap();

    let cipher = Aes128::new((&key_arr).into());
    let mut payload = Vec::with_capacity(ct.len());
    let mut prev = iv_arr;
    for chunk in ct.chunks(16) {
        let mut blk: [u8; 16] = chunk.try_into().unwrap();
        cipher.decrypt_block((&mut blk).into());
        for (b, p) in blk.iter_mut().zip(&prev) {
            *b ^= p;
        }
        payload.extend_from_slice(&blk);
        prev.copy_from_slice(chunk);
    }

    // PKCS7 剥离
    let pad = *payload.last().ok_or("空密文")? as usize;
    if !(1..=16).contains(&pad)
        || payload.len() < pad
        || payload[payload.len() - pad..]
            .iter()
            .any(|&b| b as usize != pad)
    {
        return Err(format!("PKCS7 填充非法：pad={pad}"));
    }
    payload.truncate(payload.len() - pad);

    if payload.len() < 48 {
        return Err(format!("payload 过短：{}", payload.len()));
    }
    let (app_hash, gz) = payload.split_at(48);

    // 完整性哈希校验（CF 链上的第三次 calculate，取 digest[16..64]）
    let calc_hash = &core.calculate(gz)[16..64];
    if calc_hash != app_hash {
        return Err(format!(
            "完整性哈希不符：app={} calc={}",
            app_hash
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            calc_hash
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
        ));
    }

    let mut plain = Vec::new();
    GzDecoder::new(gz)
        .read_to_end(&mut plain)
        .map_err(|e| format!("gunzip 失败：{e}"))?;
    Ok((plain, gz.to_vec()))
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
    if device_id.is_empty() {
        return Err(AppError::Media("注册响应缺少 device_id".into()));
    }
    Ok(RegisterResult {
        device_id,
        install_id,
        device_token: get_str("device_token"),
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

/// 注册 query 的 40 个公共参数（指纹字段；`cdid`/`openudid`/时间戳每次生成，
/// 服务端按这套指纹发新设备号）。
fn register_query(cdid: &str, openudid: &str, now_ms: u64) -> Vec<(String, String)> {
    let pairs: Vec<(&str, String)> = vec![
        ("_rticket", now_ms.to_string()),
        ("ac", "wifi".into()),
        ("aid", "8662".into()),
        ("appTheme", "light".into()),
        ("app_name", "novelread".into()),
        ("app_type", "normal".into()),
        ("cdid", cdid.into()),
        ("channel", "xiaomi_8662_64".into()),
        ("cpu_support64", "true".into()),
        ("cronet_version", "8d40f833_2026-03-03".into()),
        ("device_brand", "xiaomi".into()),
        ("device_platform", "android".into()),
        ("device_type", "23127PN0CC".into()),
        ("dpi", "460".into()),
        ("first_launch_timestamp", (now_ms / 1000).to_string()),
        ("host_abi", "arm64-v8a".into()),
        ("is_android_fold", "0".into()),
        ("is_android_pad", "0".into()),
        ("is_guest_mode", "0".into()),
        ("is_preinstall", "0".into()),
        ("language", "zh".into()),
        ("last_deeplink_update_version_code", "0".into()),
        (
            "manifest_version_code",
            hongguo_signer::APP_VERSION_CODE_STR.into(),
        ),
        ("md", "0".into()),
        ("minor_status", "0".into()),
        ("need_personal_recommend", "1".into()),
        ("openudid", openudid.into()),
        ("os", "android".into()),
        ("os_api", "34".into()),
        ("os_version", "14".into()),
        ("package", "com.phoenix.read".into()),
        ("resolution", "1200*2670".into()),
        ("ssmix", "a".into()),
        ("ts", (now_ms / 1000).to_string()),
        ("tt_data", "a".into()),
        ("ttnet_version", "4.2.243.31-douyin".into()),
        (
            "update_version_code",
            hongguo_signer::APP_VERSION_CODE_STR.into(),
        ),
        ("use_store_region_cookie", "1".into()),
        ("version_code", hongguo_signer::APP_VERSION_CODE_STR.into()),
        ("version_name", hongguo_signer::APP_VERSION_NAME.into()),
    ];
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// 新设备指纹：cdid / openudid / clientudid / req_id 一次生成。
struct FreshIdentity {
    cdid: String,
    openudid: String,
    clientudid: String,
    req_id: String,
}

fn fresh_identity() -> FreshIdentity {
    fn uuid_v4() -> String {
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
    FreshIdentity {
        cdid: uuid_v4(),
        openudid: {
            let b: [u8; 8] = rand::random();
            b.iter().map(|x| format!("{x:02x}")).collect()
        },
        clientudid: uuid_v4(),
        req_id: uuid_v4(),
    }
}

/// 首次安装时间戳相对当前时间的回拨量（毫秒）：抓包模板值，设备「刚
/// 装不久」的拟真参数（apk_first_install_time 与 first_launch_timestamp 共用）。
const FIRST_INSTALL_BACKDATE_MS: u64 = 12_000;

/// 构造 AppLog 注册 JSON（官方 7.4.2.32 形态：字段集按 2026-10-26 官方
/// app_log 密文解包模板逐字段重铸，设备身份与机型段替换为本侧值；
/// `ab_sdk_version` 是服务端下发的 AB 名单，真首装没有，不带）。
fn applog_register_json(id: &FreshIdentity, now_ms: u64) -> Value {
    // 字段多，json! 宏会顶爆递归限制，header 用 Map 逐项构建
    let mut h = serde_json::Map::new();
    h.insert("access".into(), json!("wifi"));
    h.insert("aid".into(), json!(8662));
    h.insert(
        "apk_first_install_time".into(),
        json!(now_ms - FIRST_INSTALL_BACKDATE_MS),
    );
    h.insert(
        "app_version".into(),
        json!(hongguo_signer::APP_VERSION_NAME),
    );
    h.insert("app_version_minor".into(), json!(""));
    h.insert("cdid".into(), json!(id.cdid));
    h.insert("channel".into(), json!("xiaomi_8662_64"));
    h.insert("clientudid".into(), json!(id.clientudid));
    h.insert("cpu_abi".into(), json!("arm64-v8a"));
    h.insert(
        "custom".into(),
        json!({
            "account_region": "cn-gd",
            "account_region_src": "did",
            "dragon_device_type": "phone",
            "host_bit": 64,
            "is_android_pad_screen": "0",
            "mute_align_zip": false,
            "mute_enable": false,
            "mute_err_code": 1002,
            "mute_has_oat": false,
            "mute_host_version": hongguo_signer::APP_VERSION_CODE,
            "mute_patch_version": 10000000,
            "mute_rule_id": 0,
            "web_ua": format!(
                "Dalvik/2.1.0 (Linux; U; Android 14; Xiaomi 14 Build/UKQ1.230804.001) Phoenix/{}",
                hongguo_signer::APP_VERSION_NAME
            ),
        }),
    );
    h.insert("density_dpi".into(), json!(460));
    h.insert("device_brand".into(), json!("Xiaomi"));
    h.insert("device_manufacturer".into(), json!("Xiaomi"));
    h.insert("device_model".into(), json!("23127PN0CC"));
    h.insert("display_density".into(), json!("xxhdpi"));
    h.insert("display_density_v2".into(), json!("xxhdpi"));
    h.insert("display_name".into(), json!("红果免费短剧"));
    h.insert("ipv6_list".into(), json!([]));
    h.insert("language".into(), json!("zh"));
    h.insert(
        "manifest_version_code".into(),
        json!(hongguo_signer::APP_VERSION_CODE),
    );
    h.insert("not_request_sender".into(), json!(1));
    h.insert("oaid_may_support".into(), json!(false));
    h.insert("openudid".into(), json!(id.openudid));
    h.insert("os".into(), json!("Android"));
    h.insert("os_api".into(), json!(34));
    h.insert("os_version".into(), json!("14"));
    h.insert("package".into(), json!("com.phoenix.read"));
    h.insert("platform".into(), json!("Android"));
    h.insert("region".into(), json!("CN"));
    h.insert("release_build".into(), json!("a0af56b"));
    h.insert("resolution".into(), json!("1200*2670"));
    h.insert("resolution_v2".into(), json!("2670x1200"));
    h.insert("rom".into(), json!("hyperos"));
    h.insert("rom_version".into(), json!("OS1.0.32.0.UNCCNXM"));
    h.insert("sdk_version".into(), json!(507150490));
    h.insert("sdk_version_code".into(), json!(517150089));
    h.insert("sdk_version_name".into(), json!("5.7.15-bugfix.4"));
    // 官方 7.4.2.32 APK 的签名指纹（2026-10-26 官方实包解包，随包更新）
    h.insert("sig_hash".into(), json!("56a962410c494bbaf0b58dba20cae56f"));
    h.insert("sim_region".into(), json!("cn"));
    h.insert("carrier".into(), json!("CHINA MOBILE"));
    h.insert("mcc_mnc".into(), json!("46000"));
    h.insert("sim_serial_number".into(), json!([]));
    h.insert("tweaked_channel".into(), json!("xiaomi_8662_64"));
    h.insert("timezone".into(), json!(8));
    h.insert("tz_name".into(), json!("Asia/Shanghai"));
    h.insert("tz_offset".into(), json!(28800));
    h.insert(
        "update_version_code".into(),
        json!(hongguo_signer::APP_VERSION_CODE),
    );
    h.insert(
        "version_code".into(),
        json!(hongguo_signer::APP_VERSION_CODE),
    );

    json!({
        "magic_tag": "ss_app_log",
        "header": Value::Object(h),
        "_gen_time": now_ms,
    })
}

/// 注册一台全新设备。
///
/// 签名档案必须用**空 device_id/iid**（服务端按 query+body 指纹发新号；
/// 带着旧号签会被当成「已注册设备」原样回执），cdid/openudid 与
/// query/body 三处保持同一指纹。
pub async fn register_device(env: &ApiEnv) -> AppResult<RegisterResult> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let id = fresh_identity();
    // 7.4.2.32 形态（bdinstall.a1/d 逆向 + 官方实包 2026-10-26）：
    // query 只剩 `req_id`，body = gzip(注册 JSON)（SDK 默认 z=true 分支，
    // 不再走 TT-Encrypt V5），Content-Type 仍是 tt-data=a
    let q = vec![
        ("req_id".to_string(), id.req_id.clone()),
        ("tt_data".to_string(), "a".to_string()),
    ];

    let plain = applog_register_json(&id, now_ms).to_string();
    let body = gzip_java_flavor(plain.as_bytes());
    // debug dump（定位指纹/加密差异用，TT_DEBUG=1 时开启）
    if std::env::var("TT_DEBUG").is_ok() {
        let dir = super::capture_dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("rust_reg_plain.json"), &plain);
        let hex: String = body.iter().map(|b| format!("{b:02x}")).collect();
        let _ = std::fs::write(dir.join("rust_reg_body.hex"), &hex);
        let meta = serde_json::json!({"cdid": id.cdid, "openudid": id.openudid, "clientudid": id.clientudid, "req_id": id.req_id, "_rticket": now_ms.to_string(), "ts": (now_ms / 1000).to_string(), "first_launch_timestamp": ((now_ms - FIRST_INSTALL_BACKDATE_MS) / 1000).to_string()});
        let _ = std::fs::write(
            dir.join("rust_reg_meta.json"),
            serde_json::to_string(&meta).unwrap(),
        );
    }

    // 签名档案用调用方的静态档案（含已知合法的 device_id）：实测服务端
    // 要求注册请求带有「已激活设备上下文」——用全新/无 device_id 的档案
    // 签名会被静默拒绝（device_id=0）；静态设备上下文 + body 新指纹 =
    // 服务端按 body 指纹发放新设备号。
    let reg_env = ApiEnv {
        proxy: env.proxy.clone(),
        device: env.device.clone(),
        cookie: None,
        x_tt_token: None,
    };

    // Set-Cookie 捕获版：响应头会下发本设备自己的 ttreq 票（按新
    // install_id 发放），要随档案长期携带——静态兜底票绑定的是静态 iid，
    // 配新 iid 会出现「install_id 与票不同源」的 Cookie
    let response = api_call_full_response(
        REGISTER_ORIGIN,
        REGISTER_PATH,
        Some(body),
        &q,
        &[(
            "Content-Type".into(),
            "application/octet-stream;tt-data=a".into(),
        )],
        &reg_env,
    )
    .await?;
    let mut result = parse_register(&response.bytes)?;
    result.cdid = id.cdid.clone();
    result.openudid = id.openudid.clone();
    result.ttreq = response
        .set_cookies
        .iter()
        .find_map(|c| {
            let head = c.split(';').next()?.trim();
            head.strip_prefix("ttreq=").map(|s| s.to_string())
        })
        .unwrap_or_default();
    if result.device_id == "0" {
        let text = String::from_utf8_lossy(&response.bytes);
        return Err(AppError::Media(format!(
            "注册被服务端静默拒绝（device_id=0），加密或指纹形态可能过期；原始响应: {}",
            &text[..text.len().min(400)]
        )));
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;

    /// 临时考察：解密官方 7.4.2.32 真注册（同意隐私后触发，2026-10-26
    /// virgin 实抓）的 device_register 密文体，打印注册 JSON 模板。
    #[test]
    fn tmp_decrypt_register_74232_capture() {
        let body = include_bytes!("../../tests/fixtures/device_register_74232_real.bin");
        let (plain, _gz) = tt_decrypt_v5(body).expect("官方 device_register 应能解密");
        println!("===== register plain =====");
        println!("{}", String::from_utf8_lossy(&plain));
    }

    /// 临时考察：解密官方 7.4.2.32 清数据冷启动的第一条 app_log 密文体，
    /// 打印明文 JSON 供指纹模板对齐（看完即删或转正式锁样本）。
    #[test]
    fn tmp_decrypt_applog_74232_capture() {
        let body = include_bytes!("../../tests/fixtures/applog_register_74232_real.bin");
        let (plain, _gz) = tt_decrypt_v5(body).expect("官方 app_log 应能解密");
        println!("===== app_log plain =====");
        println!("{}", String::from_utf8_lossy(&plain));
    }

    /// 真实 App 抓包（register_req_real.bin，2026-10-03 真机注册 body）的
    /// 离线闭环：解密 → 同盐重加密 → 必须逐字节复现原始 body。
    /// 锁死「派生链 + AES-CBC + PKCS7 + gzip 字节流」与官方客户端一致。
    #[test]
    fn tt_v5_real_capture_roundtrip() {
        let body = include_bytes!("../../tests/fixtures/register_req_real.bin");
        let (plain, app_gz) = tt_decrypt_v5(body).expect("真实抓包应能解密");

        // 解出的明文必须是合法 AppLog 注册 JSON
        let v: serde_json::Value = serde_json::from_slice(&plain).expect("解出的明文应是 JSON");
        assert_eq!(v["magic_tag"], "ss_app_log", "magic_tag 不符");
        assert_eq!(&app_gz[..2], &[0x1f, 0x8b], "payload 48 偏移处应是 gzip 流");

        let salt: [u8; 32] = body[6..38].try_into().unwrap();
        let re = tt_encrypt_v5_with_salt(&plain, &salt);

        // 结构锁：同盐重加密必须复现 magic+salt 区与总长（IV 是随机的、
        // gzip 字节流因压缩器而异，此两者服务端均不校验——服务端用
        // header 里的 IV 解密并按 digest[16..64] 校验完整性哈希）
        assert_eq!(&re[..38], &body[..38], "magic/salt 区不一致");
        assert_eq!(
            re.len(),
            body.len(),
            "总长不一致（PKCS7 吸收 gz 差异后应相等）"
        );

        // 往返锁：重加密密文再次解密必须还原同一明文（加密自洽）
        let (plain2, _) = tt_decrypt_v5(&re).expect("重加密密文应能自解");
        assert_eq!(plain2, plain, "加解密往返不还原");
    }

    /// 「被服务端接受的注册 body」字节级复现锁：解密样本拿到 (gz, salt,
    /// iv) 后重加密，必须逐字节相等。锁死 AES-CBC/PKCS7/hash 窗口/拼装
    /// 全链路与真实接受形态一致（样本为 2026-10-03 服务端发号成功的
    /// TT-Encrypt V5 body，解密需通过 digest[16..64] 校验）。
    #[test]
    fn tt_v5_reproduces_accepted_body() {
        let hex = include_str!("../../tests/fixtures/py_enc_accepted.hex");
        let hex = hex.trim();
        let body: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let (plain, gz) = tt_decrypt_v5(&body).expect("被接受样本必须能解密（hash 窗口锁）");
        let v: serde_json::Value = serde_json::from_slice(&plain).expect("被接受样本明文应是 JSON");
        assert_eq!(v["magic_tag"], "ss_app_log");

        let salt: [u8; 32] = body[6..38].try_into().unwrap();
        let iv: [u8; 16] = body[38..54].try_into().unwrap();
        let re = tt_encrypt_v5_full(&gz, &salt, &iv);
        assert_eq!(re, body, "同 (gz,salt,iv) 重加密必须逐字节复现被接受 body");
    }

    /// 7.4.2.32 形态注册变体：body header 预填本地 iid/device_id（官方
    /// guarantee 形态，bdinstall a1.k 的 has-ids 分支），签名档案与 body 同源。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_register_74232_localid() {
        use hongguo_common::model::ProxyConfig;

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let iid = 4_000_000_000_000_000u64 + rand::random::<u64>() % 999_999_999_999_999;
        let did = 4_000_000_000_000_000u64 + rand::random::<u64>() % 999_999_999_999_999;
        let id = fresh_identity();
        let mut profile =
            hongguo_signer::device::DeviceProfile::fresh_register_profile(&id.cdid, &id.openudid);
        profile.set("iid", &iid.to_string());
        profile.set("device_id", &did.to_string());
        hongguo_signer::device::align_app_version(&mut profile);

        let mut v = applog_register_json(&id, now_ms);
        v["header"]["install_id"] = json!(iid.to_string());
        v["header"]["device_id"] = json!(did.to_string());
        let plain = v.to_string();
        let body = gzip_java_flavor(plain.as_bytes());
        let q = vec![
            ("req_id".to_string(), id.req_id.clone()),
            ("tt_data".to_string(), "a".to_string()),
        ];
        let env = ApiEnv {
            proxy: ProxyConfig::default(),
            device: profile,
            cookie: None,
            x_tt_token: None,
        };
        match api_call_full_response(
            REGISTER_ORIGIN,
            REGISTER_PATH,
            Some(body),
            &q,
            &[(
                "Content-Type".into(),
                "application/octet-stream;tt-data=a".into(),
            )],
            &env,
        )
        .await
        {
            Ok(r) => println!(
                "[reg-localid] bytes={} set_cookies={:?} body={}",
                r.bytes.len(),
                r.set_cookies,
                String::from_utf8_lossy(&r.bytes)
            ),
            Err(e) => println!("[reg-localid] ERR {e}"),
        }
    }

    /// 官方新形态对照实验（2026-10-26 官方 virgin 抓包三点一线）：全新身份
    /// 的官方 App 查询不带 iid/device_id/cdid、零 cookie，reading 直接 200。
    /// 我方同形态：无身份档案（仅机型指纹+版本）+ 无 cookie 打搜索探针。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_anonymous_read() {
        use hongguo_common::model::ProxyConfig;

        let id = fresh_identity();
        let mut profile =
            hongguo_signer::device::DeviceProfile::fresh_register_profile(&id.cdid, &id.openudid);
        // 官方 virgin 形态二分：query 带 cdid（客户端自生成 UUID），无
        // iid/device_id/openudid，零 cookie
        profile.remove("openudid");
        hongguo_signer::device::align_app_version(&mut profile);
        let env = ApiEnv {
            // 走本机 mitm 便于逐字段对照官方成功请求（排查匿名被拒用）
            proxy: ProxyConfig {
                mode: hongguo_common::model::settings::ProxyMode::Manual,
                url: "http://127.0.0.1:8080".into(),
            },
            device: profile,
            cookie: None,
            x_tt_token: None,
        };
        match crate::recommend::fetch_feed_tabs(&env).await {
            Ok(tabs) => println!(
                "[anon-read] feed tabs: {:?} → 匿名读链路成立",
                tabs.iter().map(|t| t.tab_type.clone()).collect::<Vec<_>>()
            ),
            Err(e) => println!("[anon-read] feed 失败: {e}"),
        }
    }

    /// 官方 7.4.2.32 真注册形态复刻（2026-10-26 virgin 实抓三点一线）：
    /// origin=log5-applog.fqnovel.com、query 四参、无签名头、body=TC 加密
    /// 的官方模板 JSON。成功判据：响应 device_id 非 0 + Set-Cookie ttreq。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_register_exact_74232() {
        let id = fresh_identity();
        // 对照变体：换用官方已知指纹（virgin 实抓的 cdid/openudid/clientudid）
        // —— 服务端对该指纹应回发原 ids（new_user=0）
        let id = FreshIdentity {
            cdid: "945a6bc1-5862-438a-90ec-865ae3e18de5".into(),
            openudid: "906e2492bd53b61d".into(),
            clientudid: "561e83c2-56f1-48d6-bd2e-edcf5c64a9cd".into(),
            req_id: id.req_id,
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let custom = serde_json::json!({
            "account_region_src": "fallback",
            "mute_host_version": hongguo_signer::APP_VERSION_CODE,
            "mute_err_code": 1002,
            "mute_align_zip": false,
            "mute_rule_id": 0,
            "mute_enable": false,
            "mute_patch_version": 10000000,
            "mute_has_oat": false,
            "host_bit": 64,
            "account_region": "cn",
            "dragon_device_type": "phone",
        });
        // 字段多，json! 宏会顶爆递归限制，header 用 Map 逐项构建
        let pairs: &[(&str, Value)] = &[
            ("os", json!("Android")),
            ("os_version", json!("14")),
            ("os_api", json!(34)),
            ("device_model", json!("23127PN0CC")),
            ("device_brand", json!("Xiaomi")),
            ("device_manufacturer", json!("Xiaomi")),
            ("cpu_abi", json!("arm64-v8a")),
            ("density_dpi", json!(460)),
            ("display_density", json!("xxhdpi")),
            ("resolution", json!("1200*2670")),
            ("display_density_v2", json!("xxhdpi")),
            ("resolution_v2", json!("2670x1200")),
            ("access", json!("wifi")),
            ("rom", json!("hyperos")),
            ("rom_version", json!("OS1.0.32.0.UNCCNXM")),
            ("language", json!("zh")),
            ("timezone", json!(8)),
            ("region", json!("CN")),
            ("tz_name", json!("Asia/Shanghai")),
            ("tz_offset", json!(28800)),
            ("sim_region", json!("cn")),
            ("carrier", json!("CHINA MOBILE")),
            ("mcc_mnc", json!("46000")),
            ("clientudid", json!(id.clientudid)),
            ("openudid", json!(id.openudid)),
            ("channel", json!("xiaomi_8662_64")),
            ("not_request_sender", json!(1)),
            ("aid", json!(8662)),
            ("release_build", json!("a0af56b")),
            ("custom", custom),
            ("package", json!("com.phoenix.read")),
            ("app_version", json!(hongguo_signer::APP_VERSION_NAME)),
            ("app_version_minor", json!(hongguo_signer::APP_VERSION_NAME)),
            ("version_code", json!(hongguo_signer::APP_VERSION_CODE)),
            (
                "update_version_code",
                json!(hongguo_signer::APP_VERSION_CODE),
            ),
            (
                "manifest_version_code",
                json!(hongguo_signer::APP_VERSION_CODE),
            ),
            ("app_name", json!("novelread")),
            ("tweaked_channel", json!("xiaomi_8662_64")),
            ("display_name", json!("红果免费短剧")),
            ("sig_hash", json!("56a962410c494bbaf0b58dba20cae56f")),
            ("cdid", json!(id.cdid)),
            ("ipv6_list", json!([])),
            ("sim_serial_number", json!([])),
            ("oaid_may_support", json!(false)),
            ("device_platform", json!("android")),
            ("git_hash", json!("27660d0")),
            ("sdk_version_code", json!(4030294)),
            ("sdk_target_version", json!(30)),
            ("req_id", json!(id.req_id)),
            ("sdk_version", json!("4.3.2-rc.44")),
            ("guest_mode", json!(0)),
            ("sdk_flavor", json!("cnInner")),
            ("pre_installed_channel", json!("")),
            (
                "apk_first_install_time",
                json!(now_ms - FIRST_INSTALL_BACKDATE_MS),
            ),
            ("is_system_app", json!(0)),
        ];
        let mut h = serde_json::Map::new();
        for (k, v) in pairs {
            h.insert((*k).to_string(), v.clone());
        }
        let mut outer = serde_json::Map::new();
        outer.insert("header".into(), Value::Object(h));
        outer.insert("magic_tag".into(), json!("ss_app_log"));
        outer.insert("_gen_time".into(), json!(now_ms));
        let plain = Value::Object(outer).to_string();
        let body = tt_encrypt_v5(plain.as_bytes());
        // 暖场序列（官方冷启动次序：settings → app_log → register），
        // 让风控先见到 cdid，再打注册
        let warm_client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:8080").expect("代理地址无效"))
            .danger_accept_invalid_certs(true)
            .build()
            .expect("client 构造失败");
        let settings_url = format!(
            "https://is.snssdk.com/service/settings/v3/?aid=8662&app_name=novelread&channel=xiaomi_8662_64&cdid={}&device_platform=android&update_version_code={}&version_code={}&version_name={}&manifest_version_code={}",
            id.cdid,
            hongguo_signer::APP_VERSION_CODE_STR,
            hongguo_signer::APP_VERSION_CODE_STR,
            hongguo_signer::APP_VERSION_NAME,
            hongguo_signer::APP_VERSION_CODE_STR
        );
        let warm1 = warm_client
            .get(&settings_url)
            .header("user-agent", hongguo_signer::video_ua())
            .header("sdk-version", "2")
            .send()
            .await;
        println!("[reg-exact] 暖场 settings: {:?}", warm1.map(|r| r.status()));
        let applog_body = tt_encrypt_v5(
            serde_json::json!({
                "header": {
                    "aid": 8662, "app_name": "novelread", "channel": "xiaomi_8662_64",
                    "cdid": id.cdid, "clientudid": id.clientudid, "openudid": id.openudid,
                    "device_id": "", "install_id": "",
                    "version_code": hongguo_signer::APP_VERSION_CODE,
                    "update_version_code": hongguo_signer::APP_VERSION_CODE,
                    "manifest_version_code": hongguo_signer::APP_VERSION_CODE,
                    "app_version": hongguo_signer::APP_VERSION_NAME,
                    "os": "Android", "os_version": "14", "os_api": 34,
                    "device_model": "23127PN0CC", "device_brand": "Xiaomi",
                    "device_manufacturer": "Xiaomi", "device_platform": "android",
                    "language": "zh", "region": "CN", "timezone": 8,
                },
                "magic_tag": "ss_app_log",
                "_gen_time": now_ms,
            })
            .to_string()
            .as_bytes(),
        );
        let warm2 = warm_client
            .post("https://log5-applog.fqnovel.com/service/2/app_log/?aid=8662&device_platform=android&device_id=&iid=&version_code=74232")
            .header("content-type", "application/octet-stream;tt-data=a")
            .header("user-agent", hongguo_signer::video_ua())
            .body(applog_body)
            .send()
            .await;
        println!("[reg-exact] 暖场 app_log: {:?}", warm2.map(|r| r.status()));
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let url = format!(
            "https://log5-applog.fqnovel.com/service/2/device_register/?req_id={}&tt_data=a&okhttp_version=4.2.243.31-douyin&use_store_region_cookie=1",
            id.req_id
        );
        // 官方注册请求无任何签名头；x-ss-stub = body MD5 大写
        let stub = hongguo_signer::md5_hex_upper(&body);
        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:8080").expect("代理地址无效"))
            .danger_accept_invalid_certs(true)
            .build()
            .expect("client 构造失败");
        let resp = client
            .post(&url)
            .header("x-ss-stub", stub)
            .header("x-ss-req-ticket", now_ms.to_string())
            .header("x-vc-bdturing-sdk-version", "4.0.3.cn")
            .header("sdk-version", "2")
            .header("passport-sdk-version", "5051452")
            .header("user-agent", hongguo_signer::video_ua())
            .header("content-type", "application/octet-stream;tt-data=a")
            .header("accept-encoding", "gzip")
            .header(
                "cookie",
                "passport_csrf_token=015f1641adca7c82c7a5225d0cf8de40; passport_csrf_token_default=015f1641adca7c82c7a5225d0cf8de40",
            )
            .body(body)
            .send()
            .await
            .expect("register 请求失败");
        let set_cookies: Vec<String> = resp
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok().map(str::to_string))
            .collect();
        let status = resp.status();
        let raw = resp.bytes().await.unwrap();
        let plain_resp = String::from_utf8_lossy(&raw).to_string();
        println!("[reg-exact] status={status} set_cookies={set_cookies:?} resp={plain_resp}");
    }

    /// 官方 7.4.2.32 的身份认领通道：POST /service/2/app_log/（TC 加密的
    /// ss_app_log 载荷，header 预填本地 iid/device_id），看响应 Set-Cookie
    /// 是否发放 ttreq/odin_tt——这是官方游客身份的「注册」。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_applog_claim() {
        use hongguo_common::model::ProxyConfig;

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let iid = 4_000_000_000_000_000u64 + rand::random::<u64>() % 999_999_999_999_999;
        let did = 4_000_000_000_000_000u64 + rand::random::<u64>() % 999_999_999_999_999;
        let id = fresh_identity();
        let mut profile =
            hongguo_signer::device::DeviceProfile::fresh_register_profile(&id.cdid, &id.openudid);
        profile.set("iid", &iid.to_string());
        profile.set("device_id", &did.to_string());
        hongguo_signer::device::align_app_version(&mut profile);

        let mut v = applog_register_json(&id, now_ms);
        v["header"]["install_id"] = json!(iid.to_string());
        v["header"]["device_id"] = json!(did.to_string());
        // app_log 是事件端点：官方载荷带 launch/event_v3/options，缺了报
        // "error data"。补最小事件集（结构照 2026-10-26 官方密文解包）。
        let sb: [u8; 16] = rand::random();
        let session = format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}#1",
            sb[0],
            sb[1],
            sb[2],
            sb[3],
            sb[4],
            sb[5],
            sb[6] & 0x0f | 0x40,
            sb[7],
            sb[8] & 0x3f | 0x80,
            sb[9],
            sb[10],
            sb[11],
            sb[12],
            sb[13],
            sb[14],
            sb[15]
        );
        let datetime = chrono_local(now_ms);
        v["launch"] = json!([{
            "local_time_ms": now_ms,
            "tea_event_index": 10001,
            "session_id": format!("{session}#1"),
            "datetime": datetime,
        }]);
        v["event_v3"] = json!([]);
        v["options"] = json!({
            "request_from": 1,
            "applog_priority": {"enable": 1, "value": -1},
        });
        let body = tt_encrypt_v5(v.to_string().as_bytes());

        let q = vec![
            (
                "version_code".to_string(),
                hongguo_signer::APP_VERSION_CODE_STR.to_string(),
            ),
            ("device_platform".to_string(), "android".to_string()),
            ("device_id".to_string(), did.to_string()),
            ("aid".to_string(), "8662".to_string()),
            ("iid".to_string(), iid.to_string()),
        ];
        let env = ApiEnv {
            proxy: ProxyConfig::default(),
            device: profile,
            cookie: None,
            x_tt_token: None,
        };
        match api_call_full_response(
            "https://log5-applog-lf.fqnovel.com",
            "/service/2/app_log/",
            Some(body),
            &q,
            &[(
                "Content-Type".into(),
                "application/octet-stream;tt-data=a".into(),
            )],
            &env,
        )
        .await
        {
            Ok(r) => println!(
                "[applog-claim] bytes={} set_cookies={:?} body={}",
                r.bytes.len(),
                r.set_cookies,
                String::from_utf8_lossy(&r.bytes)
            ),
            Err(e) => println!("[applog-claim] ERR {e}"),
        }
    }

    /// app_log 事件里的本地时间串（"2026-10-26 20:33:34" 形态，UTC+8）。
    fn chrono_local(now_ms: u64) -> String {
        let secs = (now_ms / 1000) as i64 + 8 * 3600;
        let days = secs / 86400;
        let rem = secs % 86400;
        let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
        // civil-from-days（Howard Hinnant 算法）
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let mo = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if mo <= 2 { y + 1 } else { y };
        format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}")
    }

    /// 本地身份全链路试跑（官方 7.4.2.32 实测形态，2026-10-26 抓包三点一线）：
    /// 本地生成 iid/device_id → cookie 只带 install_id（官方首请求同款）
    /// → settings 端点换票（服务端 Set-Cookie 发 ttreq/odin_tt）
    /// → reading 搜索探针验活性。官方 App 全程不调 device_register。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_local_device_identity() {
        use crate::search::search_series;
        use hongguo_common::model::ProxyConfig;

        // 本地生成 16 位十进制身份（官方本地生成形态）
        let iid = 4_000_000_000_000_000u64 + rand::random::<u64>() % 999_999_999_999_999;
        let did = 4_000_000_000_000_000u64 + rand::random::<u64>() % 999_999_999_999_999;
        let ub: [u8; 16] = rand::random();
        let cdid = format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-4{:02x}-a{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            ub[0],
            ub[1],
            ub[2],
            ub[3],
            ub[4],
            ub[5],
            ub[6] & 0x0f,
            ub[7] & 0x3f,
            ub[8],
            ub[9],
            ub[10],
            ub[11],
            ub[12],
            ub[13]
        );
        let ob: [u8; 8] = rand::random();
        let openudid: String = ob.iter().map(|x| format!("{x:02x}")).collect();
        let mut profile =
            hongguo_signer::device::DeviceProfile::fresh_register_profile(&cdid, &openudid);
        profile.set("iid", &iid.to_string());
        profile.set("device_id", &did.to_string());
        hongguo_signer::device::align_app_version(&mut profile);
        println!("[local-id] 本地身份 iid={iid} device_id={did}");

        let base_cookie = format!("store-region=cn-gd; store-region-src=did; install_id={iid}");
        let mut env = ApiEnv {
            proxy: ProxyConfig::default(),
            device: profile,
            cookie: Some(base_cookie.clone()),
            x_tt_token: None,
        };

        // 第 1 步：monitor settings 换票（官方首请求 1:1 复刻，2026-10-26 抓包：
        // 无签名头、无 cookie、query 只带 device_id；服务端 Set-Cookie 发 ttreq/odin_tt）
        let url = format!(
            "https://mon.zijieapi.com/monitor/appmonitor/v3/settings?\
             update_version_code=4020196&os=Android&app_version=4.2.1&device_id={did}\
             &channel=release&sdk_version=4.2.1&aid=2010"
        );
        let plain_client = reqwest::Client::new();
        let resp = plain_client
            .get(&url)
            .header(
                "User-Agent",
                "Dalvik/2.1.0 (Linux; U; Android 14; Xiaomi 14 Build/UKQ1.230804.001)",
            )
            .header("Accept-Encoding", "gzip")
            .send()
            .await
            .expect("monitor settings 请求失败");
        let set_cookies: Vec<String> = resp
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok().map(str::to_string))
            .collect();
        println!(
            "[local-id] monitor settings status={} set_cookies={:?}",
            resp.status(),
            set_cookies
        );
        let mut issued = String::new();
        for sc in &set_cookies {
            if let Some(kv) = sc.split(';').next() {
                issued.push_str(kv);
                issued.push_str("; ");
            }
        }
        if !issued.is_empty() {
            env.cookie = Some(format!("{base_cookie}; {issued}"));
        }

        // 第 2 步：reading 搜索探针（服务端是否认领本地身份看这里）
        match search_series("热门", 0, "", &env).await {
            Ok(p) => println!(
                "[local-id] 搜索 {} 条 → 本地身份可用（官方形态成立）",
                p.items.len()
            ),
            Err(e) => println!("[local-id] 搜索失败: {e}"),
        }
    }

    /// 抓包样本形状：数字 + 字符串双形态并存。
    #[test]
    fn parses_register_response() {
        let raw = br#"{"server_time":1791016810,"device_id":2715158266032906,"install_id":2715158266282762,"new_user":1,"device_id_str":"2715158266032906","install_id_str":"2715158266282762","device_token":"AAAY"}"#;
        let r = parse_register(raw).expect("应能解析");
        assert_eq!(r.device_id, "2715158266032906");
        assert_eq!(r.install_id, "2715158266282762");
        assert_eq!(r.device_token, "AAAY");
        assert_eq!(r.server_time, 1791016810);
        assert!(r.new_user);
    }

    /// Python 逆向实现（TikTok 固件同源）固定盐加密的 golden：解密必须
    /// 还原出同一段明文——锁住「盐派生 + AES-CBC + PKCS7」与公开实现一致。
    #[test]
    fn error_code_is_surfaced() {
        let raw = br#"{"code":110,"message":"ILLEGAL_ACCESS"}"#;
        let err = parse_register(raw).unwrap_err();
        assert!(err.to_string().contains("110"));
    }
}

#[cfg(test)]
mod probe {
    use super::*;
    use crate::client::ApiEnv;
    use hongguo_common::model::ProxyConfig;
    use hongguo_common::utils::url::encode_rfc3986;

    fn anon_env() -> ApiEnv {
        ApiEnv::anonymous(ProxyConfig::default())
    }

    // register_query 在模块级（register_device 共用）。

    /// 试探 1：明文 form 表单体（老版本 App 形态，body = query 的 urlencode）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_register_plaintext_form() {
        let env = anon_env();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let cdid = uuid_v4();
        let openudid = hex_random();
        let mut q = register_query(&cdid, &openudid, now_ms);
        q.retain(|(k, _)| k != "tt_data"); // 声明明文形态，去掉加密标记

        // form 体 = query 全量 urlencode（经典注册形态）
        let body = q
            .iter()
            .map(|(k, v)| format!("{}={}", k, encode_rfc3986(v)))
            .collect::<Vec<_>>()
            .join("&");

        match api_call_full(
            REGISTER_ORIGIN,
            REGISTER_PATH,
            Some(body.into_bytes()),
            &q,
            &env,
        )
        .await
        {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                println!("[register-form] {}", &text[..text.len().min(600)]);
                match parse_register(&bytes) {
                    Ok(r) => println!(
                        "[register-form] device_id={} install_id={} new_user={}",
                        r.device_id, r.install_id, r.new_user
                    ),
                    Err(e) => println!("[register-form] 解析失败: {e}"),
                }
            }
            Err(e) => println!("[register-form] ERR {e}"),
        }
    }

    /// 试探 2：老式 form 补全指纹字段（cpu_model/density/timezone 等
    /// 老版本 App 注册时 body 里的经典字段，query 只带公共项）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_register_legacy_fingerprint_form() {
        let env = anon_env();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let cdid = uuid_v4();
        let openudid = hex_random();
        let mut q = register_query(&cdid, &openudid, now_ms);
        q.retain(|(k, _)| k != "tt_data");

        let mut pairs: Vec<(String, String)> = q.clone();
        pairs.extend([
            (
                "cpu_model".into(),
                "Qualcomm Technologies, Inc SM8650".into(),
            ),
            ("density".into(), "2.75".into()),
            ("language".into(), "zh".into()),
            ("mc".into(), "5c:c9:d3:12:34:56".into()),
            ("timezone".into(), "Asia/Shanghai".into()),
            ("req_id".into(), format!("{cdid}-{now_ms}")),
            ("rom_version".into(), "UKQ1.230804.001".into()),
            ("sim_region".into(), "cn".into()),
        ]);

        let body = pairs
            .iter()
            .map(|(k, v)| format!("{}={}", k, encode_rfc3986(v)))
            .collect::<Vec<_>>()
            .join("&");

        match api_call_full(
            REGISTER_ORIGIN,
            REGISTER_PATH,
            Some(body.into_bytes()),
            &q,
            &env,
        )
        .await
        {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                println!("[register-legacy] {}", &text[..text.len().min(600)]);
                match parse_register(&bytes) {
                    Ok(r) => println!(
                        "[register-legacy] device_id={} install_id={} new_user={}",
                        r.device_id, r.install_id, r.new_user
                    ),
                    Err(e) => println!("[register-legacy] 解析失败: {e}"),
                }
            }
            Err(e) => println!("[register-legacy] ERR {e}"),
        }
    }

    /// 试探 3：gzip 压缩的明文 form（抓包头里 content-encoding: gzip +
    /// log-encode-type: gzip，试探该形态是否与加密无关地被接受）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_register_gzip_form() {
        use std::io::Write as _;
        let env = anon_env();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let cdid = uuid_v4();
        let openudid = hex_random();
        let mut q = register_query(&cdid, &openudid, now_ms);
        q.retain(|(k, _)| k != "tt_data");

        let form = q
            .iter()
            .map(|(k, v)| format!("{}={}", k, encode_rfc3986(v)))
            .collect::<Vec<_>>()
            .join("&");
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(form.as_bytes()).unwrap();
        let body = gz.finish().unwrap();

        let extra = [
            ("content-encoding".to_string(), "gzip".to_string()),
            ("log-encode-type".to_string(), "gzip".to_string()),
            (
                "content-type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            ),
        ];
        match crate::client::api_call_full_with_headers(
            REGISTER_ORIGIN,
            REGISTER_PATH,
            Some(body),
            &q,
            &extra,
            &env,
        )
        .await
        {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                println!("[register-gzip] {}", &text[..text.len().min(600)]);
            }
            Err(e) => println!("[register-gzip] ERR {e}"),
        }
    }

    /// 试探 4：TT-Encrypt V5 加密 body（Python 逆向实现预生成，hex 文件
    /// 读入——用于在移植 Rust 前验证「签名 + 加密形态」整条链路）。
    #[tokio::test]
    #[ignore = "需要先跑 python 生成 register_body.hex"]
    async fn probe_register_ttencript_v5() {
        let hex = std::fs::read_to_string(crate::capture_dir().join("register_body.hex"))
            .expect("先跑 python 生成 register_body.hex");
        let hex = hex.trim();
        let body: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        println!(
            "[register-v5] body {} 字节, magic {:?}",
            body.len(),
            &body[..6]
        );

        // meta 里有与 body 配套的 cdid/openudid/时间戳（query 必须与 body 一致）
        let meta: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(crate::capture_dir().join("register_body_meta.json")).unwrap(),
        )
        .unwrap();
        let cdid = meta["cdid"].as_str().unwrap();
        let openudid = meta["openudid"].as_str().unwrap();
        let now_ms = meta["_rticket"].as_str().unwrap().parse::<u64>().unwrap();
        let mut q = register_query(cdid, openudid, now_ms);
        // 抓包形态：加密包在 body，query 保留 tt_data=a
        // （register_query 里本来就带 tt_data=a，不删）

        let env = anon_env();
        match api_call_full(REGISTER_ORIGIN, REGISTER_PATH, Some(body), &q, &env).await {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                println!("[register-v5] {}", &text[..text.len().min(600)]);
            }
            Err(e) => println!("[register-v5] ERR {e}"),
        }
    }

    /// 试探 5：TT-Encrypt V5 + AppLog JSON（真实注册形态——抓包 body
    /// 用 Python 逆向实现解密出的就是这份 JSON 结构）。
    #[tokio::test]
    #[ignore = "需要先跑 python 生成 register_body2.hex"]
    async fn probe_register_applog_json() {
        let hex = std::fs::read_to_string(crate::capture_dir().join("register_body2.hex"))
            .expect("先跑 python 生成 register_body2.hex");
        let body: Vec<u8> = (0..hex.trim().len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex.trim()[i..i + 2], 16).unwrap())
            .collect();

        let meta: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(crate::capture_dir().join("register_body2_meta.json"))
                .unwrap(),
        )
        .unwrap();
        let now_ms = meta["_rticket"].as_str().unwrap().parse::<u64>().unwrap();
        let q = register_query(
            meta["cdid"].as_str().unwrap(),
            meta["openudid"].as_str().unwrap(),
            now_ms,
        );

        // 注册签名档案：device_id/iid 必须为空（服务端要发新的），
        // cdid/openudid 与 query/body 保持同一指纹
        let mut dev = hongguo_signer::anonymous_profile(meta["cdid"].as_str().unwrap());
        dev.set("openudid", meta["openudid"].as_str().unwrap());
        let env = ApiEnv {
            proxy: anon_env().proxy,
            device: dev,
            cookie: None,
            x_tt_token: None,
        };
        match api_call_full(REGISTER_ORIGIN, REGISTER_PATH, Some(body), &q, &env).await {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                println!("[register-json] {}", &text[..text.len().min(600)]);
                match parse_register(&bytes) {
                    Ok(r) => println!(
                        "[register-json] device_id={} install_id={} new_user={}",
                        r.device_id, r.install_id, r.new_user
                    ),
                    Err(e) => println!("[register-json] 解析失败: {e}"),
                }
            }
            Err(e) => println!("[register-json] ERR {e}"),
        }
    }

    /// 纯 Rust 注册链路：AppLog JSON + tt_encrypt_v5 + 静态档案签名。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_register_device_rust() {
        let env = anon_env();
        match register_device(&env).await {
            Ok(r) => println!(
                "[register-rust] device_id={} install_id={} new_user={}",
                r.device_id, r.install_id, r.new_user
            ),
            Err(e) => println!("[register-rust] ERR {e}"),
        }
    }

    /// 档案矩阵：节流假设检验。每个变体全新指纹+全新 body，间隔 2s。
    /// P0=重放 py_fresh2（21:23 被接受的全新注册，验证端点与幂等重发）；
    /// P1=静态档案签名（现状，若 0/0 而其他过 → 档案被节流）；
    /// P2=fresh 档案（无 device_id/iid 字段，检验「需要已知设备」结论
    /// 是否被空值字段污染）；P3=71332 档案去掉 id 字段。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_register_profile_matrix() {
        let env0 = anon_env();

        // P0：重放 py_fresh2
        let dir = crate::capture_dir();
        if let (Ok(hex), Ok(meta_s)) = (
            std::fs::read_to_string(dir.join("py_fresh2.hex")),
            std::fs::read_to_string(dir.join("py_fresh2_meta.json")),
        ) {
            let hex = hex.trim();
            let body: Vec<u8> = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect();
            let meta: serde_json::Value = serde_json::from_str(&meta_s).unwrap();
            let q = register_query(
                meta["cdid"].as_str().unwrap(),
                meta["openudid"].as_str().unwrap(),
                meta["_rticket"].as_str().unwrap().parse().unwrap(),
            );
            match api_call_full(REGISTER_ORIGIN, REGISTER_PATH, Some(body), &q, &env0).await {
                Ok(b) => println!(
                    "[pm-P0重放] {}",
                    &String::from_utf8_lossy(&b)[..160.min(b.len())]
                ),
                Err(e) => println!("[pm-P0重放] ERR {e}"),
            }
        } else {
            println!("[pm-P0重放] 样本缺失，跳过");
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // 变体发送器：全新指纹 + 指定签名档案
        async fn send_fresh(tag: &str, env: &ApiEnv) {
            match register_device(env).await {
                Ok(r) => println!(
                    "[pm-{tag}] device_id={} new_user={}",
                    r.device_id, r.new_user
                ),
                Err(e) => println!("[pm-{tag}] ERR {e}"),
            }
        }

        // P1：静态档案（现状）
        send_fresh("P1静态档案", &env0).await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // P2：fresh 档案（74232，无 device_id/iid/cdid/openudid 字段）
        let mut fresh = hongguo_signer::device::DeviceProfile::fresh_register_profile("", "");
        fresh.remove("cdid");
        fresh.remove("openudid");
        let env2 = ApiEnv {
            proxy: env0.proxy.clone(),
            device: fresh,
            cookie: None,
            x_tt_token: None,
        };
        send_fresh("P2fresh档案", &env2).await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // P3：匿名档案去掉 device_id/iid（本就为空）
        let dev3 = hongguo_signer::anonymous_profile(&hongguo_signer::fresh_cdid());
        let env3 = ApiEnv {
            proxy: env0.proxy.clone(),
            device: dev3,
            cookie: None,
            x_tt_token: None,
        };
        send_fresh("P3匿名71332", &env3).await;
    }

    fn uuid_v4() -> String {
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

    fn hex_random() -> String {
        let b: [u8; 8] = rand::random();
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
}
