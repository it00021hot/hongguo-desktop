//! 追更的本地领域模型（更新提醒通知）。
//!
//! 追更列表本身是服务端接口的传输结构（`hongguo_api::interact::FollowUpdateItem`），不落库；这里只有更新检查的产物——通知（落 `follow_notice` 表，走 IPC 给前端）。

use serde::{Deserialize, Serialize};

/// 通知 kind：追更更新。预约上线（"online"）留待预约提醒接入时启用。
pub const NOTICE_KIND_FOLLOW: &str = "follow";
pub const NOTICE_KIND_ONLINE: &str = "online";

/// 一条更新提醒（追的剧出新集 / 预约的剧上线）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowNotice {
    pub id: i64,
    /// `follow` 追更更新 / `online` 预约上线（前端按 kind 显示标签）
    pub kind: String,
    pub series_id: String,
    /// 剧名
    pub title: String,
    /// 通知正文（如「追的剧更新了，新增 2 集（共 216 集）」）
    pub detail: String,
    /// 产生时间（unix 秒）
    pub at: i64,
    #[serde(default)]
    pub read: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goes_out_as_camel_case() {
        let n = FollowNotice {
            id: 1,
            kind: NOTICE_KIND_FOLLOW.into(),
            series_id: "7677818497497320472".into(),
            title: "摆摊大王".into(),
            detail: "追的剧更新了，新增 2 集（共 216 集）".into(),
            at: 1_792_041_548,
            read: false,
        };
        let v = serde_json::to_value(&n).unwrap();
        assert_eq!(v["seriesId"], "7677818497497320472");
        assert_eq!(v["kind"], "follow");
        assert!(v.get("series_id").is_none());
    }
}
