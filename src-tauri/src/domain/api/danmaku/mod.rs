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

mod model;

pub use model::{CommentItem, CommentPage, Danmaku, SeriesReviewPage};

use serde_json::Value;

use super::client::ApiEnv;
use crate::error::{AppError, AppResult};

/// hgplayer 实测的 reading 系 API 主机（我们的视频接口走 sinfonlineb，
/// commentapi 只在这个主机上存在）。
pub const LQ_API_ORIGIN: &str = "https://api5-normal-lq.fqnovel.com";

/// 一集最多拉多少个 30 秒窗口（2 小时封顶，防服务端 cursor 异常死循环）。
const MAX_WINDOWS: usize = 240;

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

/// 评论区请求体（与弹幕同端点不同形态）。
///
/// 2026-10-06 抓 hgplayer 1.1.5 重锁：评论区形态与弹幕形态在服务端是
/// **两套校验**——沿用 1.1.3 的弹幕形 business_param + server_channel=1000
/// 会被拒（实测 103001 参数错误）；评论必须用 `need_count/req_type` +
/// `server_channel=18`，且 `need_count: true` 让响应带上评论总数
/// （`common_list_info.total`，互动栏评论计数的数据源）。
fn comments_payload(group_id: &str, book_id: &str, cursor: &str) -> Value {
    serde_json::json!({
        "aid": 8662,
        "business_param": {
            "book_id": book_id,
            "need_count": true,
            "req_type": 0,
        },
        "comment_source": 4,
        "comment_type": 4,
        "compliance_status": 0,
        "count": 20,
        "cursor": cursor,
        "group_id": group_id,
        "group_type": 30,
        "server_channel": 18,
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
    let list_info = v
        .pointer("/data/common_list_info")
        .cloned()
        .unwrap_or(Value::Null);
    let mut items = Vec::new();
    for entry in v
        .pointer("/data/data_list")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let Some(comment) = entry.get("comment") else {
            continue;
        };
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

/// 拉评论区的一页（cursor 传上一页返回的 next_cursor，首页传空）。
pub async fn fetch_comments_page(
    group_id: &str,
    book_id: &str,
    cursor: &str,
    env: &ApiEnv,
) -> AppResult<CommentPage> {
    let path = format!("/novel/commentapi/comment/list/{group_id}/v1/");
    let body = serde_json::to_vec(&comments_payload(group_id, book_id, cursor))
        .map_err(|e| AppError::Signer(e.to_string()))?;
    let bytes = super::client::api_call_reading(LQ_API_ORIGIN, &path, Some(body), &[], env).await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析评论响应失败: {e}")))?;
    check_comment_code(&v)?;
    parse_comment_page(&v)
}

/// 剧级评论（详情页「剧评」，整部剧一条线）。
///
/// 2026-10-07 抓 hgplayer 详情页锁定：同一端点换**形态**——`group_id` 是
/// **series_id**（不是 vid）、`group_type=1`（书/剧维度）、`comment_source=1`、
/// `comment_type=2`、`server_channel=34`；business_param 是另一组全量字段
/// （照抄抓包，审计纪律）。响应与单集评论同构（common_list_info.total /
/// data_list），解析复用。
fn series_comments_payload(group_id: &str, cursor: &str) -> Value {
    serde_json::json!({
        "aid": 8662,
        "business_param": {
            "book_id": group_id,
            "comment_sort_debug": false,
            "end_offset_time": 0,
            "hit_interaction_data_migration_ab": false,
            "is_last_episode": false,
            "item_count": 0,
            "max_item_count": 0,
            "need_count": true,
            "need_danmaku_occlusion_face_data": false,
            "outflow_comment_count": 0,
            "para_index": 0,
            "playlet_consume_duration_ms": 0,
            "playlet_item_consume_duration_ms": 0,
            "playlet_item_duration": 0,
            "read_item_count": 0,
            "req_type": 0,
            "start_offset_time": 0,
        },
        "comment_source": 1,
        "comment_type": 2,
        "count": 10,
        "cursor": cursor,
        "group_id": group_id,
        "group_type": 1,
        "server_channel": 34,
        "sort": 1,
    })
}

/// 拉剧级评论的一页（详情页「剧评」tab 数据源 + 头部评分）。
pub async fn fetch_series_comments_page(
    series_id: &str,
    cursor: &str,
    env: &ApiEnv,
) -> AppResult<SeriesReviewPage> {
    let path = format!("/novel/commentapi/comment/list/{series_id}/v1/");
    let body = serde_json::to_vec(&series_comments_payload(series_id, cursor))
        .map_err(|e| AppError::Signer(e.to_string()))?;
    let bytes = super::client::api_call_reading(LQ_API_ORIGIN, &path, Some(body), &[], env).await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析剧评响应失败: {e}")))?;
    check_comment_code(&v)?;
    let page = parse_comment_page(&v)?;
    // credibility_score 线上是**数字**形态（8.3），字符串/数字两种都兜
    let score = v
        .pointer("/data/extra/credibility_score")
        .map(|x| match x {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default();
    let score_cnt = v
        .pointer("/data/extra/credibility_score_count")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let tags = v
        .pointer("/data/extra/book_info/tags")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect();
    Ok(SeriesReviewPage {
        page,
        score,
        score_cnt,
        tags,
    })
}

/// 发剧评（详情页「剧评」tab 的评论框）。
///
/// 端点同单集评论（comment/add），但组维度照剧评拉取形态换：group_id=
/// **series_id**、group_type=1、comment_source=1、comment_type=2、
/// server_channel=34（与 [`series_comments_payload`] 同一维度，2026-10-07
/// 抓包字段）；business_param 照评论形态（data_type=4）全量字段。
/// 返回服务端分配的 comment_id。
pub async fn send_series_review(series_id: &str, text: &str, env: &ApiEnv) -> AppResult<String> {
    let payload = serde_json::json!({
        "aid": 8662,
        "business_param": {
            "book_id": series_id,
            "has_aigc_content": false,
            "ignore_urge_rule": false,
            "log_extra": {},
            "offset": 0,
            "preset_text_id": "",
            "shark_param": super::interact::shark_param(),
            "text_feature": {},
        },
        "comment_source": 1,
        "comment_type": 2,
        "commit_source": 9,
        "data_type": 4,
        "group_id": series_id,
        "group_type": 1,
        "server_channel": 34,
        "text": text,
    });
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造剧评请求失败: {e}")))?;
    let bytes = super::client::api_call_reading(
        LQ_API_ORIGIN,
        super::interact::COMMENT_ADD_PATH,
        Some(raw),
        &[],
        env,
    )
    .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析剧评响应失败: {e}")))?;
    check_comment_code(&value)?;
    Ok(value
        .pointer("/data/comment_info/comment_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string())
}

fn check_comment_code(v: &Value) -> AppResult<()> {
    if v.get("code").and_then(Value::as_i64) != Some(0) {
        let msg = v.get("message").and_then(Value::as_str).unwrap_or("?");
        return Err(AppError::Media(format!(
            "评论接口返回 {}: {msg}",
            v.get("code").and_then(Value::as_i64).unwrap_or(-1)
        )));
    }
    Ok(())
}

/// 评论响应 → [`CommentPage`]（单集评论与剧级评论同构，共用）。
fn parse_comment_page(v: &Value) -> AppResult<CommentPage> {
    let list_info = v
        .pointer("/data/common_list_info")
        .cloned()
        .unwrap_or(Value::Null);
    let mut page = CommentPage {
        total: list_info.get("total").and_then(Value::as_i64).unwrap_or(0),
        has_more: list_info
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_cursor: list_info
            .get("cursor")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        items: Vec::new(),
    };
    for entry in v
        .pointer("/data/data_list")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let Some(comment) = entry.get("comment") else {
            continue;
        };
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
        let base = comment.pointer("/common/user_info/base_info");
        page.items.push(CommentItem {
            comment_id,
            user_name: base
                .and_then(|b| b.get("user_name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            avatar: base
                .and_then(|b| b.get("expand_user_avatar"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            text,
            create_time: comment
                .pointer("/common/create_timestamp")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            digg_count: comment
                .pointer("/stat/digg_count")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            reply_count: comment
                .pointer("/stat/reply_count")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            user_digg: comment
                .pointer("/user_action/user_digg")
                .and_then(Value::as_bool)
                == Some(true),
        });
    }
    Ok(page)
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
        let p = danmaku_payload(
            "g",
            "b",
            r#"{"start_offset_time":30000,"end_offset_time":60000}"#,
        );
        assert!(p["cursor"].as_str().unwrap().contains("end_offset_time"));
    }

    #[test]
    fn comments_payload_matches_capture_20261006() {
        // 2026-10-06 抓 hgplayer 1.1.5：评论形态与弹幕形态分叉，
        // 沿用弹幕形 business_param / server_channel=1000 会被 103001 拒
        let p = comments_payload("g", "b", "");
        assert_eq!(p["comment_type"], 4);
        assert_eq!(p["comment_source"], 4);
        assert_eq!(p["server_channel"], 18, "评论走 18，弹幕才是 1000");
        assert_eq!(p["business_param"]["need_count"], true);
        assert_eq!(p["business_param"]["req_type"], 0);
        assert_eq!(p["count"], 20);
        assert!(
            p["business_param"].get("playlet_item_duration").is_none(),
            "弹幕形字段混进评论体会 103001"
        );
    }

    #[test]
    fn series_comments_payload_matches_capture_20261007() {
        // 2026-10-07 抓 hgplayer 详情页：剧级评论换形态——group_id 是
        // series_id、group_type=1、source=1/type=2/channel=34，
        // business_param 是全量字段组（照抄抓包）
        let p = series_comments_payload("7687941387362257982", "");
        assert_eq!(p["group_type"], 1, "1 = 书/剧维度，30 是单集");
        assert_eq!(p["comment_source"], 1);
        assert_eq!(p["comment_type"], 2);
        assert_eq!(p["server_channel"], 34);
        assert_eq!(p["group_id"], "7687941387362257982");
        assert_eq!(p["business_param"]["book_id"], "7687941387362257982");
        assert_eq!(p["business_param"]["need_count"], true);
        assert_eq!(p["count"], 10);
    }
}

#[cfg(test)]
mod probe {
    use super::*;

    /// 拉一集的**全部**评论区（按 cursor 翻页到 has_more=false，上限见 [`MAX_WINDOWS`]）。
    ///
    /// 热门集会拉几百页，command 层已改用 [`fetch_comments_page`] 按页返回；
    /// 本函数只服务 probe 全量对账，与生产代码无关。
    async fn fetch_comments_all(
        group_id: &str,
        book_id: &str,
        env: &ApiEnv,
    ) -> AppResult<CommentPage> {
        let mut all = CommentPage::default();
        let mut seen = std::collections::HashSet::new();
        let mut cursor = String::new();
        for _ in 0..MAX_WINDOWS {
            let page = fetch_comments_page(group_id, book_id, &cursor, env).await?;
            if all.total == 0 {
                all.total = page.total;
            }
            all.has_more = page.has_more;
            all.next_cursor = page.next_cursor.clone();
            for item in page.items {
                if seen.insert(item.comment_id.clone()) {
                    all.items.push(item);
                }
            }
            if !all.has_more || all.next_cursor.is_empty() {
                break;
            }
            cursor = all.next_cursor.clone();
        }
        Ok(all)
    }

    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_danmaku_full() {
        let env = ApiEnv::anonymous(crate::domain::model::ProxyConfig::default());
        let all = fetch_danmaku_all("7690197301075119166", "7690150906532219966", &env)
            .await
            .expect("整集弹幕");
        println!(
            "[danmaku-full] {} 条，前 5: {:?}",
            all.len(),
            &all[..all.len().min(5)]
        );
        assert!(!all.is_empty(), "这一集实测有弹幕");
        // 时间轴升序（fetch_danmaku_all 已排序）
        let mut sorted = all.clone();
        sorted.sort_by_key(|d| d.offset_ms);
        assert_eq!(all, sorted);
    }

    /// 评论区（ct=4/src=4）匿名可读性探测：2026-10-06 实录旧弹幕形态被拒
    /// （103001），换 1.1.5 新形态（need_count/req_type/ch=18）后应恢复。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_comments_full() {
        let env = ApiEnv::anonymous(crate::domain::model::ProxyConfig::default());
        match fetch_comments_all("7690197301075119166", "7690150906532219966", &env).await {
            Ok(page) => {
                println!(
                    "[comments-full] total={} items={} 首3: {:?}",
                    page.total,
                    page.items.len(),
                    &page.items[..page.items.len().min(3)]
                );
            }
            Err(e) => {
                println!("[comments-full] ERR {e}");
                panic!("评论区拉取失败：{e}");
            }
        }
    }

    /// 回复列表端点探测：hgplayer 1.1.5 实操里展开回复没有独立请求被抓到，
    /// 端点未知——这里按同族 API 命名惯例穷举候选，code==0 即锁定。
    /// 锚点（2026-10-06 抓包）：group=7690957575432457241 有 2 条回复的
    /// 父评论 7693257216923468569（book=7690883800057777177）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_reply_list_candidates() {
        let env = ApiEnv::anonymous(crate::domain::model::ProxyConfig::default());
        let group = "7690957575432457241";
        let book = "7690883800057777177";
        let comment = "7693257216923468569";

        // 候选：路径已锁定 /novel/commentapi/reply/list/{评论id}/v1/（二进制
        // 字符串实证 + 404 分界），handler 按 (comment_source, comment_type)
        // 注册——对 8662 已知 source {4,601}，扫 × type 组合。
        let mut cases: Vec<(String, String, Value)> = Vec::new();
        for (p, tag, body) in [
            (
                format!("/novel/commentapi/reply/list/{group}/v1/"),
                "L group路径+body.comment_id".to_string(),
                serde_json::json!({
                    "aid": 8662,
                    "business_param": {"book_id": book, "need_count": true},
                    "comment_id": comment,
                    "comment_source": 4,
                    "comment_type": 4,
                    "count": 20,
                    "cursor": "",
                    "group_id": group,
                    "group_type": 30,
                    "server_channel": 18,
                }),
            ),
            (
                format!("/novel/commentapi/reply/list/{comment}/"),
                "M 无v1后缀".to_string(),
                serde_json::json!({
                    "aid": 8662,
                    "business_param": {"book_id": book, "need_count": true},
                    "comment_id": comment,
                    "comment_source": 4,
                    "comment_type": 4,
                    "count": 20,
                    "cursor": "",
                    "group_id": group,
                    "group_type": 30,
                    "server_channel": 18,
                }),
            ),
        ] {
            cases.push((p, tag, body));
        }
        for (path, tag, body) in cases {
            let raw = serde_json::to_vec(&body).unwrap();
            match super::super::client::api_call_reading(LQ_API_ORIGIN, &path, Some(raw), &[], &env)
                .await
            {
                Ok(bytes) => {
                    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                    let code = v.get("code").and_then(Value::as_i64).unwrap_or(-1);
                    let mut detail = String::new();
                    if code == 0 {
                        // 打首条评论的 id / 回复链路字段，确认真的是回复列表
                        match v
                            .pointer("/data/data_list/0")
                            .or_else(|| v.pointer("/data/reply_list/0"))
                        {
                            Some(entry) => {
                                detail = format!(
                                    " entry_keys={:?} comment_id={:?} reply_to={:?} text={:?}",
                                    entry
                                        .as_object()
                                        .map(|o| o.keys().cloned().collect::<Vec<_>>()),
                                    entry.pointer("/comment/comment_id").and_then(Value::as_str),
                                    entry
                                        .pointer("/comment/expand/reply_to_comment_id")
                                        .and_then(Value::as_str)
                                        .or_else(|| entry
                                            .pointer("/comment/reply_to_comment_id")
                                            .and_then(Value::as_str)),
                                    entry
                                        .pointer("/comment/common/content/text")
                                        .and_then(Value::as_str),
                                );
                            }
                            None => {
                                // 无列表条目时打印整个 data（J 这类变体可能是回复专用结构）
                                let data = v.pointer("/data").cloned().unwrap_or(Value::Null);
                                let s = serde_json::to_string(&data).unwrap_or_default();
                                detail = format!(" data无列表，原文: {}", &s[..s.len().min(600)]);
                            }
                        }
                    }
                    let debug = v
                        .pointer("/BaseResp/StatusMessage")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let debug_info = v.get("debug_info").and_then(Value::as_str).unwrap_or("");
                    println!(
                        "[reply-list/{tag}] code={code}{detail} msg={debug} debug_info={debug_info}"
                    );
                }
                Err(e) => println!("[reply-list/{tag}] ERR {e}"),
            }
        }
    }
}
