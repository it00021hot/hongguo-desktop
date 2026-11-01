//! TLS 证书来源的平台分叉（仅 Android 使用）。
//!
//! reqwest 0.13 的 `rustls` feature 默认走 rustls-platform-verifier：
//! 桌面（schannel / security-framework）直接可用；Android 的验证器要经
//! JNI 调系统证书链路，且必须先做一次性初始化（`android::init_with_env`
//! + 应用内嵌 Kotlin 组件）——缺初始化时全部 TLS 握手失败，表象是
//! 「应用像没有网络」（2026-11 Android APK 实测）。
//!
//! Android 端改用编译内置的 webpki 根证书走纯 Rust 验证路径：无 JNI、
//! 无初始化，覆盖全部公共 CA（官方接口与图床均为公共 CA 签发）。桌面端
//! 不经过本模块，维持 reqwest 默认行为不变。

use std::sync::Arc;

use crate::{AppError, AppResult};

/// 构造仅信任内置 webpki 根证书的 rustls 客户端配置。
///
/// 交给 `reqwest::ClientBuilder::use_preconfigured_tls` 使用；TLS 层由此
/// 全权接管（不再叠加 reqwest 的证书/ALPN 配置），HTTP 版本偏好由
/// builder 侧自行维持（本项目 API 链路固定 HTTP/1.1，无 ALPN 需求）。
pub fn webpki_tls_config() -> AppResult<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| AppError::Network(format!("TLS 协议版本配置失败: {e}")))?
        .with_root_certificates(roots)
        .with_no_client_auth())
}
