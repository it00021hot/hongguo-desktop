//! 启动装配：顺序即依赖顺序。
//!
//! 先加载数据 → 磁盘扫描补登记 → 设备生命周期复检 → 把待跑任务推入调度
//! → 最后探测转码能力。

pub mod device;
pub mod downloader;
pub mod rescan;
pub mod store;
pub mod transcoder;
