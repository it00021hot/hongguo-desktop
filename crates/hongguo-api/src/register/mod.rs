//! 设备注册：`POST log.snssdk.com/service/2/device_register/`。
//!
//! 注册是整个 App API 身份链的起点：服务端按请求里的 cdid/openudid
//! 指纹发放 `device_id` / `install_id`，之后所有业务请求的设备参数与
//! cookie（install_id/ttreq）都从这来。播放取流需要服务端「认识」的
//! 设备，匿名档案不可播。
//!
//! ## 线上格式（73932 活形态）
//!
//! 请求（`register_device`）：
//! - query：40 参公共指纹（[`register_query`]，版本身份覆写为 73932）
//! - body：TT-Encrypt V5 加密的注册 JSON（[`reg73932_json_str`]）
//! - 头：`content-type: application/octet-stream;tt-data=a` + 签名客户端
//!   五件套 + `lc: 101` + `log-encode-type: gzip` + `x-ss-dp: 8662`
//! - 响应：JSON `device_id/install_id/device_token/new_user` +
//!   Set-Cookie `install_id` + `ttreq`（票按新 install_id 发放，须随档案
//!   长期携带）；`device_id: 0` 表示被静默拒绝。
//!
//! TT-Encrypt V5 格式：
//!
//! ```text
//! packet = magic(6, "tc"+v5) ‖ salt(32 随机) ‖ AES-128-CBC(payload+PKCS7)
//!          —— 包内没有 IV 字段；key 与 iv 全部从 salt 派生且不上线
//! derive(salt) = sha512( sha512(salt) ‖ ORD_LIST(64) )   // 64B
//! key = derive[0..16]；iv = derive[16..32]
//! payload = sha512(gz) 完整 64 字节 ‖ gz，再 PKCS7
//! gzip 头 XFL/OS 覆写 00/00（flate2 默认 02/ff 与官方字节流不符）
//! ```
//!
//! 格式细节由 `tt_v5_real_capture_roundtrip` /
//! `tt_v5_reproduces_accepted_body` 两个逐字节锁锁定；解密或完整性校验
//! 失败时服务端静默回 `device_id: 0`。

mod model;

pub use model::RegisterResult;

use serde_json::{Value, json};

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
    tt_encrypt_v5_full(&gzip_java_flavor(plaintext), salt)
}

/// 加密核心：gz 字节由调用方指定（逐字节锁定测试用）。
///
/// 线上格式见模块文档；`iv` 不上线，与 key 一样从 salt 派生。
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

    // payload = gz 的完整 64 字节摘要 ‖ gz（服务端按前 64 字节做完整性
    // 校验），再 PKCS7 填充。
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

/// TT-Encrypt V5 解密（对拍/取证用）：返回 (明文字节, 抓包里的 gz 字节)。
///
/// 与加密对称：iv/key 全从 salt 派生（包内无 IV 字段），payload 前 64
/// 字节是 gz 的完整摘要。
pub fn tt_decrypt_v5(body: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    use aes::Aes128;
    use aes::cipher::{BlockCipherDecrypt, KeyInit};
    use flate2::read::GzDecoder;
    use std::io::Read;

    use hongguo_signer::tt_hash::{TT_ORD_LIST, TtHashCore};

    if body.len() < 6 + 32 + 16 || body[..6] != TT_MAGIC {
        return Err(format!(
            "不是 TT-Encrypt V5 包：len={} magic={:02x?}",
            body.len(),
            &body[..6.min(body.len())]
        ));
    }
    let salt = &body[6..38];
    let ct = &body[38..];
    if !ct.len().is_multiple_of(16) {
        return Err(format!("密文长度 {} 不是 16 的倍数", ct.len()));
    }

    let mut core = TtHashCore::new();
    let h1 = core.calculate(salt);
    let mut seed = h1;
    seed.extend_from_slice(&TT_ORD_LIST);
    let key_iv = core.calculate(&seed);
    let key_arr: [u8; 16] = key_iv[..16].try_into().unwrap();
    let iv_arr: [u8; 16] = key_iv[16..32].try_into().unwrap();

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

    if payload.len() < 64 {
        return Err(format!("payload 过短：{}", payload.len()));
    }
    let (app_hash, gz) = payload.split_at(64);

    // 完整性哈希校验：payload 前 64 字节 == gz 的完整摘要
    let calc_hash = core.calculate(gz);
    if calc_hash.as_slice() != app_hash {
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

/// 注册 query 的公共指纹参数（40 个；`cdid`/`openudid`/时间戳每次生成，
/// 服务端按这套指纹发新设备号）。版本身份字段由 [`register_device`]
/// 覆写为 73932。
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

/// 首次安装时间戳相对当前时间的回拨量（毫秒）：hgplayer `GenerateProfileWith`
/// 为 `randInt(100000,500000)`（100~500 秒），取中值 300 秒同分布。
const FIRST_INSTALL_BACK_MS: u64 = 300_000;

/// 构造 AppLog 注册 JSON 明文（73932 活形态；模板逐字段复刻自 hgplayer
/// `GenerateProfileWith`/`Header` 反编译）：`openudid` 由 cdid 派生
/// （`md5("{cdid}_{N}").hex()[:16]`，N∈[1e5,1e6)），`client_ipv4` 是
/// `116.130.x.x` 段随机伪值（服务端不校验真实出口），
/// `apk_first_install_time` 回拨 100~500 秒。
fn reg73932_json_str(id: &FreshIdentity, now_ms: u64) -> String {
    let mut h = serde_json::Map::new();
    h.insert("access".into(), json!("wifi"));
    h.insert("aid".into(), json!(8662));
    h.insert(
        "apk_first_install_time".into(),
        json!(now_ms - FIRST_INSTALL_BACK_MS),
    );
    h.insert("app_version".into(), json!("7.3.9.32"));
    h.insert("app_version_minor".into(), json!(""));
    h.insert("cdid".into(), json!(id.cdid));
    h.insert("channel".into(), json!("xiaomi_8662_64"));
    h.insert("clientudid".into(), json!(id.clientudid));
    h.insert("cpu_abi".into(), json!("arm64-v8a"));
    h.insert(
        "custom".into(),
        json!({
            "client_ipv4": format!("116.130.{}.{}", rand::random::<u8>() as u32 % 231 + 10, rand::random::<u8>() as u32 % 231 + 10),
            "filter_warn": 0,
            "is_android_fold": 0,
            "is_android_pad": 0,
            "web_ua": "Mozilla/5.0 (Linux; Android 14; Xiaomi 14 Build/UKQ1.230804.001; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/120.0.0.0 Mobile Safari/537.36",
        }),
    );
    h.insert("density_dpi".into(), json!(460));
    h.insert("device_brand".into(), json!("xiaomi"));
    h.insert("device_category".into(), json!("phone"));
    h.insert("device_manufacturer".into(), json!("Xiaomi"));
    h.insert("device_model".into(), json!("Xiaomi 14"));
    h.insert("device_platform".into(), json!("android"));
    h.insert("display_density".into(), json!("xxhdpi"));
    h.insert("display_name".into(), json!("红果短剧"));
    h.insert("git_hash".into(), json!("fe95b95"));
    h.insert("guest_mode".into(), json!(0));
    h.insert("ipv6_list".into(), json!([]));
    h.insert("is_system_app".into(), json!(0));
    h.insert("language".into(), json!("zh"));
    h.insert("manifest_version_code".into(), json!(73932));
    h.insert("not_request_sender".into(), json!(0));
    h.insert("oaid_may_support".into(), json!(true));
    h.insert("openudid".into(), json!(id.openudid));
    h.insert("os".into(), json!("Android"));
    h.insert("os_api".into(), json!(34));
    h.insert("os_version".into(), json!("14"));
    h.insert("package".into(), json!("com.phoenix.read"));
    h.insert("region".into(), json!("CN"));
    h.insert("release_build".into(), json!("UKQ1.230804.001_1701234567"));
    h.insert("req_id".into(), json!(id.req_id));
    h.insert("resolution".into(), json!("2670x1200"));
    h.insert("rom".into(), json!("hyperos"));
    h.insert("rom_version".into(), json!("OS1.0.32.0.UNCCNXM"));
    h.insert("sdk_flavor".into(), json!("china"));
    h.insert("sdk_target_version".into(), json!(29));
    h.insert("sdk_version".into(), json!("3.7.3-rc.116-douyin"));
    h.insert("sig_hash".into(), json!("aea615ab910015038f73c47e45d21466"));
    h.insert("sim_serial_number".into(), json!([]));
    h.insert("timezone".into(), json!(8));
    h.insert("tz_name".into(), json!("Asia/Shanghai"));
    h.insert("tz_offset".into(), json!(28800));
    h.insert("update_version_code".into(), json!(73932));
    h.insert("version_code".into(), json!(73932));
    let mut o = serde_json::Map::new();
    o.insert("magic_tag".into(), json!("ss_app_log"));
    o.insert("header".into(), Value::Object(h));
    o.insert("_gen_time".into(), json!(now_ms));
    Value::Object(o).to_string()
}

/// 注册一台全新设备。
///
/// 73932 活形态：40 参富 query + TT-Encrypt V5 加密的 73932 指纹 JSON +
/// 签名客户端（五件套 + `lc/x-ss-dp/log-encode-type`）。签名档案用
/// **全新指纹**（无 device_id/iid），cdid/openudid 与 query/body 三处
/// 同源。`env` 只取代理设置。
pub async fn register_device(env: &ApiEnv) -> AppResult<RegisterResult> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let id = fresh_identity();

    let mut profile =
        hongguo_signer::device::DeviceProfile::fresh_register_profile(&id.cdid, &id.openudid);
    // 注册走 73932 指纹；UA 尾段同为 73932 时代的 Cronet 形态，与档案成套
    for k in [
        "version_code",
        "update_version_code",
        "manifest_version_code",
        "pv_player",
    ] {
        profile.set(k, "73932");
    }
    profile.set("version_name", "7.3.9.32");
    profile.set_user_agent(
        "com.phoenix.read/73932 (Linux; U; Android 14; zh_CN; Xiaomi 14; \
         Build/UKQ1.230804.001; Cronet/TTNetVersion:8d40f833 QuicVersion:462f352c 2026-08-31)",
    );

    let mut q = register_query(&id.cdid, &id.openudid, now_ms);
    for (k, v) in q.iter_mut() {
        let nv = match k.as_str() {
            "version_code" | "update_version_code" | "manifest_version_code" => "73932",
            "version_name" => "7.3.9.32",
            _ => continue,
        };
        *v = nv.to_string();
    }

    let body = tt_encrypt_v5(reg73932_json_str(&id, now_ms).as_bytes());
    // debug dump（定位指纹/加密差异用，TT_DEBUG=1 时开启）
    if std::env::var("TT_DEBUG").is_ok() {
        let dir = super::capture_dir();
        let _ = std::fs::create_dir_all(&dir);
        let plain = reg73932_json_str(&id, now_ms);
        let _ = std::fs::write(dir.join("rust_reg_plain.json"), &plain);
        let hex: String = body.iter().map(|b| format!("{b:02x}")).collect();
        let _ = std::fs::write(dir.join("rust_reg_body.hex"), &hex);
        let meta = serde_json::json!({"cdid": id.cdid, "openudid": id.openudid, "clientudid": id.clientudid, "req_id": id.req_id, "_rticket": now_ms.to_string(), "ts": (now_ms / 1000).to_string()});
        let _ = std::fs::write(
            dir.join("rust_reg_meta.json"),
            serde_json::to_string(&meta).unwrap(),
        );
    }

    // 注册档案零 cookie（新指纹等服务端发号）；响应 Set-Cookie 下发本
    // 设备自己的 ttreq 票（按新 install_id 发放），要随档案长期携带
    let reg_env = ApiEnv {
        proxy: env.proxy.clone(),
        device: profile,
        cookie: None,
        x_tt_token: None,
    };

    let response = api_call_full_response(
        REGISTER_ORIGIN,
        REGISTER_PATH,
        Some(body),
        &q,
        &[
            (
                "Content-Type".into(),
                "application/octet-stream;tt-data=a".into(),
            ),
            ("lc".into(), "101".into()),
            ("log-encode-type".into(), "gzip".into()),
            ("x-ss-dp".into(), "8662".into()),
        ],
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

    /// 真实 App 抓包（register_req_real.bin，官方真机注册 body）的离线
    /// 闭环：解密 → 同 (gz,salt) 重加密 → 必须逐字节复现原始 body。
    /// 锁死「派生链 + IV 不上线 + AES-CBC + PKCS7 + gzip 字节流」与官方
    /// 客户端一致。
    #[test]
    fn tt_v5_real_capture_roundtrip() {
        let body = include_bytes!("../../tests/fixtures/register_req_real.bin");
        let (plain, app_gz) = tt_decrypt_v5(body).expect("真实抓包应能解密");

        // 解出的明文必须是合法 AppLog 注册 JSON
        let v: serde_json::Value = serde_json::from_slice(&plain).expect("解出的明文应是 JSON");
        assert_eq!(v["magic_tag"], "ss_app_log", "magic_tag 不符");
        assert_eq!(&app_gz[..2], &[0x1f, 0x8b], "payload 64 偏移处应是 gzip 流");

        let salt: [u8; 32] = body[6..38].try_into().unwrap();
        // 直接用解密出的 gz 重加密（官方压缩器与 flate2 字节流不必相同，
        // 逐字节锁定的对象是加密拼装层，不是压缩器）
        let re = tt_encrypt_v5_full(&app_gz, &salt);
        assert_eq!(re, body, "同 (gz,salt) 重加密必须逐字节复现原始 body");

        // 往返锁：重加密密文再次解密必须还原同一明文（加密自洽）
        let (plain2, _) = tt_decrypt_v5(&re).expect("重加密密文应能自解");
        assert_eq!(plain2, plain, "加解密往返不还原");
    }

    /// 「被服务端接受的注册 body」字节级复现锁：解密样本拿到 (gz, salt)
    /// 后重加密，必须逐字节相等。锁死 AES-CBC/PKCS7/64B 摘要前缀/拼装
    /// 全链路与真实接受形态一致（样本为服务端发号成功的 TT-Encrypt V5
    /// body）。
    #[test]
    fn tt_v5_reproduces_accepted_body() {
        let hex = include_str!("../../tests/fixtures/py_enc_accepted.hex");
        let hex = hex.trim();
        let body: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let (plain, gz) = tt_decrypt_v5(&body).expect("被接受样本必须能解密（64B 摘要锁）");
        let v: serde_json::Value = serde_json::from_slice(&plain).expect("被接受样本明文应是 JSON");
        assert_eq!(v["magic_tag"], "ss_app_log");

        let salt: [u8; 32] = body[6..38].try_into().unwrap();
        let re = tt_encrypt_v5_full(&gz, &salt);
        assert_eq!(re, body, "同 (gz,salt) 重加密必须逐字节复现被接受 body");
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

    /// 业务错误码必须浮出（ILLEGAL_ACCESS 等）。
    #[test]
    fn error_code_is_surfaced() {
        let raw = br#"{"code":110,"message":"ILLEGAL_ACCESS"}"#;
        let err = parse_register(raw).unwrap_err();
        assert!(err.to_string().contains("110"));
    }

    /// 注册端点存活性诊断：[`register_device`] 全链路真机试注。
    /// `device_id: 0` / 报错即形态或端点有变。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_register_live() {
        let env = ApiEnv::anonymous(hongguo_common::model::ProxyConfig::default());
        match register_device(&env).await {
            Ok(r) => println!(
                "[register-live] device_id={} install_id={} new_user={} ttreq={}",
                r.device_id,
                r.install_id,
                r.new_user,
                if r.ttreq.is_empty() { "无" } else { "有" }
            ),
            Err(e) => println!("[register-live] ERR {e}"),
        }
    }
}
