use crate::i18n::tr;
use hongguo_common::model::LocalHistoryItem;
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

impl From<LocalHistoryItem> for HistoryItem {
    fn from(item: LocalHistoryItem) -> Self {
        Self {
            id: item.series_id,
            vid: item.vid,
            title: item.title,
            cover: ImageSrc::from(item.cover),
            episode: item.vid_index.max(1),
            episodes: item.episode_cnt,
            position_ms: item.position_ms,
            duration_ms: item.duration_ms,
            updated_at_ms: item.updated_at_ms,
        }
    }
}

impl From<hongguo_api::history::WatchHistoryItem> for HistoryItem {
    fn from(item: hongguo_api::history::WatchHistoryItem) -> Self {
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
        let query = query.trim().to_lowercase();
        tab_matches
            && (item.title.to_lowercase().contains(&query)
                || item.id.to_lowercase().contains(&query))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn item(episode: u32, episodes: u32) -> HistoryItem {
        HistoryItem {
            id: "Series-ABC".into(),
            vid: "episode".into(),
            title: "故事".into(),
            cover: ImageSrc::from(""),
            episode,
            episodes,
            position_ms: 0,
            duration_ms: 1,
            updated_at_ms: 0,
        }
    }

    #[test]
    fn completion_is_last_episode_even_without_finished_video() {
        let finished = item(24, 24);
        assert!(HistoryTab::Finished.matches(&finished, ""));
        assert!(!HistoryTab::Unfinished.matches(&finished, ""));
        assert!(HistoryTab::Unfinished.matches(&item(0, 80), ""));
        assert!(!HistoryTab::Finished.matches(&item(1, 0), ""));
    }

    #[test]
    fn search_matches_title_and_series_identity_after_trimming() {
        assert!(HistoryTab::All.matches(&item(1, 80), " abc "));
        assert!(HistoryTab::All.matches(&item(1, 80), "故事"));
        assert!(!HistoryTab::All.matches(&item(1, 80), "missing"));
        assert!(!HistoryTab::Finished.matches(&item(1, 80), "故事"));
    }
}
