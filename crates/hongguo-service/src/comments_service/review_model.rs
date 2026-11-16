use hongguo_api::danmaku::{CommentTagStat, SeriesReviewPage};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReviewSummary {
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

pub fn summary_for_page(cursor: &str, page: &SeriesReviewPage) -> Option<ReviewSummary> {
    cursor.is_empty().then(|| ReviewSummary::from(page))
}

/// Star choice is UI state. Only this conversion crosses the review API boundary.
pub fn review_score(stars: u8) -> Option<i64> {
    (1..=5).contains(&stars).then_some(i64::from(stars) * 2)
}
