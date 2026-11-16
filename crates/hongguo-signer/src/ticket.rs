//! `_rticket` 抖动：挑一个服务端受理的签名分支。
//!
//! X-Argus 服务端对 branch == 1 那档**不受理**（空响应）；逐毫秒抖动 `_rticket` 会改变
//! 签名摘要从而改变分支判定，最多试 32 个偏移找可用分支，正常第 1 次就命中。

use base64::Engine;

use crate::device::DeviceProfile;
use crate::primitives::{be32, le32, md5_raw, sm3};
use crate::xargus::branch_of;
use crate::xgorgon::x_gorgon;
use crate::{build_medusa, helios};

/// 官方 App 接口 origin。
pub const API_ORIGIN: &str = "https://api5-normal-sinfonlineb.fqnovel.com";

/// 抖动上限，与 JS 实现一致。
const MAX_TICKET_OFFSET: u32 = 32;

/// 查询串编码：与 Python `urlencode` / `encodeURIComponent` 语义一致（unreserved 含 `!'()` 原样保留）。
/// 编码方式**不可改**——签名是 query 字符串的联合函数，编码变化即失配。
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

/// 挑一个服务端接受的 ticket，返回 `(url, query, ticket, khronos)`；签名只关心 pathname+query，`origin` 可换域名。
/// 业务参数插在设备参数之后、`ts`/`_rticket` 之前——位置本身参与签名，调用方没有机会在签名后改 query。
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
        let mut params: Vec<(&str, String)> =
            device.iter().map(|(k, v)| (k, v.to_string())).collect();
        // 生命周期桥：biz_query 是调用方给的 owned 对，借成 &str 拼进同一张表
        let biz_owned: Vec<(String, String)> = biz_query.to_vec();
        for (k, v) in &biz_owned {
            params.push((k.as_str(), v.clone()));
        }
        params.push(("ts", khronos.to_string()));
        params.push(("_rticket", ticket.to_string()));

        let query = encode_query(&params);
        let body_md5 = body.map(md5_raw).unwrap_or([0u8; 16]);

        // ⚠️ 传进去的必须是 query 的 **SM3 摘要**，不是 query 原文——传原文会算出
        //    与实际 x-argus 不同的分支，抖动挑出的 ticket 可能落在不受理的 branch 1（HTTP 200 + 0 字节）。
        if branch_of(&sm3(query.as_bytes()), &body_md5, &le32(khronos)) != 1 {
            let url = format!("{origin}{pathname}?{query}");
            return (url, query, ticket, khronos);
        }
    }

    // 32 次都落在 branch 1：沿用最后一次的偏移（与 JS 一致）
    let ticket = base_ticket + u64::from(MAX_TICKET_OFFSET - 1);
    let khronos = (ticket / 1000) as u32;
    let mut params: Vec<(&str, String)> = device.iter().map(|(k, v)| (k, v.to_string())).collect();
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
/// 查询串不单独存一份：它已原样拼在 `url` 里，单独再存会让人有机会只改一处，把「签过名的 query」与「实际发出的 query」拆开。
pub struct SignedRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// 签名绑定 URL、body 与时间戳，发出前不得修改已签名的请求。
pub fn sign_request_with(
    origin: &str,
    pathname: &str,
    body: Option<Vec<u8>>,
    device: &DeviceProfile,
    biz_query: &[(String, String)],
    extra_headers: &[(String, String)],
) -> SignedRequest {
    let (url, query, ticket, khronos) = resolve_ticket_with(
        origin,
        pathname,
        body.as_deref(),
        device,
        biz_query,
        now_millis(),
    );

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
        ("sdk-version".into(), "2".into()),
        // 旧值 50561 / 3.7.2.cn 会被服务端按旧指纹拒
        ("passport-sdk-version".into(), "5051452".into()),
        ("x-vc-bdturing-sdk-version".into(), "4.0.3.cn".into()),
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
                device.get("cdid"),
                device.get("device_id"),
                device.get("version_name"),
            ),
        ),
    ];
    // 不带 x-tt-dt / x-tt-request-tag：官方业务请求不带这两个头，匿名请求带上会被静默拒（空响应）

    if let Some(b) = &body {
        // 调用方显式给过 content-type（如 passport 系 form 请求）就不再叠默认 JSON——
        // 两个同名头会让服务端按第一个解析，form body 被当 JSON 读，业务字段全丢
        let has_ct = extra_headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
        if !has_ct {
            headers.push((
                "Content-Type".into(),
                "application/json; charset=UTF-8".into(),
            ));
        }
        headers.push(("x-ss-stub".into(), crate::md5_hex_upper(b)));
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
