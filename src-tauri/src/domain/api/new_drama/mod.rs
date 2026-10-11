//! 新剧推荐接口（2026-10-03 mitmproxy 抓包锁定，详见 docs/hongguo-api-endpoints.md）。
//!
//! `GET .../cell/change/v1/`（**v1/ 带斜杠**，与排行榜不同路径），
//! `selected_items=firstonlinetime_new, cell_gender`；响应形态与排行榜相同
//! （`data.cell_view.cell_data[].video_data[]`），条目与筛选面板解析复用
//! rank 域的 `parse_rank_items` / `parse_cell_selector`。

use serde_json::Value;

use super::client::{ApiEnv, api_call_reading};
use super::danmaku::LQ_API_ORIGIN;
use super::discover::check_code;
use super::rank::{RankPage, parse_cell_selector, parse_rank_items};
use crate::error::{AppError, AppResult};

/// 新剧推荐（v1/ 带斜杠——两条路径后缀不一致是服务端原状，抓包实锤）。
pub const NEW_DRAMA_CELL_PATH: &str = "/reading/bookapi/bookmall/cell/change/v1/";

/// 新剧推荐页（`cell_gender`：2=全部，其余见抓包；分页 offset/limit）。
/// 新剧推荐页（`cell_gender`：2=全部，其余见抓包；分页 offset/limit）。
///
/// 翻页必须回传上一页响应的 `session_id`（2026-10-11 抓 hgplayer 1.1.8
/// 实流：第二页起 session_id 每页都在参；首页不传）。
pub async fn fetch_new_drama(
    gender: i64,
    offset: i64,
    session_id: Option<&str>,
    env: &ApiEnv,
) -> AppResult<RankPage> {
    let mut q: Vec<(String, String)> = [
        ("cell_id", "7431550523368554558"),
        ("selected_items", "firstonlinetime_new"),
        ("cell_gender", gender.to_string().as_str()),
        ("change_type", "1"),
        ("client_req_type", "1"),
        ("limit", "18"),
        ("offset", offset.to_string().as_str()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    if let Some(sid) = session_id.filter(|s| !s.is_empty()) {
        q.push(("session_id".to_string(), sid.to_string()));
    }
    let bytes = api_call_reading(LQ_API_ORIGIN, NEW_DRAMA_CELL_PATH, None, &q, env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析新剧失败: {e}")))?;
    check_code(&value)?;
    let data = value.get("data");
    Ok(RankPage {
        items: parse_rank_items(data)?,
        tabs: parse_cell_selector(data),
        has_more: data
            .and_then(|d| d.get("has_more"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_offset: data
            .and_then(|d| d.get("next_offset"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        session_id: data
            .and_then(|d| d.get("session_id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

#[cfg(test)]
pub(crate) mod probe {
    use super::*;
    use crate::domain::api::rank::probe::{anon_env, hg_env};

    /// 新剧推荐直连。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_new_drama() {
        let env = hg_env(&anon_env());
        let page = fetch_new_drama(2, 0, None, &env).await.expect("新剧推荐");
        println!(
            "[new-drama] {} 条, #1={:?}",
            page.items.len(),
            page.items.first().map(|i| &i.title)
        );
        assert!(!page.items.is_empty());
    }
}
