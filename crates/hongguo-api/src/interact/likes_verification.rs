//! Canonical, paginated post-mutation verification for signed-in video likes.
use std::collections::HashSet;

use hongguo_common::{AppError, AppResult};
use serde_json::Value;

use crate::client::{ApiEnv, LQ_API_ORIGIN, api_call_reading};

pub async fn fetch_active_ids(env: &ApiEnv) -> AppResult<HashSet<String>> {
    let mut ids = HashSet::new();
    let mut offset = 0;
    for _ in 0..100 {
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
        match read_page(&value, offset, &mut ids)? {
            Some(next) => offset = next,
            None => return Ok(ids),
        }
    }
    Err(invalid("page limit"))
}

fn invalid(reason: &str) -> AppError {
    AppError::Internal(format!("invalid liked videos response: {reason}"))
}

fn read_page(value: &Value, offset: i64, ids: &mut HashSet<String>) -> AppResult<Option<i64>> {
    let data = value.get("data").ok_or_else(|| invalid("missing data"))?;
    let has_more = data
        .get("has_more")
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid("missing has_more"))?;
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
                let id = video
                    .get("vid")
                    .and_then(super::list::bookshelf_id)
                    .ok_or_else(|| invalid("missing video identifier"))?;
                let liked = video
                    .get("user_digg")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| invalid("missing liked state"))?;
                if liked {
                    ids.insert(id);
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
        return Ok(None);
    }
    let next = data
        .get("next_offset")
        .and_then(Value::as_i64)
        .filter(|&next| next > offset)
        .ok_or_else(|| invalid("cursor did not advance"))?;
    Ok(Some(next))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn verification_collects_only_actual_likes_and_follows_cursor() {
        let mut ids = HashSet::new();
        let page = json!({"data":{"has_more":true,"next_offset":20,"mixed_data_list":[
            {"video_data":{"vid":"a","user_digg":true}},
            {"video_data":{"vid":"b","user_digg":false}},
            {"comment_data": {}}
        ]}});
        assert_eq!(read_page(&page, 0, &mut ids).unwrap(), Some(20));
        assert_eq!(ids, HashSet::from(["a".into()]));
        assert!(read_page(&page, 20, &mut ids).is_err());
        let empty = json!({"data":{"count":0,"has_more":false,"mixed_data_list":null}});
        assert_eq!(read_page(&empty, 0, &mut ids).unwrap(), None);
    }

    #[test]
    fn absent_or_malformed_data_cannot_confirm_removal() {
        let mut ids = HashSet::new();
        for page in [
            json!({"data":{"count":2,"has_more":false,"mixed_data_list":null}}),
            json!({"data":{"has_more":false,"mixed_data_list":[{"video_data":{"vid":"a"}}]}}),
            json!({"data":{"mixed_data_list":[]}}),
        ] {
            assert!(read_page(&page, 0, &mut ids).is_err());
        }
    }
}
