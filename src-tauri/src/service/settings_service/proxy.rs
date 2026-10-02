//! 网络代理。
//!
//! 三种模式：跟随系统环境变量 / 手动指定 / 强制直连。
//! 保存设置后立即生效——`build_client` 每次按当前配置构造 client，
//! API 解析、视频下载、图片加载统一走它，无需重启。

use std::time::Instant;

use reqwest::Proxy;

use crate::domain::model::{ProxyConfig, ProxyMode, ProxyTestResult};
use crate::error::{AppError, AppResult};

/// 按配置构造 HTTP client。
pub fn build_client(config: &ProxyConfig) -> AppResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .user_agent(crate::signer::VIDEO_UA)
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10));

    match config.resolved() {
        Some(url) => {
            let proxy =
                Proxy::all(&url).map_err(|e| AppError::Network(format!("代理地址无效: {e}")))?;
            builder = builder.proxy(proxy);
        }
        None => {
            builder = builder.no_proxy();
        }
    }

    builder
        .build()
        .map_err(|e| AppError::Network(format!("构造 HTTP client 失败: {e}")))
}

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
        Ok(resp) => Ok(ProxyTestResult {
            ok: resp.status().is_success() || resp.status().is_redirection(),
            elapsed_ms,
            message: format!("HTTP {}", resp.status().as_u16()),
        }),
        Err(e) => Ok(ProxyTestResult {
            ok: false,
            elapsed_ms,
            message: e.to_string(),
        }),
    }
}

/// 代理状态描述，供设置页展示。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProxyStatus {
    pub mode: ProxyMode,
    /// 实际生效的代理地址
    pub effective: String,
    /// 展示用标签
    pub label: String,
}

/// 取当前代理状态。
pub fn status(config: &ProxyConfig) -> ProxyStatus {
    let effective = config.resolved().unwrap_or_else(|| "直连".to_string());
    let label = match config.mode {
        ProxyMode::System => "跟随系统".to_string(),
        ProxyMode::Manual => "手动指定".to_string(),
        ProxyMode::Direct => "强制直连".to_string(),
    };
    ProxyStatus {
        mode: config.mode,
        effective,
        label,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_mode_builds_no_proxy_client() {
        // 构造成功即说明直连配置被接受
        let _c = build_client(&ProxyConfig {
            mode: ProxyMode::Direct,
            url: "http://127.0.0.1:7890".into(),
        })
        .unwrap();
    }

    #[test]
    fn manual_mode_builds_client() {
        let c = build_client(&ProxyConfig {
            mode: ProxyMode::Manual,
            url: "http://127.0.0.1:7890".into(),
        })
        .unwrap();
        let _ = c;
    }

    #[test]
    fn invalid_proxy_url_errors() {
        let err = build_client(&ProxyConfig {
            mode: ProxyMode::Manual,
            url: "not a valid url".into(),
        });
        assert!(err.is_err(), "非法代理地址应报错而不是静默忽略");
    }

    #[test]
    fn status_reports_effective() {
        let s = status(&ProxyConfig {
            mode: ProxyMode::Manual,
            url: "http://127.0.0.1:1080".into(),
        });
        assert_eq!(s.effective, "http://127.0.0.1:1080");
        assert_eq!(s.label, "手动指定");
    }

    #[test]
    fn direct_status_says_直连() {
        let s = status(&ProxyConfig {
            mode: ProxyMode::Direct,
            url: String::new(),
        });
        assert_eq!(s.effective, "直连");
    }
}
