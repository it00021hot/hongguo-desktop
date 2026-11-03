//! 持久化：Turso 内嵌数据库。[`db`] 异步核心（连接、schema 版本迁移、事务收口）、[`entity`] 八个模块（device / follow / local_history / merge / playback / series / settings / task）的 SQL 实现、[`bridge`] 同步门面 [`Store`]（专职 DB 线程串行执行 SQL）、[`paths`] 数据目录解析。
//!
//! 本 crate 不依赖 tauri：「下载队列快照写回库」这类编排留给上层（见 `service::download_service::persist_tasks`），这里只提供 [`Store`] 的存储原语。

pub mod bridge;
pub mod db;
pub mod entity;
pub mod paths;

pub use bridge::Store;
