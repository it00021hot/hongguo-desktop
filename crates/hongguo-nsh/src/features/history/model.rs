use crate::i18n::tr;
use nsh_core::element::{ImageSrc, SharedString};

#[derive(Clone, PartialEq)]
pub(super) struct HistoryItem {
    pub id: String,
    pub vid: String,
    pub title: String,
    pub cover: ImageSrc,
    pub episode: u32,
    pub episodes: u32,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub updated_at_ms: i64,
}

impl From<hongguo_service::contracts::history::WatchHistoryItem> for HistoryItem {
    fn from(item: hongguo_service::contracts::history::WatchHistoryItem) -> Self {
        Self {
            id: item.series_id,
            vid: item.vid,
            title: item.title,
            cover: ImageSrc::from(item.cover),
            episode: item.vid_index.max(1).min(i64::from(u32::MAX)) as u32,
            episodes: item.episode_cnt.max(0).min(i64::from(u32::MAX)) as u32,
            position_ms: item.position_ms,
            duration_ms: item.duration_ms,
            updated_at_ms: item.updated_at_ms,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum HistoryTab {
    All,
    Unfinished,
    Finished,
}

impl HistoryTab {
    pub const ALL: [Self; 3] = [Self::All, Self::Unfinished, Self::Finished];
    pub fn label(self) -> SharedString {
        tr(match self {
            Self::All => "history.tabAll",
            Self::Unfinished => "history.tabUnfinished",
            Self::Finished => "history.tabFinished",
        })
    }
    pub fn matches(self, item: &HistoryItem, query: &str) -> bool {
        let finished = item.episodes > 0 && item.episode.max(1) >= item.episodes;
        let tab_matches = match self {
            Self::All => true,
            Self::Unfinished => !finished,
            Self::Finished => finished,
        };
        tab_matches
            && (query.is_empty()
                || item.title.to_lowercase().contains(query)
                || item.id.to_lowercase().contains(query))
    }
}

pub(super) fn watching(item: &HistoryItem) -> SharedString {
    if item.duration_ms <= 0 {
        return tr("player.epShort")
            .replace("{index}", &item.episode.max(1).to_string())
            .into();
    }
    let total = if item.episodes == 0 {
        "–".into()
    } else {
        item.episodes.to_string()
    };
    let seconds = item.position_ms.max(0) / 1_000;
    tr("history.watching")
        .replace("{index}", &item.episode.max(1).to_string())
        .replace("{total}", &total)
        .replace(
            "{time}",
            &if seconds >= 3600 {
                format!(
                    "{}:{:02}:{:02}",
                    seconds / 3600,
                    seconds / 60 % 60,
                    seconds % 60
                )
            } else {
                format!("{:02}:{:02}", seconds / 60, seconds % 60)
            },
        )
        .into()
}

pub(super) fn time_ago(updated: i64, now: i64) -> SharedString {
    if updated <= 0 {
        return SharedString::default();
    }
    match now.saturating_sub(updated) / 86_400_000 {
        days if days <= 0 => tr("history.today"),
        1 => tr("history.yesterday"),
        days => tr("history.daysAgo")
            .replace("{count}", &days.to_string())
            .into(),
    }
}
