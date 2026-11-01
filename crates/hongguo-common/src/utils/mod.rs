//! 无业务语义的纯工具层。
//!
//! 准入标准：不依赖 AppState/领域类型、纯函数、可单测。各域重复实现
//! 自这里收敛——同一份逻辑只在本层存一份，域内旧副本一律删除。

pub mod hex;
pub mod json;
pub mod path;
pub mod time;
pub mod url;
// TLS 证书来源的平台分叉只在 Android 有消费者（桌面走 reqwest 默认的
// platform-verifier），模块与依赖一起按目标门控，桌面构建零感知。
#[cfg(target_os = "android")]
pub mod tls;
