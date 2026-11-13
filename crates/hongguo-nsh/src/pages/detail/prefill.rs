/// Card data available before resolving a series, including known upcoming releases.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DetailPrefill {
    pub series_id: String,
    pub title: String,
    pub cover: String,
    pub tags: Vec<String>,
    pub desc: String,
}

impl DetailPrefill {
    pub(crate) fn from_rank(item: &hongguo_api::rank::RankItem) -> Self {
        Self {
            series_id: item.series_id.clone(),
            title: item.title.clone(),
            cover: item.cover.clone(),
            tags: item.tags.clone(),
            desc: item.description.clone(),
        }
    }
    pub(crate) fn from_work(item: &hongguo_api::danmaku::CreatorWork) -> Self {
        Self {
            series_id: item.series_id.clone(),
            title: item.title.clone(),
            cover: item.cover.clone(),
            tags: item.tags.clone(),
            desc: item.intro.clone(),
        }
    }
    pub(crate) fn from_calendar(item: &hongguo_api::calendar::CalendarItem) -> Self {
        Self {
            series_id: item.series_id.clone(),
            title: item.title.clone(),
            cover: item.cover.clone(),
            tags: item.rec_tags.clone(),
            desc: item.description.clone(),
        }
    }
    pub(crate) fn from_related(item: &hongguo_api::detail::RelatedItem) -> Self {
        Self {
            series_id: item.series_id.clone(),
            title: item.title.clone(),
            cover: item.cover.clone(),
            tags: item
                .tag
                .split(',')
                .filter(|tag| !tag.is_empty())
                .map(str::to_owned)
                .collect(),
            desc: item.video_desc.clone(),
        }
    }
    pub(super) fn series(&self) -> hongguo_common::model::Series {
        hongguo_common::model::Series {
            series_id: self.series_id.clone(),
            title: self.title.clone(),
            cover: self.cover.clone(),
            tags: self.tags.clone(),
            ..Default::default()
        }
    }

    pub(super) fn metadata(&self) -> hongguo_api::detail::SeriesMeta {
        hongguo_api::detail::SeriesMeta {
            series_id: self.series_id.clone(),
            title: self.title.clone(),
            cover: self.cover.clone(),
            tags: self.tags.clone(),
            intro: self.desc.clone(),
            series_status: 1,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_is_a_zero_episode_display_snapshot_and_not_a_playable_archive() {
        let seed = DetailPrefill {
            series_id: "upcoming".into(),
            title: "Future story".into(),
            cover: "local-frame.png".into(),
            tags: vec!["Drama".into()],
            desc: "Known intro".into(),
        };
        let series = seed.series();
        assert_eq!(series.series_id, seed.series_id);
        assert_eq!(series.title, seed.title);
        assert_eq!(series.tags, seed.tags);
        assert_eq!(series.episode_count, 0);
        assert!(series.episodes.is_empty());
        assert_eq!(seed.metadata().intro, seed.desc);
    }
}
