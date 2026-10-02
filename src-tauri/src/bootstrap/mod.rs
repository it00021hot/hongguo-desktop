//! 启动装配：顺序即依赖顺序。
//!
//! 先加载数据 → 再把待跑任务推入调度 → 最后探测转码能力。

pub mod downloader;
pub mod store;
pub mod transcoder;
