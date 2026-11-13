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

#[cfg(test)]
mod tests {
    use super::*;
    fn item(id: &str) -> RankItem {
        RankItem {
            series_id: id.into(),
            title: id.into(),
            ..Default::default()
        }
    }

    #[test]
    fn new_releases_continue_by_raw_received_count_and_the_server_session() {
        let query = Query {
            offset: 18,
            previous: vec![item("one")],
            ..Default::default()
        };
        let page = finish_recommendation(
            RankPage {
                items: vec![item("one"), item("two")],
                has_more: false,
                next_offset: 0,
                session_id: "server-session".into(),
                ..Default::default()
            },
            query,
        );
        assert_eq!(page.next_offset, 20);
        assert!(page.has_more);
        assert_eq!(page.session_id, "server-session");
        assert_eq!(
            page.items
                .iter()
                .map(|item| item.series_id.as_str())
                .collect::<Vec<_>>(),
            ["one", "two"]
        );
    }

    #[test]
    fn an_empty_new_release_page_finishes_without_discarding_loaded_series() {
        let page = finish_recommendation(
            RankPage::default(),
            Query {
                offset: 18,
                previous: vec![item("one")],
                ..Default::default()
            },
        );
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.next_offset, 18);
        assert!(!page.has_more);
    }

    #[test]
    fn an_empty_first_page_is_an_empty_finished_feed() {
        let page = finish_recommendation(RankPage::default(), Query::default());
        assert!(page.items.is_empty());
        assert!(!page.has_more);
    }
}
