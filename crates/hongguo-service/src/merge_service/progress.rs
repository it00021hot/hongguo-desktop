//! 合并进度的对外回调。合并是重活（兼容合并要逐集转码），调用方需要能报告
//! 「第几集 / 共几集」的口子；用 `Arc<dyn Fn>` 而非泛型回调，避免闭包类型
//! 传染到两个 merge 实现的签名上。

use hongguo_common::model::MergeTask;

/// 进度跨过步进时上报。`done` 是整体「已完成集数」的小数口径（1.35 = 第 2 集转了 35%），
/// `total` 是参与合并的集数（任务登记时数不出来，只有上报里带得出来）。
pub type ProgressSink = std::sync::Arc<dyn Fn(f64, usize, &MergeTask) + Send + Sync>;
