//! 弹幕：commentapi 的 comment/list（comment_type=20）。
//!
//! 实测口径（2026-10 抓包锁定）：
//! - `POST {LQ_API_ORIGIN}/novel/commentapi/comment/list/{group_id}/v1/`，
//!   **group_id（=分集 vid）在路径里**；
//! - 参数在 body **顶层**（不包 biz_param），抓包形状见 [`danmaku_payload`]；
//! - 自定义头 `x-reading-request: {ticket_ms}-{random}`；
//! - 分页靠 `cursor`（`{"start_offset_time":ms,"end_offset_time":ms}` 30 秒窗口），
//!   一集要多拉几窗才能拿全，本模块代为循环（上限见 [`MAX_WINDOWS`]）；
//! - 弹幕的**时间轴**在 `comment.expand.offset_time`（毫秒，与 cursor 同单位）。
//!
//! 发送（comment/add）需要登录态，登录里程碑落地后再接。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::client::ApiEnv;
use crate::error::{AppError, AppResult};

/// hgplayer 实测的 reading 系 API 主机（我们的视频接口走 sinfonlineb，
/// commentapi 只在这个主机上存在）。
pub const LQ_API_ORIGIN: &str = "https://api5-normal-lq.fqnovel.com";

/// 一集最多拉多少个 30 秒窗口（2 小时封顶，防服务端 cursor 异常死循环）。
const MAX_WINDOWS: usize = 240;

/// 一条弹幕。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Danmaku {
    pub comment_id: String,
    pub text: String,
    /// 出现时间（毫秒，视频内时间轴）
    #[serde(default)]
    pub offset_ms: u64,
    #[serde(default)]
    pub digg_count: i64,
}

/// 拉一集的**全部**弹幕（内部按 cursor 窗口循环到 has_more=false）。
///
/// `group_id` 是分集 vid，`book_id` 是 series_id——两个都来自剧集档案。
pub async fn fetch_danmaku_all(
    group_id: &str,
    book_id: &str,
    env: &ApiEnv,
) -> AppResult<Vec<Danmaku>> {
    let mut all: Vec<Danmaku> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut cursor = String::new();
    for _ in 0..MAX_WINDOWS {
        let page = fetch_danmaku_window(group_id, book_id, &cursor, env).await?;
        let has_more = page.has_more;
        cursor = page.next_cursor;
        for d in page.items {
            if seen.insert(d.comment_id.clone()) {
                all.push(d);
            }
        }
        if !has_more {
            break;
        }
    }
    all.sort_by_key(|d| d.offset_ms);
    Ok(all)
}

/// 一窗弹幕。
struct DanmakuWindow {
    items: Vec<Danmaku>,
    has_more: bool,
    next_cursor: String,
}

/// 抓包形状的请求体。
fn danmaku_payload(group_id: &str, book_id: &str, cursor: &str) -> Value {
    serde_json::json!({
        "aid": 8662,
        "business_param": {
            "book_id": book_id,
            "need_danmaku_guide_type": [],
            "playlet_item_duration": 60000,
            "start_offset_time": 0,
        },
        "comment_source": 601,
        "comment_type": 20,
        "compliance_status": 0,
        "count": 90,
        "cursor": cursor,
        "group_id": group_id,
        "group_type": 30,
        "server_channel": 1000,
        "sort": 1,
    })
}

async fn fetch_danmaku_window(
    group_id: &str,
    book_id: &str,
    cursor: &str,
    env: &ApiEnv,
) -> AppResult<DanmakuWindow> {
    let path = format!("/novel/commentapi/comment/list/{group_id}/v1/");
    let body = serde_json::to_vec(&danmaku_payload(group_id, book_id, cursor))
        .map_err(|e| AppError::Signer(e.to_string()))?;
    // reading 系统一入口：轻签名头 + gzip body（1.1.3 抓包形态）
    let bytes = super::client::api_call_reading(LQ_API_ORIGIN, &path, Some(body), &[], env).await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析弹幕响应失败: {e}")))?;
    if v.get("code").and_then(Value::as_i64) != Some(0) {
        let msg = v.get("message").and_then(Value::as_str).unwrap_or("?");
        return Err(AppError::Media(format!(
            "弹幕接口返回 {}: {msg}",
            v.get("code").and_then(Value::as_i64).unwrap_or(-1)
        )));
    }
    let list_info = v.pointer("/data/common_list_info").cloned().unwrap_or(Value::Null);
    let mut items = Vec::new();
    for entry in v
        .pointer("/data/data_list")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let Some(comment) = entry.get("comment") else { continue };
        let comment_id = comment
            .get("comment_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let text = comment
            .pointer("/common/content/text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if comment_id.is_empty() || text.is_empty() {
            continue;
        }
        items.push(Danmaku {
            comment_id,
            text,
            offset_ms: comment
                .pointer("/expand/offset_time")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                .max(0) as u64,
            digg_count: comment
                .pointer("/stat/digg_count")
                .and_then(Value::as_i64)
                .unwrap_or(0),
        });
    }
    Ok(DanmakuWindow {
        has_more: list_info
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_cursor: list_info
            .get("cursor")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_shape_matches_capture() {
        let p = danmaku_payload("7690197301075119166", "7690150906532219966", "");
        // 抓包锚点字段：任何一个改形都会 100103
        assert_eq!(p["comment_type"], 20, "20 = 弹幕");
        assert_eq!(p["comment_source"], 601);
        assert_eq!(p["group_type"], 30);
        assert_eq!(p["count"], 90);
        assert_eq!(p["aid"], 8662);
        assert_eq!(p["group_id"], "7690197301075119166");
        assert_eq!(p["business_param"]["book_id"], "7690150906532219966");
        assert_eq!(p["cursor"], "");
    }

    #[test]
    fn window_cursor_passthrough() {
        let p = danmaku_payload("g", "b", r#"{"start_offset_time":30000,"end_offset_time":60000}"#);
        assert!(p["cursor"].as_str().unwrap().contains("end_offset_time"));
    }
}

#[cfg(test)]
mod probe {
    use super::*;

    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_danmaku_full() {
        let env = ApiEnv::anonymous(crate::domain::model::ProxyConfig::default());
        let all = fetch_danmaku_all("7690197301075119166", "7690150906532219966", &env)
            .await
            .expect("整集弹幕");
        println!("[danmaku-full] {} 条，前 5: {:?}", all.len(), &all[..all.len().min(5)]);
        assert!(!all.is_empty(), "这一集实测有弹幕");
        // 时间轴升序（fetch_danmaku_all 已排序）
        let mut sorted = all.clone();
        sorted.sort_by_key(|d| d.offset_ms);
        assert_eq!(all, sorted);
    }
}
