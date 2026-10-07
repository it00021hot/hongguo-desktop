//! 合并进度的对外回调。
//!
//! 合并是重活（兼容合并要逐集转码），调用方需要一个能报告「第几集 / 共几集」
//! 的口子。这里用 `Arc<dyn Fn>` 而非泛型回调：合并函数是同步的，
//! 但事件发射闭包会被 `merge_cmd` 捕获 `AppHandle`，用泛型会把闭包类型
//! 传染到两个 merge 实现的签名上。

use crate::domain::model::MergeTask;

/// 进度跨过步进时上报。
///
/// `done` 是整体「已完成集数」的**小数口径**：集内实时进度折进小数部分
/// （1.35 = 第 2 集转了 35%），上层用它算百分比。`total` 是参与合并的集数——
/// 任务登记时还数不出来（要等合并内部盘点输入），只有上报里带得出来，
/// 上层顺手拿它填 `episode_count`。
pub type ProgressSink = std::sync::Arc<dyn Fn(f64, usize, &MergeTask) + Send + Sync>;
