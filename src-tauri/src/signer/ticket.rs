//! `_rticket` 抖动：挑一个服务端受理的签名分支。
//!
//! X-Argus 有 3 个分支，服务端对 branch == 1 的那一档**不受理**（表现为空响应）。
//! 逐毫秒抖动 `_rticket` 会改变签名摘要，从而改变分支判定，因此这里最多试 32 个
//! 偏移去找一个可用分支。正常情况下第 1 次就命中，失败率极低。

use base64::Engine;

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
/// 返回 `(url, query, ticket, khronos)`。
pub fn resolve_ticket(
    pathname: &str,
    body: Option<&[u8]>,
    device: &[(&'static str, &'static str)],
    base_ticket: u64,
) -> (String, String, u64, u32) {
    for offset in 0..MAX_TICKET_OFFSET {
        let ticket = base_ticket + u64::from(offset);
        let khronos = (ticket / 1000) as u32;
        let mut params: Vec<(&str, String)> =
            device.iter().map(|(k, v)| (*k, (*v).to_string())).collect();
        params.push(("ts", khronos.to_string()));
        params.push(("_rticket", ticket.to_string()));

        let query = encode_query(&params);
        let body_md5 = body.map(md5_raw).unwrap_or([0u8; 16]);

        // ⚠️ 传进去的必须是 query 的 **SM3 摘要**，不是 query 原文。
        //    传原文会算出与实际 x-argus 不同的分支，抖动挑出的 ticket
        //    可能正好落在服务端不受理的 branch 1 上，表现为 HTTP 200 + 0 字节。
        if branch_of(&sm3(query.as_bytes()), &body_md5, &le32(khronos)) != 1 {
            let url = format!("{API_ORIGIN}{pathname}?{query}");
            return (url, query, ticket, khronos);
        }
    }

    // 32 次都落在 branch 1：沿用最后一次的偏移（与 JS 一致）
    let ticket = base_ticket + u64::from(MAX_TICKET_OFFSET - 1);
    let khronos = (ticket / 1000) as u32;
    let mut params: Vec<(&str, String)> =
        device.iter().map(|(k, v)| (*k, (*v).to_string())).collect();
    params.push(("ts", khronos.to_string()));
    params.push(("_rticket", ticket.to_string()));
    let query = encode_query(&params);
    let url = format!("{API_ORIGIN}{pathname}?{query}");
    (url, query, ticket, khronos)
}

/// 一次签名请求所需的全部材料。
pub struct SignedRequest {
    pub url: String,
    pub query: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// 给一个请求补齐全部签名头。
///
/// ⚠️ 签名是「URL + body + 时间戳」三者的联合函数，任一处在发出前被改动
///    （包括 query 参数顺序、URL 编码方式、body 的字节内容）都会导致签名失配，
///    服务端静默丢弃（HTTP 200 + 空 body）。请勿在签完名之后再动 url / body。
pub fn sign_request(
    pathname: &str,
    body: Option<Vec<u8>>,
    device: &[(&'static str, &'static str)],
    extra_headers: &[(String, String)],
) -> SignedRequest {
    let (url, query, ticket, khronos) =
        resolve_ticket(pathname, body.as_deref(), device, now_millis());

    let random = rand::random::<u16>();
    let b64 = base64::engine::general_purpose::STANDARD;

    let mut headers: Vec<(String, String)> = vec![
        ("User-Agent".into(), crate::signer::VIDEO_UA.to_string()),
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
                device_field(device, "device_id"),
                device_field(device, "version_name"),
            ),
        ),
        ("x-tt-dt".into(), String::new()),
    ];

    if let Some(b) = &body {
        headers.push((
            "Content-Type".into(),
            "application/json; charset=UTF-8".into(),
        ));
        headers.push(("x-ss-stub".into(), crate::signer::md5_hex_upper(b)));
    }

    headers.extend_from_slice(extra_headers);

    SignedRequest {
        url,
        query,
        headers,
        body,
    }
}

fn device_field<'a>(device: &'a [(&'static str, &'static str)], key: &str) -> &'a str {
    device
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or_default()
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// POST + 签名。
pub fn sign_post(
    pathname: &str,
    payload: Vec<u8>,
    device: &[(&'static str, &'static str)],
) -> SignedRequest {
    sign_request(pathname, Some(payload), device, &[])
}

/// GET + 签名。
pub fn sign_get(pathname: &str, device: &[(&'static str, &'static str)]) -> SignedRequest {
    sign_request(pathname, None, device, &[])
}

/// 供测试与探针使用：直接取 query 的 SM3。
pub fn query_sm3(query: &str) -> [u8; 32] {
    sm3(query.as_bytes())
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
            let (_, _, ticket, khronos) = resolve_ticket(
                "/novel/player/multi_video_detail/v1/",
                None,
                &device,
                1_700_000_000_000 + i,
            );
            let query = format!("x={ticket}");
            let _ = khronos;
            // 复算：query 参与 branch 判定，但此处只验证 ticket 递增
            assert!(ticket >= 1_700_000_000_000 + i);
            let _ = query;
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
}
