//! 剧集档案服务。
//!
//! 只负责「链接 / series_id → 分集解析」与「档案登记」两件事。
//! 解析逻辑在 [`resolver`]，持久化在 [`registry`]。

pub mod registry;
pub mod resolver;
