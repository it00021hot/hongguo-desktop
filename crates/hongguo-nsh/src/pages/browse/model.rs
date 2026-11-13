use hongguo_api::discover::{BrowseFilters, SelectorItem, SelectorRow};
use hongguo_api::rank::RankItem;

/// Identifies result ownership independently of pagination and device revisions.
#[derive(Clone, Default, PartialEq)]
pub(super) struct Scope {
    filters: BrowseFilters,
    term: String,
}

#[derive(Clone, Default, PartialEq)]
pub(super) struct LoadedPage {
    pub scope: Scope,
    pub page: hongguo_api::rank::RankPage,
}

#[derive(Clone, Default, PartialEq)]
pub(super) struct Query {
    pub filters: BrowseFilters,
    pub term: String,
    pub offset: i64,
    pub session: String,
    pub previous: Vec<RankItem>,
    pub identity: u64,
}

impl Query {
    pub fn view_snapshot(&self) -> Self {
        Self {
            filters: self.filters.clone(),
            term: self.term.clone(),
            ..Default::default()
        }
    }

    pub fn scope(&self) -> Scope {
        Scope {
            filters: self.filters.clone(),
            term: self.term.clone(),
        }
    }

    pub fn pick(&mut self, dimension: Dimension, value: String) {
        if dimension.get(&self.filters) != value {
            dimension.set(&mut self.filters, value);
            self.reset_paging();
        }
    }

    pub fn selected_filter_count(&self) -> usize {
        Dimension::ALL
            .iter()
            .filter(|dimension| !dimension.get(&self.filters).is_empty())
            .count()
    }

    pub fn reset_paging(&mut self) {
        self.offset = 0;
        self.session.clear();
        self.previous.clear();
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Dimension {
    Genre,
    Theme,
    Role,
    Epoch,
    Sort,
    Gender,
    OnlineTime,
    Duration,
    Creation,
}

impl Dimension {
    const ALL: [Self; 9] = [
        Self::Genre,
        Self::Theme,
        Self::Role,
        Self::Epoch,
        Self::Sort,
        Self::Gender,
        Self::OnlineTime,
        Self::Duration,
        Self::Creation,
    ];
    pub fn from_row(row: &str) -> Option<Self> {
        Some(match row {
            "genre" => Self::Genre,
            "category_dim_theme" => Self::Theme,
            "category_dim_role" => Self::Role,
            "category_dim_epoch" => Self::Epoch,
            "sort" => Self::Sort,
            "gender" => Self::Gender,
            "online_time" => Self::OnlineTime,
            "duration" => Self::Duration,
            "creation_status" => Self::Creation,
            _ => return None,
        })
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Genre => "browse.fGenre",
            Self::Theme => "browse.fTheme",
            Self::Role => "browse.fRole",
            Self::Epoch => "browse.fEpoch",
            Self::Sort => "browse.fSort",
            Self::Gender => "browse.fGender",
            Self::OnlineTime => "browse.fOnlineTime",
            Self::Duration => "browse.fDuration",
            Self::Creation => "browse.fCreation",
        }
    }
    pub fn get(self, filters: &BrowseFilters) -> &str {
        match self {
            Self::Genre => &filters.genre,
            Self::Theme => &filters.theme,
            Self::Role => &filters.role,
            Self::Epoch => &filters.epoch,
            Self::Sort => &filters.sort,
            Self::Gender => &filters.gender,
            Self::OnlineTime => &filters.online_time,
            Self::Duration => &filters.duration,
            Self::Creation => &filters.creation_status,
        }
    }
    pub fn set(self, filters: &mut BrowseFilters, value: String) {
        let field = match self {
            Self::Genre => &mut filters.genre,
            Self::Theme => &mut filters.theme,
            Self::Role => &mut filters.role,
            Self::Epoch => &mut filters.epoch,
            Self::Sort => &mut filters.sort,
            Self::Gender => &mut filters.gender,
            Self::OnlineTime => &mut filters.online_time,
            Self::Duration => &mut filters.duration,
            Self::Creation => &mut filters.creation_status,
        };
        *field = value;
    }
}

pub(super) fn rows_with_fallback(
    mut rows: Vec<SelectorRow>,
    label: impl Fn(&str) -> String,
) -> Vec<SelectorRow> {
    if !rows.iter().any(|row| row.row_type == "duration") {
        rows.push(SelectorRow {
            row_type: "duration".into(),
            row_name: label("browse.fDuration"),
            items: [
                ("duration_0_60", "browse.duration060"),
                ("duration_60_120", "browse.duration60120"),
                ("duration_120_plus", "browse.duration120Plus"),
            ]
            .into_iter()
            .map(|(id, key)| SelectorItem {
                id: id.into(),
                name: label(key),
            })
            .collect(),
        });
    }
    if !rows.iter().any(|row| row.row_type == "creation_status") {
        rows.push(SelectorRow {
            row_type: "creation_status".into(),
            row_name: label("browse.fCreation"),
            items: [
                ("creation_status_0", "browse.creationFinished"),
                ("creation_status_1", "browse.creationOngoing"),
            ]
            .into_iter()
            .map(|(id, key)| SelectorItem {
                id: id.into(),
                name: label(key),
            })
            .collect(),
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_duration_uses_verified_options_without_replacing_server_choices() {
        let rows = rows_with_fallback(Vec::new(), str::to_owned);
        let duration = rows.iter().find(|row| row.row_type == "duration").unwrap();
        assert_eq!(
            duration
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["duration_0_60", "duration_60_120", "duration_120_plus"]
        );
        let server = SelectorRow {
            row_type: "duration".into(),
            row_name: "Custom".into(),
            items: Vec::new(),
        };
        let rows = rows_with_fallback(vec![server.clone()], str::to_owned);
        assert_eq!(
            rows.iter().filter(|row| row.row_type == "duration").count(),
            1
        );
        assert_eq!(rows[0], server);
        assert!(rows.iter().any(|row| row.row_type == "creation_status"));
    }

    #[test]
    fn choosing_one_dimension_preserves_the_others_and_resets_only_paging() {
        let mut query = Query {
            filters: BrowseFilters {
                genre: "comic_series".into(),
                theme: "cate_262".into(),
                ..Default::default()
            },
            term: "keyword".into(),
            offset: 40,
            session: "cursor".into(),
            previous: vec![RankItem::default()],
            identity: 9,
        };
        Dimension::Duration.set(&mut query.filters, "duration_0_60".into());
        query.reset_paging();
        assert_eq!(Dimension::Genre.get(&query.filters), "comic_series");
        assert_eq!(Dimension::Theme.get(&query.filters), "cate_262");
        assert_eq!(Dimension::Duration.get(&query.filters), "duration_0_60");
        assert_eq!(
            (query.offset, query.session.as_str(), query.previous.len()),
            (0, "", 0)
        );
        assert_eq!((query.term.as_str(), query.identity), ("keyword", 9));
        assert!(Dimension::from_row("future_unknown_dimension").is_none());
    }
}
