use hongguo_api::rank::{RankItem, RankPage};

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum Audience {
    #[default]
    All,
    Male,
    Female,
}

impl Audience {
    pub(super) fn value(self) -> i64 {
        match self {
            Self::All => 2,
            Self::Male => 1,
            Self::Female => 0,
        }
    }
}

#[derive(Clone, Default, PartialEq)]
pub(super) struct Query {
    pub(super) audience: Audience,
    pub(super) offset: i64,
    pub(super) session: String,
    pub(super) previous: Vec<RankItem>,
    pub(super) identity: u64,
}

pub(super) fn finish_recommendation(mut page: RankPage, query: Query) -> RankPage {
    let received = page.items.len() as i64;
    page.next_offset = query.offset + received;
    page.has_more = received > 0;
    page.items = super::super::series_feed::append(query.previous, page.items);
    page
}

pub(super) fn card(item: RankItem, downloaded: usize) -> crate::ui::NewDramaCard {
    let mut category = item.tags;
    if let Some(first) = item
        .sub_title
        .split('·')
        .next()
        .filter(|value| !value.is_empty())
    {
        category.push(first.to_owned());
    }
    crate::ui::NewDramaCard {
        id: item.series_id,
        title: item.title,
        cover: item.cover,
        heat: item.rec_text,
        category: category
            .into_iter()
            .filter(|value| !value.is_empty())
            .take(4)
            .collect::<Vec<_>>()
            .join("·"),
        episodes: item.episode_cnt,
        score: item.score,
        downloaded,
    }
}
