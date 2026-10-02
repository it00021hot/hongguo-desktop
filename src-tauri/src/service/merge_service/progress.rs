//! 合并进度的对外回调。
//!
//! 合并是重活（兼容合并要逐集转码），调用方需要一个能报告「第几集 / 共几集」
//! 的口子。这里用 `Arc<dyn Fn>` 而非泛型回调：合并函数是同步的，
//! 但事件发射闭包会被 `merge_cmd` 捕获 `AppHandle`，用泛型会把闭包类型
//! 传染到两个 merge 实现的签名上。

use crate::domain::model::MergeTask;

/// 一集完成时上报一次。
pub type ProgressSink = std::sync::Arc<dyn Fn(usize, usize, &MergeTask) + Send + Sync>;
