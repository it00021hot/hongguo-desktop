use hongguo_api::danmaku::{CommentTagStat, SeriesReviewPage};

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct ReviewSummary {
    pub score: String,
    pub count: i64,
    pub tags: Vec<CommentTagStat>,
}

impl From<&SeriesReviewPage> for ReviewSummary {
    fn from(page: &SeriesReviewPage) -> Self {
        Self {
            score: page.score.clone(),
            count: page.score_cnt,
            tags: page.tag_stats.clone(),
        }
    }
}

pub(super) fn summary_for_page(cursor: &str, page: &SeriesReviewPage) -> Option<ReviewSummary> {
    cursor.is_empty().then(|| ReviewSummary::from(page))
}

/// Star choice is UI state. Only this conversion crosses the review API boundary.
pub(super) fn review_score(stars: u8) -> Option<i64> {
    (1..=5).contains(&stars).then_some(i64::from(stars) * 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subsequent_pages_cannot_replace_the_first_page_rating_metadata() {
        let page = SeriesReviewPage {
            score: "8.3".into(),
            score_cnt: 42,
            ..Default::default()
        };
        assert_eq!(summary_for_page("", &page).unwrap().score, "8.3");
        assert!(summary_for_page("next-cursor", &SeriesReviewPage::default()).is_none());
    }
    #[test]
    fn review_summary_uses_the_credibility_fields_and_filter_tags() {
        let page = SeriesReviewPage {
            score: "8.3".into(),
            score_cnt: 1001,
            tags: vec!["这是题材，不是评分统计".into()],
            tag_stats: vec![CommentTagStat {
                tag_name: "世界观宏大".into(),
                count: 26,
            }],
            ..Default::default()
        };
        let summary = ReviewSummary::from(&page);
        assert_eq!(summary.score, "8.3");
        assert_eq!(summary.count, 1001);
        assert_eq!(summary.tags[0].count, 26);
    }
    #[test]
    fn an_unselected_or_out_of_range_star_choice_cannot_be_sent_as_a_rating() {
        assert_eq!(review_score(0), None);
        assert_eq!(review_score(1), Some(2));
        assert_eq!(review_score(5), Some(10));
        assert_eq!(review_score(6), None);
    }
}
