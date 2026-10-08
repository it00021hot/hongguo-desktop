//! 网络代理。
//!
//! 三种模式：跟随系统环境变量 / 手动指定 / 强制直连。
//! 保存设置后立即生效——`build_client` 每次按当前配置构造 client，
//! API 解析、视频下载、图片加载统一走它，无需重启。
//!
//! client 的构造本身归网络层（[`crate::domain::api::client`]），
//! 这里只管「代理配得好不好、连不连得通」。

use std::time::Instant;

use crate::domain::api::client::build_client;
use crate::domain::model::{ProxyConfig, ProxyTestResult};
use crate::error::AppResult;

/// 测试代理连通性，返回耗时。
pub async fn test_proxy(draft: &ProxyConfig) -> AppResult<ProxyTestResult> {
    let client = build_client(draft)?;
    let started = Instant::now();

    // 拿一个稳定的探测地址，只关心能否连通与耗时
    let result = client
        .head("https://api5-normal-sinfonlineb.fqnovel.com/")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await;

    let elapsed_ms = started.elapsed().as_millis();
    match result {
        // 收到任何 HTTP 状态码都算连通：探测地址是 API 主域的裸根，CDN 在
        // 根路径常态回 404——「有响应」本身就证明 DNS/TCP/TLS/代理全通。
        // 按 2xx 判定会把可用的代理误报成 404 失败（2026-10-09 用户实测）。
        Ok(resp) => Ok(ProxyTestResult {
            ok: true,
            elapsed_ms,
            message: format!("HTTP {}，连通", resp.status().as_u16()),
        }),
        Err(e) => Ok(ProxyTestResult {
            ok: false,
            elapsed_ms,
            message: e.to_string(),
        }),
    }
}
