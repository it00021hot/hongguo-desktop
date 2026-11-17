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
    pub(crate) fn from_suggestion(item: &hongguo_service::contracts::search::SuggestItem) -> Self {
        Self {
            series_id: item.series_id.clone(),
            title: item.word.clone(),
            cover: item.cover.clone(),
            desc: item.abstract_text.clone(),
            ..Default::default()
        }
    }

    pub(crate) fn from_rank(item: &hongguo_service::contracts::rank::RankItem) -> Self {
        Self {
            series_id: item.series_id.clone(),
            title: item.title.clone(),
            cover: item.cover.clone(),
            tags: item.tags.clone(),
            desc: item.description.clone(),
        }
    }
    pub(crate) fn from_work(item: &hongguo_service::contracts::danmaku::CreatorWork) -> Self {
        Self {
            series_id: item.series_id.clone(),
            title: item.title.clone(),
            cover: item.cover.clone(),
            tags: item.tags.clone(),
            desc: item.intro.clone(),
        }
    }
    pub(crate) fn from_calendar(item: &hongguo_service::contracts::calendar::CalendarItem) -> Self {
        Self {
            series_id: item.series_id.clone(),
            title: item.title.clone(),
            cover: item.cover.clone(),
            tags: item.rec_tags.clone(),
            desc: item.description.clone(),
        }
    }
    pub(crate) fn from_related(item: &hongguo_service::contracts::detail::RelatedItem) -> Self {
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

    pub(super) fn metadata(&self) -> hongguo_service::contracts::detail::SeriesMeta {
        hongguo_service::contracts::detail::SeriesMeta {
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
