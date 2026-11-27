use crate::{
    client::{ApiEnv, SINFONLINEC_API_ORIGIN, api_call_reading},
    discover::FeedItem,
};
use hongguo_common::{AppError, AppResult, model::Highlight};
use serde_json::{Value, json};

const ALBUM_PATH: &str = "/novel/player/album_detail/v1/";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HighlightAlbum {
    pub title: String,
    pub intro: String,
    pub items: Vec<AlbumMember>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AlbumMember {
    pub member_id: Option<String>,
    pub clip: FeedItem,
}

pub async fn fetch(album_id: &str, env: &ApiEnv) -> AppResult<HighlightAlbum> {
    let request = json!({
        "album_id": album_id,
        "need_video_detail_info": true,
        "biz_param": { "source": 4 }
    });
    let payload =
        serde_json::to_vec(&request).map_err(|error| AppError::Media(error.to_string()))?;
    let bytes =
        api_call_reading(SINFONLINEC_API_ORIGIN, ALBUM_PATH, Some(payload), &[], env).await?;
    let value = serde_json::from_slice(&bytes)
        .map_err(|error| AppError::Media(format!("解析作品合集失败: {error}")))?;
    parse(&value)
}

fn parse(value: &Value) -> AppResult<HighlightAlbum> {
    crate::discover::check_code(value)?;
    let album = value
        .pointer("/data/album_data")
        .filter(|data| data.is_object())
        .ok_or_else(|| AppError::NotFound("作品合集".into()))?;
    let details = value.pointer("/data/video_detail_data");
    let entries = album
        .get("video_detail_list")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let items: Vec<_> = entries
        .iter()
        .filter_map(|row| {
            let id = [row.get("series_id_str"), row.get("series_id")]
                .into_iter()
                .flatten()
                .find_map(super::id);
            let fallback = row
                .get("video_data")
                .filter(|data| data.is_object())
                .unwrap_or(row);
            let detail = Some(fallback)
                .filter(|data| {
                    data.get("video_list")
                        .and_then(Value::as_array)
                        .is_some_and(|videos| !videos.is_empty())
                })
                .or_else(|| {
                    id.as_ref()
                        .and_then(|id| details?.get(id))
                        .map(|detail| detail.get("video_data").unwrap_or(detail))
                })
                .unwrap_or(fallback);
            let mut item = crate::recommend::recommend_feed_entry(&json!({
                "series_id": id.as_deref(), "vid": row.get("vid"), "video_detail": detail
            }))?;
            if item.title.is_empty() {
                item.title = row
                    .get("series_title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
            }
            let highlight = item.highlight.get_or_insert_with(Highlight::default);
            highlight.episode_number = super::episode_number(row)
                .or_else(|| super::episode_number(detail))
                .or(highlight.episode_number);
            Some(AlbumMember {
                member_id: id,
                clip: item,
            })
        })
        .collect();
    log::debug!(
        "[HighlightAlbum] list entries={}, parsed={}",
        entries.len(),
        items.len()
    );
    let field = |key| {
        album
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Ok(HighlightAlbum {
        title: field("title"),
        intro: field("intro"),
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_uses_ordered_clips_and_exact_video_ids() {
        let page = parse(&json!({"code":0, "data": {
            "album_data": {"title": "Creator playlist", "video_detail_list": [
                {"series_id": 70, "series_title":"Second lexically",
                 "video_list":[{"vid":"71","series_id":"70"}]},
                {"series_id": 20, "series_title":"First lexically",
                 "video_list":[{"vid":"21","series_id":"20"}]}
            ]}
        }}))
        .unwrap();
        assert_eq!(page.title, "Creator playlist");
        assert_eq!(
            page.items
                .iter()
                .map(|member| member.clip.vid.as_str())
                .collect::<Vec<_>>(),
            ["71", "21"]
        );
        assert!(
            page.items
                .iter()
                .all(|member| member.clip.highlight.is_some())
        );
    }
}
