use hongguo_common::model::{Highlight, RelatedDrama, RelatedKind};
use serde_json::Value;
pub mod album;

fn id(value: &Value) -> Option<String> {
    let id = match value {
        Value::String(value) => value.trim().to_owned(),
        Value::Number(value) => value.to_string(),
        _ => return None,
    };
    (id.parse::<u64>().is_ok_and(|number| number > 0)).then_some(id)
}

fn episode_number(detail: &Value) -> Option<u32> {
    [
        detail.pointer("/video_list/0/vid_index"),
        detail.get("vid_index"),
    ]
    .into_iter()
    .flatten()
    .find_map(|value| {
        let number = value.as_u64().or_else(|| value.as_str()?.parse().ok())?;
        u32::try_from(number).ok().filter(|number| *number > 0)
    })
}

/// A single archive entry is not a clip marker. Use official PUGC metadata
/// and preserve the playback identity separately from the attached drama.
pub(crate) fn parse(
    video_detail: &Value,
    outer_cell: Option<&Value>,
    playback_id: &str,
) -> Option<Highlight> {
    let author = video_detail
        .pointer("/ugc_user_info/base_info/user_name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let related_id = video_detail
        .get("related_album_id")
        .and_then(id)
        .or_else(|| {
            outer_cell
                .and_then(|cell| cell.pointer("/extra/related_album_id"))
                .and_then(id)
        })
        .filter(|related| related != playback_id);
    let pugc = outer_cell
        .and_then(|cell| cell.pointer("/cell_extra_data/rec_filter_info/filter_id_map/pugc_id"))
        .and_then(id)
        .is_some();
    if !pugc
        && !(related_id.is_some()
            && video_detail
                .get("ugc_user_info")
                .is_some_and(Value::is_object))
    {
        return None;
    }
    let bar = outer_cell
        .and_then(|cell| cell.get("bottom_bar"))
        .filter(|bar| bar.is_object())
        .or_else(|| video_detail.get("bottom_bar"));
    let related = related_id.map(|series_id| RelatedDrama {
        series_id,
        kind: if bar
            .and_then(|bar| bar.get("bottom_bar_type"))
            .and_then(Value::as_str)
            == Some("album")
        {
            RelatedKind::Album
        } else {
            RelatedKind::Drama
        },
        label: bar
            .and_then(|bar| bar.get("text"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(" · "),
    });
    Some(Highlight {
        author,
        related,
        episode_number: episode_number(video_detail),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn single_episode_alone_is_not_a_highlight() {
        let video = json!({"episode_cnt": 1, "series_title": "A one-episode drama"});
        assert!(parse(&video, None, "10").is_none());
    }

    #[test]
    fn creator_clip_without_related_drama_has_no_destination() {
        let cell =
            json!({"cell_extra_data":{"rec_filter_info":{"filter_id_map":{"pugc_id":"10"}}}});
        let video =
            json!({"related_album_id": "10", "bottom_bar":{"schema":"https://example.test/"}});
        let highlight = parse(&video, Some(&cell), "10").unwrap();
        assert!(highlight.related.is_none());
    }

    #[test]
    fn top_level_bottom_bar_has_priority_over_detail_copy() {
        let cell = json!({"bottom_bar": {"text": ["Fresh label", "70 episodes"], "bottom_bar_type":"album"}});
        let video = json!({"related_album_id": "98", "ugc_user_info": {}, "bottom_bar": {"text": ["Old label"]}});
        let highlight = parse(&video, Some(&cell), "10").unwrap();
        assert_eq!(highlight.related.as_ref().unwrap().kind, RelatedKind::Album);
        assert_eq!(
            highlight.related.unwrap().label,
            "Fresh label · 70 episodes"
        );
    }
}
