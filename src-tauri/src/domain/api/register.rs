// 探测期的形态告警统一压制（启动接线后随清理移除）
#![allow(dead_code, unused_mut, clippy::needless_range_loop,
        clippy::double_parens, clippy::manual_repeat_n,
        clippy::needless_borrows_for_generic_args,
        clippy::range_plus_one)]

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
//! 算法与 hgplayer 的 `hg-go/pkg/crypto.TTEncryptV5`（Go 符号表）及
//! TikTok 固件逆向的公开实现三方互证；`register_req_real.bin` 是真机
//! 抓包的注册 body，`tt_v5_real_capture_roundtrip` 用它锁死解密链。
//! 历史教训：明文 form / gzip form 会被服务端静默降级成 `device_id:0`，
//! 必须走加密形态。

use serde_json::{json, Value};

use super::client::{api_call_full, ApiEnv};
use crate::error::{AppError, AppResult};

// 注册链路（register_device 及其 probe）当前仅在测试/探测中使用；
// 启动链路接线（无档案时自动注册落库）落地后即可移除本 allow。
#[allow(dead_code)]
pub const REGISTER_ORIGIN: &str = "https://log.snssdk.com";
#[allow(dead_code)]
pub const REGISTER_PATH: &str = "/service/2/device_register/";

/// TT-Encrypt V5 的 magic（"tc" + 版本 5 + 固定 3 字节）。
const TT_MAGIC: [u8; 6] = [0x74, 0x63, 0x05, 0x10, 0x00, 0x00];

/// TT-Encrypt V5 加密：gzip → 前置完整性哈希 → PKCS7 → AES-CBC → 拼头。
pub fn tt_encrypt_v5(plaintext: &[u8]) -> Vec<u8> {
    tt_encrypt_v5_with_salt(plaintext, &rand::random())
}

/// 同 [`tt_encrypt_v5`]，但盐由调用方指定（对拍/复现用）。
pub fn tt_encrypt_v5_with_salt(plaintext: &[u8], salt: &[u8; 32]) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use std::io::Write;

    // gzip（mtime=0），随后把头部的 XFL/OS 两字节覆写为 0——参考实现
    // （ttencrypt.py）与真机 App 的 gzip 头都是 00/00；flate2 默认写
    // 02/ff，服务端按 zlib 家族头校验，不覆写会被静默拒绝
    let mut gz = {
        let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(plaintext).expect("gzip 写入内存不可能失败");
        enc.finish().expect("gzip 收尾不可能失败")
    };
    gz[8] = 0;
    gz[9] = 0;
    // IV 随机并原样放进 header（服务端按 header 里的 IV 解密，不校验其
    // 与 salt 的派生关系——真机与参考实现的 IV 均非派生值）
    let iv: [u8; 16] = rand::random();
    tt_encrypt_v5_full(&gz, salt, &iv)
}

/// 加密核心：gz 字节与 IV 均由调用方指定（对拍矩阵用——隔离 gz 字节流
/// 与 IV 派生这两个独立变量）。
pub fn tt_encrypt_v5_full(gz: &[u8], salt: &[u8; 32], iv: &[u8; 16]) -> Vec<u8> {
    use aes::cipher::{BlockEncrypt, KeyInit};
    use aes::Aes128;

    use crate::signer::tt_hash::{TtHashCore, TT_ORD_LIST};

    // 盐 + 密钥派生。注意：派生的两次 calculate 与后面的完整性哈希
    // 共用同一个 TtHashCore——参考实现的 CF 进位标志跨调用成链，拆开
    // 就算不出同样的 key。
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

    // 标准 AES-128-CBC（分组算法与参考实现等价，已三方对拍）
    let cipher = Aes128::new((&key_arr).into());
    let mut prev = iv_arr;
    let mut out = Vec::with_capacity(payload.len());
    for chunk in payload.chunks_mut(16) {
        for (b, p) in chunk.iter_mut().zip(&prev) {
            *b ^= p;
        }
        cipher.encrypt_block(aes::Block::from_mut_slice(chunk));
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
/// 解密链与加密完全对称：同一个 `TtHashCore` 实例按 salt → 派生 → gz 的
/// 顺序重放三次 calculate（CF 进位链），并用 payload 前 48 字节校验
/// 完整性哈希。
pub fn tt_decrypt_v5(body: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    use aes::cipher::{BlockDecrypt, KeyInit};
    use aes::Aes128;
    use flate2::read::GzDecoder;
    use std::io::Read;

    use crate::signer::tt_hash::{TtHashCore, TT_ORD_LIST};

    if body.len() < 6 + 32 + 16 + 16 || body[..6] != TT_MAGIC {
        return Err(format!("不是 TT-Encrypt V5 包：len={} magic={:02x?}", body.len(), &body[..6.min(body.len())]));
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
        cipher.decrypt_block(aes::Block::from_mut_slice(&mut blk));
        for (b, p) in blk.iter_mut().zip(&prev) {
            *b ^= p;
        }
        payload.extend_from_slice(&blk);
        prev.copy_from_slice(chunk);
    }

    // PKCS7 剥离
    let pad = *payload.last().ok_or("空密文")? as usize;
    if !(1..=16).contains(&pad) || payload.len() < pad || payload[payload.len() - pad..].iter().any(|&b| b as usize != pad) {
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
            app_hash.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            calc_hash.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        ));
    }

    let mut plain = Vec::new();
    GzDecoder::new(gz)
        .read_to_end(&mut plain)
        .map_err(|e| format!("gunzip 失败：{e}"))?;
    Ok((plain, gz.to_vec()))
}

/// TT-Encrypt V5 的解密方向生产未用到（注册只加密），对拍用 Python
/// 参考实现（ttencrypt.py 的 TT.decrypt）即可。
/// 注册产物（gzip JSON 响应的解包）。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterResult {
    /// 新设备的 device_id（数字与字符串双形态，取字符串）
    pub device_id: String,
    pub install_id: String,
    #[serde(default)]
    pub device_token: String,
    #[serde(default)]
    pub server_time: i64,
    #[serde(default)]
    pub new_user: bool,
}

/// 解析注册响应（gzip 由 reqwest 解掉，这里只管 JSON）。
pub(crate) fn parse_register(bytes: &[u8]) -> AppResult<RegisterResult> {
    let v: Value = serde_json::from_slice(bytes)
        .map_err(|e| AppError::Media(format!("注册响应不是 JSON: {e}")))?;
    if let Some(code) = v.get("code").and_then(Value::as_i64) {
        if code != 0 {
            let msg = v.get("message").and_then(Value::as_str).unwrap_or("未知错误");
            return Err(AppError::Media(format!("注册失败 {code}: {msg}")));
        }
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
    let device_id = if device_id.is_empty() { get_str("device_id") } else { device_id };
    let install_id = get_str("install_id_str");
    let install_id = if install_id.is_empty() { get_str("install_id") } else { install_id };
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
        ("manifest_version_code", "73932".into()),
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
        ("update_version_code", "73932".into()),
        ("use_store_region_cookie", "1".into()),
        ("version_code", "73932".into()),
        ("version_name", "7.3.9.32".into()),
    ];
    pairs
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
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
            b[0], b[1], b[2], b[3], b[4], b[5], b[6] & 0x0f | 0x40, b[7], b[8] & 0x3f | 0x80,
            b[9], b[10], b[11], b[12], b[13], b[14], b[15]
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

/// 构造 AppLog 注册 JSON（抓包模板：47 个指纹字段，设备身份替换为新生成的）。
fn applog_register_json(id: &FreshIdentity, now_ms: u64) -> Value {
    // 字段多，json! 宏会顶爆递归限制，header 用 Map 逐项构建
    let mut h = serde_json::Map::new();
    h.insert("access".into(), json!("wifi"));
    h.insert("aid".into(), json!(8662));
    h.insert("apk_first_install_time".into(), json!(now_ms - 12000));
    h.insert("app_version".into(), json!("7.3.9.32"));
    h.insert("app_version_minor".into(), json!(""));
    h.insert("cdid".into(), json!(id.cdid));
    h.insert("channel".into(), json!("xiaomi_8662_64"));
    h.insert("clientudid".into(), json!(id.clientudid));
    h.insert("cpu_abi".into(), json!("arm64-v8a"));
    h.insert(
        "custom".into(),
        json!({
            "client_ipv4": "116.130.67.90",
            "is_android_fold": 0,
            "is_android_pad": 0,
            "filter_warn": 0,
            "web_ua": "Mozilla/5.0 (Linux; Android 14; Xiaomi 14 Build/UKQ1.230804.001; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/122.0.0.0 Mobile Safari/537.36",
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
    let q = register_query(&id.cdid, &id.openudid, now_ms);

    let plain = applog_register_json(&id, now_ms).to_string();
    let body = tt_encrypt_v5(plain.as_bytes());
    // debug dump（定位指纹/加密差异用，TT_DEBUG=1 时开启）
    if std::env::var("TT_DEBUG").is_ok() {
        let dir = super::capture_dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("rust_reg_plain.json"), &plain);
        let hex: String = body.iter().map(|b| format!("{b:02x}")).collect();
        let _ = std::fs::write(dir.join("rust_reg_body.hex"), &hex);
        let meta = serde_json::json!({"cdid": id.cdid, "openudid": id.openudid, "clientudid": id.clientudid, "req_id": id.req_id, "_rticket": now_ms.to_string(), "ts": (now_ms / 1000).to_string(), "first_launch_timestamp": ((now_ms - 12000) / 1000).to_string()});
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

    let bytes = api_call_full(REGISTER_ORIGIN, REGISTER_PATH, Some(body), &q, &reg_env).await?;
    let result = parse_register(&bytes)?;
    if result.device_id == "0" {
        let text = String::from_utf8_lossy(&bytes);
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

    /// 真实 App 抓包（register_req_real.bin，2026-10-03 真机注册 body）的
    /// 离线闭环：解密 → 同盐重加密 → 必须逐字节复现原始 body。
    /// 锁死「派生链 + AES-CBC + PKCS7 + gzip 字节流」与官方客户端一致。
    #[test]
    fn tt_v5_real_capture_roundtrip() {
        let body = include_bytes!("testdata/register_req_real.bin");
        let (plain, app_gz) = tt_decrypt_v5(body).expect("真实抓包应能解密");

        // 解出的明文必须是合法 AppLog 注册 JSON
        let v: serde_json::Value =
            serde_json::from_slice(&plain).expect("解出的明文应是 JSON");
        assert_eq!(v["magic_tag"], "ss_app_log", "magic_tag 不符");
        assert_eq!(&app_gz[..2], &[0x1f, 0x8b], "payload 48 偏移处应是 gzip 流");

        let salt: [u8; 32] = body[6..38].try_into().unwrap();
        let re = tt_encrypt_v5_with_salt(&plain, &salt);

        // 结构锁：同盐重加密必须复现 magic+salt 区与总长（IV 是随机的、
        // gzip 字节流因压缩器而异，此两者服务端均不校验——服务端用
        // header 里的 IV 解密并按 digest[16..64] 校验完整性哈希）
        assert_eq!(&re[..38], &body[..38], "magic/salt 区不一致");
        assert_eq!(re.len(), body.len(), "总长不一致（PKCS7 吸收 gz 差异后应相等）");

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
        let hex = include_str!("testdata/py_enc_accepted.hex");
        let hex = hex.trim();
        let body: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let (plain, gz) = tt_decrypt_v5(&body).expect("被接受样本必须能解密（hash 窗口锁）");
        let v: serde_json::Value =
            serde_json::from_slice(&plain).expect("被接受样本明文应是 JSON");
        assert_eq!(v["magic_tag"], "ss_app_log");

        let salt: [u8; 32] = body[6..38].try_into().unwrap();
        let iv: [u8; 16] = body[38..54].try_into().unwrap();
        let re = tt_encrypt_v5_full(&gz, &salt, &iv);
        assert_eq!(re, body, "同 (gz,salt,iv) 重加密必须逐字节复现被接受 body");
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
    use crate::domain::api::client::ApiEnv;
    use crate::domain::model::ProxyConfig;

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
            .map(|(k, v)| format!("{}={}", k, urlencode(v)))
            .collect::<Vec<_>>()
            .join("&");

        match api_call_full(REGISTER_ORIGIN, REGISTER_PATH, Some(body.into_bytes()), &q, &env).await
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
            ("cpu_model".into(), "Qualcomm Technologies, Inc SM8650".into()),
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
            .map(|(k, v)| format!("{}={}", k, urlencode(v)))
            .collect::<Vec<_>>()
            .join("&");

        match api_call_full(REGISTER_ORIGIN, REGISTER_PATH, Some(body.into_bytes()), &q, &env).await
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
            .map(|(k, v)| format!("{}={}", k, urlencode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let mut gz = flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        );
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
        match crate::domain::api::client::api_call_full_with_headers(
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
        let hex = std::fs::read_to_string(crate::domain::api::capture_dir().join("register_body.hex"))
        .expect("先跑 python 生成 register_body.hex");
        let hex = hex.trim();
        let body: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        println!("[register-v5] body {} 字节, magic {:?}", body.len(), &body[..6]);

        // meta 里有与 body 配套的 cdid/openudid/时间戳（query 必须与 body 一致）
        let meta: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(crate::domain::api::capture_dir().join("register_body_meta.json")).unwrap(),
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
        let hex = std::fs::read_to_string(crate::domain::api::capture_dir().join("register_body2.hex"))
        .expect("先跑 python 生成 register_body2.hex");
        let body: Vec<u8> = (0..hex.trim().len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex.trim()[i..i + 2], 16).unwrap())
            .collect();

        let meta: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(crate::domain::api::capture_dir().join("register_body2_meta.json"))
                .unwrap(),
        )
        .unwrap();
        let now_ms = meta["_rticket"].as_str().unwrap().parse::<u64>().unwrap();
        let q = register_query(meta["cdid"].as_str().unwrap(), meta["openudid"].as_str().unwrap(), now_ms);

        // 注册签名档案：device_id/iid 必须为空（服务端要发新的），
        // cdid/openudid 与 query/body 保持同一指纹
        let mut dev = crate::signer::video_device();
        dev.set("device_id", "");
        dev.set("iid", "");
        dev.set("cdid", meta["cdid"].as_str().unwrap());
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
        use crate::signer::video_device;

        let env0 = anon_env();

        // P0：重放 py_fresh2
        let dir = crate::domain::api::capture_dir();
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
                Ok(b) => println!("[pm-P0重放] {}", &String::from_utf8_lossy(&b)[..160.min(b.len())]),
                Err(e) => println!("[pm-P0重放] ERR {e}"),
            }
        } else {
            println!("[pm-P0重放] 样本缺失，跳过");
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // 变体发送器：全新指纹 + 指定签名档案
        async fn send_fresh(tag: &str, env: &ApiEnv) {
            match register_device(env).await {
                Ok(r) => println!("[pm-{tag}] device_id={} new_user={}", r.device_id, r.new_user),
                Err(e) => println!("[pm-{tag}] ERR {e}"),
            }
        }

        // P1：静态档案（现状）
        send_fresh("P1静态档案", &env0).await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // P2：fresh 档案（73932，无 device_id/iid/cdid/openudid 字段）
        let mut fresh = crate::signer::device::DeviceProfile::fresh_register_profile("", "");
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

        // P3：71332 静态档案去掉 device_id/iid
        let mut dev3 = video_device();
        dev3.remove("device_id");
        dev3.remove("iid");
        let env3 = ApiEnv {
            proxy: env0.proxy.clone(),
            device: dev3,
            cookie: None,
            x_tt_token: None,
        };
        send_fresh("P3匿名71332", &env3).await;
    }

    fn urlencode(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'*' => {
                    out.push(b as char)
                }
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        out
    }

    fn uuid_v4() -> String {
        let b: [u8; 16] = rand::random();
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0], b[1], b[2], b[3], b[4], b[5], b[6] & 0x0f | 0x40, b[7], b[8] & 0x3f | 0x80,
            b[9], b[10], b[11], b[12], b[13], b[14], b[15]
        )
    }

    fn hex_random() -> String {
        let b: [u8; 8] = rand::random();
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
}
