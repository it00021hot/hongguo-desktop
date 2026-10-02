//! 启动装配：顺序即依赖顺序。
//!
//! 先加载数据 → 再注册协议与嗅探窗口 → 最后恢复未完成的下载。

pub mod downloader;
pub mod sniff;
pub mod store;
pub mod transcoder;
