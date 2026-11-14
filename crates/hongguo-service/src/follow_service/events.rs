//! 追更域的事件名与载荷（service → 壳转发 → 前端，契约三处同改）。

use hongguo_common::model::FollowNotice;

/// 新增的通知（检查定时器 / 手动检查出新集时发布；前端弹提示 + 刷新角标）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowNoticesEvent {
    pub notices: Vec<FollowNotice>,
}
