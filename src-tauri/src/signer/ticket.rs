//! `_rticket` 抖动：挑一个服务端受理的签名分支。
//!
//! X-Argus 有 3 个分支，服务端对 branch == 1 的那一档**不受理**（表现为空响应）。
//! 逐毫秒抖动 `_rticket` 会改变签名摘要，从而改变分支判定，因此这里最多试 32 个
//! 偏移去找一个可用分支。正常情况下第 1 次就命中，失败率极低。

use base64::Engine;

use crate::signer::device::DeviceProfile;
use crate::signer::primitives::{be32, le32, md5_raw, sm3};
use crate::signer::xargus::branch_of;
use crate::signer::xgorgon::x_gorgon;
use crate::signer::{build_medusa, helios};

/// 官方 App 接口 origin。
pub const API_ORIGIN: &str = "https://api5-normal-sinfonlineb.fqnovel.com";

/// 抖动上限，与 JS 实现一致。
const MAX_TICKET_OFFSET: u32 = 32;

/// 查询串编码：与 Python 的 `urlencode` 行为一致。
///
/// 分两步：先用 `encodeURIComponent` 语义转义，再对 `!'()` 额外转义。
/// 这两步**不可省略**——签名是 query 字符串的联合函数，编码方式变化即失配。
fn encode_component(item: &str) -> String {
    // encodeURIComponent 的 unreserved 集合：A-Z a-z 0-9 - _ . ! ~ * ' ( )
    const UNRESERVED: &str = "-_.!~*'()";
    let mut out = String::with_capacity(item.len());
    for byte in item.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || UNRESERVED.contains(ch) {
            out.push(ch);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// 拼查询串。
pub fn encode_query(values: &[(&str, String)]) -> String {
    values
        .iter()
        .map(|(k, v)| format!("{}={}", encode_component(k), encode_component(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// 挑一个服务端接受的 ticket。
///
/// 返回 `(url, query, ticket, khronos)`。`origin` 参数化：业务 API 与
/// passport / 设备注册走不同域名，签名只关心 pathname+query，域名不影响摘要。
/// 带业务 query 的 ticket 解析。
///
///
/// 业务参数插在设备参数之后、`ts`/`_rticket` 之前——位置本身也参与签名，
/// 与 hgplayer 的实测顺序一致；调用方不再有机会在签名后改 query。
pub fn resolve_ticket_with(
    origin: &str,
    pathname: &str,
    body: Option<&[u8]>,
    device: &DeviceProfile,
    biz_query: &[(String, String)],
    base_ticket: u64,
) -> (String, String, u64, u32) {
    for offset in 0..MAX_TICKET_OFFSET {
        let ticket = base_ticket + u64::from(offset);
        let khronos = (ticket / 1000) as u32;
        let mut params: Vec<(&str, String)> = device
            .iter()
            .map(|(k, v)| (k, v.to_string()))
            .collect();
        // 生命周期桥：biz_query 是调用方给的 owned 对，借成 &str 拼进同一张表
        let biz_owned: Vec<(String, String)> = biz_query.to_vec();
        for (k, v) in &biz_owned {
            params.push((k.as_str(), v.clone()));
        }
        params.push(("ts", khronos.to_string()));
        params.push(("_rticket", ticket.to_string()));

        let query = encode_query(&params);
        let body_md5 = body.map(md5_raw).unwrap_or([0u8; 16]);

        // ⚠️ 传进去的必须是 query 的 **SM3 摘要**，不是 query 原文。
        //    传原文会算出与实际 x-argus 不同的分支，抖动挑出的 ticket
        //    可能正好落在服务端不受理的 branch 1 上，表现为 HTTP 200 + 0 字节。
        if branch_of(&sm3(query.as_bytes()), &body_md5, &le32(khronos)) != 1 {
            let url = format!("{origin}{pathname}?{query}");
            return (url, query, ticket, khronos);
        }
    }

    // 32 次都落在 branch 1：沿用最后一次的偏移（与 JS 一致）
    let ticket = base_ticket + u64::from(MAX_TICKET_OFFSET - 1);
    let khronos = (ticket / 1000) as u32;
    let mut params: Vec<(&str, String)> = device
        .iter()
        .map(|(k, v)| (k, v.to_string()))
        .collect();
    let biz_owned: Vec<(String, String)> = biz_query.to_vec();
    for (k, v) in &biz_owned {
        params.push((k.as_str(), v.clone()));
    }
    params.push(("ts", khronos.to_string()));
    params.push(("_rticket", ticket.to_string()));
    let query = encode_query(&params);
    let url = format!("{origin}{pathname}?{query}");
    (url, query, ticket, khronos)
}

/// 一次签名请求所需的全部材料。
///
/// 查询串不单独存一份：它已经原样拼在 `url` 里，单独再存一份只会让人
/// 有机会只改其中一处，把「签过名的 query」与「实际发出的 query」拆开。
pub struct SignedRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// 给一个请求补齐全部签名头。
///
/// ⚠️ 签名是「URL + body + 时间戳」三者的联合函数，任一处在发出前被改动
///    （包括 query 参数顺序、URL 编码方式、body 的字节内容）都会导致签名失配，
///    服务端静默丢弃（HTTP 200 + 空 body）。请勿在签完名之后再动 url / body。
pub fn sign_request(
    origin: &str,
    pathname: &str,
    body: Option<Vec<u8>>,
    device: &DeviceProfile,
    extra_headers: &[(String, String)],
) -> SignedRequest {
    sign_request_with(origin, pathname, body, device, &[], extra_headers)
}

/// [`sign_request`] 的带业务 query 版本（reading 系接口的混合参数形态）。
pub fn sign_request_with(
    origin: &str,
    pathname: &str,
    body: Option<Vec<u8>>,
    device: &DeviceProfile,
    biz_query: &[(String, String)],
    extra_headers: &[(String, String)],
) -> SignedRequest {
    let (url, query, ticket, khronos) =
        resolve_ticket_with(origin, pathname, body.as_deref(), device, biz_query, now_millis());

    let random = rand::random::<u16>();
    let b64 = base64::engine::general_purpose::STANDARD;

    let mut headers: Vec<(String, String)> = vec![
        ("User-Agent".into(), device.user_agent().to_string()),
        (
            "Accept".into(),
            "application/json; charset=utf-8,application/x-protobuf".into(),
        ),
        ("x-xs-from-web".into(), "0".into()),
        ("x-ss-req-ticket".into(), ticket.to_string()),
        ("x-tt-request-tag".into(), "t=0;n=0".into()),
        ("sdk-version".into(), "2".into()),
        ("passport-sdk-version".into(), "50561".into()),
        ("x-vc-bdturing-sdk-version".into(), "3.7.2.cn".into()),
        ("x-khronos".into(), khronos.to_string()),
        // x-ladon / x-argus 目前都直接用时间戳，保留字段位置以兼容后续协议变更
        ("x-ladon".into(), b64.encode(be32(khronos))),
        ("x-argus".into(), b64.encode(le32(khronos))),
        (
            "x-gorgon".into(),
            x_gorgon(&query, body.as_deref(), khronos, u32::from(random)),
        ),
        ("x-helios".into(), helios(khronos)),
        (
            "x-medusa".into(),
            build_medusa(
                &url,
                body.as_deref(),
                khronos,
                device.get("device_id"),
                device.get("version_name"),
            ),
        ),
        ("x-tt-dt".into(), String::new()),
    ];

    if let Some(b) = &body {
        // 调用方显式给过 content-type（如 passport 系的 form 请求）就不
        // 再叠默认 JSON——两个同名头会让服务端按第一个解析，form body
        // 被当 JSON 读，业务字段全丢（2026-10-05 发码 1003 事故根因）
        let has_ct = extra_headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
        if !has_ct {
            headers.push((
                "Content-Type".into(),
                "application/json; charset=UTF-8".into(),
            ));
        }
        headers.push(("x-ss-stub".into(), crate::signer::md5_hex_upper(b)));
    }

    headers.extend_from_slice(extra_headers);

    SignedRequest { url, headers, body }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// POST + 签名（默认业务 origin）。
// M3 登录模块的便捷入口；当前仅测试引用，lib 构建下暂无消费者。
#[allow(dead_code)]
pub fn sign_post(pathname: &str, payload: Vec<u8>, device: &DeviceProfile) -> SignedRequest {
    sign_request(API_ORIGIN, pathname, Some(payload), device, &[])
}

/// GET + 签名（默认业务 origin）。
// 同 sign_post：M3 的便捷入口，当前仅测试引用。
#[allow(dead_code)]
pub fn sign_get(pathname: &str, device: &DeviceProfile) -> SignedRequest {
    sign_request(API_ORIGIN, pathname, None, device, &[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signer::device::video_device;

    #[test]
    fn encode_query_escapes_specials() {
        let got = encode_query(&[("a", "1".into()), ("b", "x y".into())]);
        assert!(got.contains('&'));
        assert!(got.contains('='));
        assert!(
            got.contains("%20") || got.contains("+"),
            "空格应被转义: {got}"
        );
    }

    #[test]
    fn resolve_ticket_avoids_branch_1() {
        let device = video_device();
        for i in 0..30u64 {
            let (_, _, ticket, _) = resolve_ticket_with(
                API_ORIGIN,
                "/novel/player/multi_video_detail/v1/",
                None,
                &device,
                &[],
                1_700_000_000_000 + i,
            );
            assert!(ticket >= 1_700_000_000_000 + i, "ticket 应随时间戳递增");
        }
    }

    #[test]
    fn sign_get_produces_all_required_headers() {
        let device = video_device();
        let signed = sign_get("/novel/player/multi_video_detail/v1/", &device);

        let names: Vec<&str> = signed.headers.iter().map(|(k, _)| k.as_str()).collect();
        for required in [
            "User-Agent",
            "x-ss-req-ticket",
            "x-khronos",
            "x-ladon",
            "x-argus",
            "x-gorgon",
            "x-helios",
            "x-medusa",
        ] {
            assert!(names.contains(&required), "缺少签名头 {required}");
        }
        // GET 不应带 body 相关的头
        assert!(!names.contains(&"x-ss-stub"));
        assert!(signed.body.is_none());
    }

    #[test]
    fn sign_post_adds_stub_header() {
        let device = video_device();
        let signed = sign_post("/x/v1/", b"{\"series_id\":1}".to_vec(), &device);
        let names: Vec<&str> = signed.headers.iter().map(|(k, _)| k.as_str()).collect();
        assert!(names.contains(&"x-ss-stub"));
        assert!(names.contains(&"Content-Type"));
    }

    #[test]
    fn signed_url_contains_api_origin() {
        let device = video_device();
        let signed = sign_get("/x/v1/", &device);
        assert!(signed.url.starts_with(API_ORIGIN));
        assert!(signed.url.contains("/x/v1/?"));
    }

    #[test]
    fn sign_request_honors_custom_origin_and_profile_ua() {
        // passport / 设备注册走别的域名；UA 必须取自档案而非全局常量，
        // 注册来的档案换机型时 UA 要跟着换
        let mut device = video_device();
        device.set("device_id", "42");
        let custom_ua = "com.phoenix.read/73932 (Linux; U; Android 14; zh_CN; Xiaomi 14)";
        // 换 UA 不改字段：serde 打个补丁重建（等价于注册流程换机型档案）
        let profile: crate::signer::device::DeviceProfile = {
            let mut v = serde_json::to_value(&device).unwrap();
            v["user_agent"] = serde_json::Value::String(custom_ua.into());
            serde_json::from_value(v).unwrap()
        };
        let signed = sign_request(
            "https://passport.example.com",
            "/passport/mobile/send_code/v1/",
            None,
            &profile,
            &[],
        );
        assert!(signed.url.starts_with("https://passport.example.com/passport/"));
        let ua = signed
            .headers
            .iter()
            .find(|(k, _)| k == "User-Agent")
            .map(|(_, v)| v.as_str())
            .unwrap();
        assert_eq!(ua, custom_ua);
        // 设备参数照常进 query（含改过的 device_id）
        assert!(signed.url.contains("device_id=42"));
    }

    #[test]
    fn resolve_ticket_url_ends_with_its_query() {
        // url 必须原样带上签过名的那份 query：调用方不再单独持有 query，
        // 一旦这里拼接分叉，x-gorgon / x-medusa 就会与实际发出的 query 不符
        let device = video_device();
        let (url, query, ticket, khronos) =
            resolve_ticket_with(API_ORIGIN, "/x/v1/", None, &device, &[], 1_700_000_000_000);
        assert!(url.ends_with(&query), "url 应原样带出 query");
        assert!(query.contains(&format!("_rticket={ticket}")));
        assert!(query.contains(&format!("ts={khronos}")));
    }

    #[test]
    fn signed_headers_match_the_query_in_url() {
        let device = video_device();
        let signed = sign_get("/x/v1/", &device);

        let query = signed.url.split_once('?').expect("url 应带 query").1;
        let params: Vec<(&str, &str)> = query
            .split('&')
            .map(|pair| pair.split_once('=').expect("参数应是 k=v"))
            .collect();

        let device_keys: Vec<&str> = device.iter().map(|(key, _)| key).collect();
        let query_keys: Vec<&str> = params.iter().map(|(key, _)| *key).collect();
        assert_eq!(
            query_keys[..device_keys.len()],
            device_keys[..],
            "设备参数顺序就是 query 顺序，改动会直接失配"
        );

        let header = |name: &str| {
            signed
                .headers
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
                .unwrap_or_else(|| panic!("缺少签名头 {name}"))
        };
        assert_eq!(
            params.get(device_keys.len()),
            Some(&("ts", header("x-khronos")))
        );
        assert_eq!(
            params.get(device_keys.len() + 1),
            Some(&("_rticket", header("x-ss-req-ticket")))
        );
    }
}
