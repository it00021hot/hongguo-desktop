//! Paginated signed-in likes and post-mutation verification.
use std::collections::HashSet;

use hongguo_common::{AppError, AppResult};
use serde_json::Value;

use crate::client::{ApiEnv, LQ_API_ORIGIN, api_call_reading};

use super::InteractionItem;

const MAX_PAGES: usize = 100;

pub struct LikedVideosPage {
    pub items: Vec<InteractionItem>,
    pub next_offset: Option<i64>,
}

pub async fn fetch_page(offset: i64, env: &ApiEnv) -> AppResult<LikedVideosPage> {
    let query = [
        ("action_type".into(), "3".into()),
        ("count".into(), "100".into()),
        ("offset".into(), offset.to_string()),
        ("object_type_list".into(), "6,15,10".into()),
    ];
    let bytes = api_call_reading(
        LQ_API_ORIGIN,
        "/reading/ugc/action/mget/v",
        None,
        &query,
        env,
    )
    .await?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| invalid(&e.to_string()))?;
    super::check_interact_code(&value)?;
    parse_page(&value, offset)
}

pub async fn fetch_active_ids(env: &ApiEnv) -> AppResult<HashSet<String>> {
    let mut ids = HashSet::new();
    let mut offset = 0;
    for _ in 0..MAX_PAGES {
        let page = fetch_page(offset, env).await?;
        ids.extend(page.items.into_iter().map(|item| item.vid));
        match page.next_offset {
            Some(next) => offset = next,
            None => return Ok(ids),
        }
    }
    Err(invalid("page limit"))
}

fn invalid(reason: &str) -> AppError {
    AppError::Internal(format!("invalid liked videos response: {reason}"))
}

fn parse_page(value: &Value, offset: i64) -> AppResult<LikedVideosPage> {
    let data = value.get("data").ok_or_else(|| invalid("missing data"))?;
    let has_more = data
        .get("has_more")
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid("missing has_more"))?;
    let mut items = Vec::new();
    match data.get("mixed_data_list") {
        Some(Value::Array(rows)) => {
            if has_more && rows.is_empty() {
                return Err(invalid("empty continuation"));
            }
            for row in rows {
                if !row.is_object() {
                    return Err(invalid("invalid mixed entry"));
                }
                let Some(video) = row.get("video_data").filter(|x| !x.is_null()) else {
                    continue;
                };
                let item = super::list::parse_interaction_item(video)
                    .ok_or_else(|| invalid("missing video identifier"))?;
                video
                    .get("user_digg")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| invalid("missing liked state"))?;
                if item.user_digg {
                    items.push(item);
                }
            }
        }
        Some(Value::Null) | None
            if !has_more
                && data
                    .get("count")
                    .and_then(Value::as_i64)
                    .is_some_and(|count| count >= 0 && count <= offset) => {}
        _ => return Err(invalid("missing list")),
    }
    if !has_more {
        return Ok(LikedVideosPage {
            items,
            next_offset: None,
        });
    }
    let next = data
        .get("next_offset")
        .and_then(Value::as_i64)
        .filter(|&next| next > offset)
        .ok_or_else(|| invalid("cursor did not advance"))?;
    Ok(LikedVideosPage {
        items,
        next_offset: Some(next),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn verification_collects_only_actual_likes_and_follows_cursor() {
        let page = json!({"data":{"has_more":true,"next_offset":20,"mixed_data_list":[
            {"video_data":{"vid":"a","series_id":"s","user_digg":true,"digged_count":4,"video_detail":{"series_title":"Title"}}},
            {"video_data":{"vid":"b","user_digg":false}},
            {"comment_data": {}}
        ]}});
        let loaded = parse_page(&page, 0).unwrap();
        assert_eq!(loaded.next_offset, Some(20));
        let [item] = loaded.items.as_slice() else {
            panic!("only the active liked video is rendered")
        };
        assert_eq!(item.vid, "a");
        assert_eq!(item.series_id, "s");
        assert_eq!(item.series_title, "Title");
        assert_eq!(item.digged_count, 4);
        assert!(parse_page(&page, 20).is_err());
        let empty = json!({"data":{"count":0,"has_more":false,"mixed_data_list":null}});
        assert_eq!(parse_page(&empty, 0).unwrap().next_offset, None);
    }

    #[test]
    fn absent_or_malformed_data_cannot_confirm_removal() {
        for page in [
            json!({"data":{"count":2,"has_more":false,"mixed_data_list":null}}),
            json!({"data":{"has_more":false,"mixed_data_list":[{"video_data":{"vid":"a"}}]}}),
            json!({"data":{"mixed_data_list":[]}}),
        ] {
            assert!(parse_page(&page, 0).is_err());
        }
    }

    #[test]
    fn synthetic_large_liked_list_follows_offsets_and_keeps_distinct_episode_ids() {
        for total in [1_000_i64, 5_000] {
            let mut offset = 0;
            let mut ids = HashSet::new();
            loop {
                let next = (offset + 100).min(total);
                let rows: Vec<_> = (offset + 1..=next).map(|vid| {
                    json!({"video_data":{"vid":vid,"series_id":"same-series","user_digg":true}})
                }).collect();
                let page = parse_page(&json!({"data":{"count":total,"has_more":next<total,"next_offset":next,"mixed_data_list":rows}}), offset).unwrap();
                for item in page.items {
                    assert_eq!(item.series_id, "same-series");
                    assert!(ids.insert(item.vid));
                }
                match page.next_offset {
                    Some(next) => offset = next,
                    None => break,
                }
            }
            assert_eq!(ids.len(), total as usize);
            assert!(ids.contains(&total.to_string()));
        }
    }
}
