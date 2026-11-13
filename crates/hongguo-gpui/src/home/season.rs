//! GPUI adapts API works to the common season policy without owning another parser.
use hongguo_api::detail::RelatedItem;
pub(crate) use hongguo_common::model::season::season_no_from_tag;
use hongguo_common::model::season::{SeasonCandidate, next_season_index};

pub(crate) fn pick_next_season(
    works: &[RelatedItem],
    current_series_id: &str,
    current_title: &str,
) -> Option<RelatedItem> {
    let candidates = works
        .iter()
        .map(|work| SeasonCandidate {
            id: &work.series_id,
            tag: &work.tag,
            episode_count: work.episode_cnt,
        })
        .collect::<Vec<_>>();
    next_season_index(&candidates, current_series_id, current_title)
        .and_then(|at| works.get(at).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_policy_keeps_the_original_api_work_metadata() {
        assert_eq!(season_no_from_tag("第3季"), Some(3));
        let works = vec![
            RelatedItem {
                series_id: "third".into(),
                tag: "第3季".into(),
                episode_cnt: 30,
                title: "完整第三季标题".into(),
                cover: "https://example.invalid/cover".into(),
                ..Default::default()
            },
            RelatedItem {
                series_id: "second".into(),
                tag: "第2季".into(),
                episode_cnt: 0,
                ..Default::default()
            },
        ];
        assert_eq!(
            pick_next_season(&works, "first", "第一季"),
            Some(works[0].clone())
        );
        assert!(pick_next_season(&works, "third", "第三季").is_none());
    }
}
