//! 网络代理：跟随系统环境变量 / 手动指定 / 强制直连三种模式，保存后立即生效
//!（`build_client` 每次按当前配置构造 client，无需重启）。client 构造归网络层
//!（[`hongguo_api::client`]），这里只管「代理配得好不好、连不连得通」。

use std::time::Instant;

use hongguo_api::client::build_client;
use hongguo_common::AppResult;
use hongguo_common::model::{ProxyConfig, ProxyTestResult};

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
        // 收到任何 HTTP 状态码都算连通：探测地址在根路径常态回 404，
        // 「有响应」本身就证明全通；按 2xx 判定会把可用代理误报成失败。
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
