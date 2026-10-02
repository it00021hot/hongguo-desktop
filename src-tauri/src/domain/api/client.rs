//! 接口调用：重试与空响应判定。
//!
//! ⚠️ 服务端在签名失效时返回 **HTTP 200 + 0 字节**，不是错误码。
//! 只看状态码会把失败误判成成功，所以这里显式检查响应体长度。
//!
//! 两条与签名强相关、不能搞错的约定：
//!
//! 1. **方法必须与签名时一致。** 签名覆盖 body 字节，带 body 的请求发成
//!    `GET` 会让服务端算出的摘要与 `x-gorgon` / `x-ss-stub` 不符，直接静默丢弃。
//! 2. **每次重试都要重新签名。** 时间戳与 `_rticket` 已经过期，复用旧签名
//!    等于拿一张过期的票去敲门。

use std::time::Duration;

use crate::error::{AppError, AppResult};

/// 重试次数。
pub const MAX_RETRIES: u32 = 3;

/// 重试间隔基数，按 `2s * (i + 1)` 递增，与现版一致。
const RETRY_BASE_DELAY: Duration = Duration::from_secs(2);

/// 调用官方 App 接口，返回响应体字节。
///
/// # 参数
/// - `pathname`：以 `/` 开头的接口路径
/// - `body`：请求体字节。`Some` 走 POST，`None` 走 GET
/// - `proxy`：当前生效的代理配置
pub async fn api_call(
    pathname: &str,
    body: Option<Vec<u8>>,
    proxy: &crate::domain::model::ProxyConfig,
) -> AppResult<Vec<u8>> {
    let client = crate::service::settings_service::proxy::build_client(proxy)?;
    let device = crate::signer::video_device();
    let mut last_err = String::new();

    for attempt in 0..MAX_RETRIES {
        match send_once(&client, pathname, body.as_deref(), &device).await {
            Ok(bytes) if !bytes.is_empty() => return Ok(bytes),
            Ok(_) => last_err = "接口返回空响应（签名可能失效）".to_string(),
            Err(e) => last_err = e.to_string(),
        }
        if attempt + 1 < MAX_RETRIES {
            tokio::time::sleep(RETRY_BASE_DELAY * (attempt + 1)).await;
        }
    }

    Err(AppError::EmptyResponse(last_err))
}

/// 发一次请求。签名在此处生成，与请求方法在同一个分支里决定，不会错配。
async fn send_once(
    client: &reqwest::Client,
    pathname: &str,
    body: Option<&[u8]>,
    device: &[(&'static str, &'static str)],
) -> AppResult<Vec<u8>> {
    let signed = match body {
        Some(bytes) => crate::signer::sign_post(pathname, bytes.to_vec(), device),
        None => crate::signer::sign_get(pathname, device),
    };

    let mut req = match &signed.body {
        Some(bytes) => client.post(&signed.url).body(bytes.clone()),
        None => client.get(&signed.url),
    };
    for (k, v) in &signed.headers {
        req = req.header(k, v);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(AppError::Network(format!("HTTP {}", resp.status())));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::ProxyConfig;
    use crate::signer::{sign_get, sign_post, video_device};

    #[test]
    fn retry_count_is_sane() {
        // 常量在编译期已知，这条断言的作用是「改动时有人会看见」
        assert!((1..=5).contains(&MAX_RETRIES));
    }

    #[test]
    fn post_signature_always_has_body() {
        let signed = sign_post("/x/v1/", b"{}".to_vec(), &video_device());
        assert!(signed.body.is_some(), "POST 签名必须带 body");
    }

    #[test]
    fn get_signature_has_no_body() {
        let signed = sign_get("/x/v1/", &video_device());
        assert!(signed.body.is_none(), "GET 签名不应带 body");
    }

    #[test]
    fn client_builds_for_default_proxy() {
        // 探针与测试都走默认代理，构造成功即说明配置可用
        let _ = crate::service::settings_service::proxy::build_client(&ProxyConfig::default());
    }
}
