//! 持久化（hongguo-store crate）：Turso 内嵌数据库。
//!
//! 分层：
//! - [`db`]：异步核心——连接、schema 版本迁移、事务收口；
//! - [`entity`]：六组实体的 SQL 实现（设置/任务/剧集/播放进度/合并 + 设备档案与元数据）；
//! - [`bridge`]：同步门面 [`Store`]——专职 DB 线程串行执行 SQL，
//!   业务代码保持同步调用习惯；
//! - [`paths`]：数据目录解析。
//!
//! 相对旧方案解决的问题：单 JSON 全量重写（播放 5 秒一整写）、
//! clone-后-save 的丢更新窗口、Windows 非原子 rename 的崩溃窗口，
//! 都在本层消除（旧 data.json 的一次性导入链路已移除，
//! 存量库用户无感，从未迁过的远古档不再被读取）。
//!
//! 本 crate 不依赖 tauri：把「下载队列快照写回库」这类编排留给上层
//! （见 `service::download_service::persist_tasks`），这里只提供
//! [`Store`] 的存储原语。

pub mod bridge;
pub mod db;
pub mod entity;
pub mod paths;

pub use bridge::Store;
