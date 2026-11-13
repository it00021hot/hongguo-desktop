use hongguo_api::detail::RelatedSeries;
use hongguo_common::model::season::{SeasonCandidate, next_season_index};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceKey {
    pub series_id: String,
    pub episode: u32,
    pub media: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Ticket {
    pub source: SourceKey,
    pub identity: u64,
    pub paused: bool,
    pub feed_revision: u64,
    serial: u64,
}

#[derive(Default)]
pub(super) struct Gate {
    current: Option<(SourceKey, u64)>,
    serial: u64,
    busy: Option<u64>,
    consumed: bool,
}

impl Gate {
    pub fn observe(&mut self, current: Option<(SourceKey, u64)>) {
        if self.current != current {
            self.current = current;
            self.busy = None;
            self.consumed = false;
        }
    }
    pub fn begin(
        &mut self,
        source: SourceKey,
        identity: u64,
        paused: bool,
        feed_revision: u64,
    ) -> Option<Ticket> {
        self.observe(Some((source.clone(), identity)));
        if self.busy.is_some() || self.consumed {
            return None;
        }
        self.serial = self.serial.wrapping_add(1);
        self.busy = Some(self.serial);
        Some(Ticket {
            source,
            identity,
            paused,
            feed_revision,
            serial: self.serial,
        })
    }
    pub fn matches(&self, ticket: &Ticket) -> bool {
        self.current.as_ref().is_some_and(|(source, identity)| {
            source == &ticket.source && *identity == ticket.identity
        }) && self.busy == Some(ticket.serial)
    }
    pub fn completed(&self, source: &SourceKey, identity: u64) -> bool {
        self.consumed
            && self
                .current
                .as_ref()
                .is_some_and(|(current, account)| current == source && *account == identity)
    }
    pub fn retry(&mut self, ticket: &Ticket) {
        if self.serial == ticket.serial
            && self.busy.is_none()
            && self.current.as_ref().is_some_and(|(source, identity)| {
                source == &ticket.source && *identity == ticket.identity
            })
        {
            self.consumed = false;
        }
    }
    pub fn finish(&mut self, ticket: &Ticket, success: bool) -> bool {
        if !self.matches(ticket) {
            return false;
        }
        self.busy = None;
        self.consumed = success;
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SeriesKind {
    Season,
    Guess,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Destination {
    Series { id: String, kind: SeriesKind },
    Feed,
    Last,
}

pub(super) fn pick(
    related: &RelatedSeries,
    current_id: &str,
    title: &str,
    feed: bool,
) -> Destination {
    let candidates = related
        .works
        .iter()
        .map(|work| SeasonCandidate {
            id: &work.series_id,
            tag: &work.tag,
            episode_count: work.episode_cnt,
        })
        .collect::<Vec<_>>();
    if let Some(at) = next_season_index(&candidates, current_id, title) {
        return Destination::Series {
            id: related.works[at].series_id.clone(),
            kind: SeriesKind::Season,
        };
    }
    // Tauri retains the online adjacent work for legacy untagged responses.
    if let Some(next) = related
        .works
        .iter()
        .position(|work| work.series_id == current_id)
        .and_then(|at| related.works.get(at + 1))
        .filter(|work| work.episode_cnt > 0)
    {
        return Destination::Series {
            id: next.series_id.clone(),
            kind: SeriesKind::Season,
        };
    }
    if feed {
        return Destination::Feed;
    }
    if let Some(next) = related.guess.first() {
        return Destination::Series {
            id: next.series_id.clone(),
            kind: SeriesKind::Guess,
        };
    }
    Destination::Last
}

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_api::detail::RelatedItem;

    fn source(id: &str, media: &str) -> SourceKey {
        SourceKey {
            series_id: id.into(),
            episode: 8,
            media: media.into(),
        }
    }
    fn work(id: &str, tag: &str, episodes: u32) -> RelatedItem {
        RelatedItem {
            series_id: id.into(),
            tag: tag.into(),
            episode_cnt: episodes,
            ..Default::default()
        }
    }
    #[test]
    fn chooses_next_season_before_feed_and_preserves_online_legacy_fallback() {
        let mut related = RelatedSeries {
            works: vec![work("s3", "第3季", 30), work("s2", "第2季", 0)],
            ..Default::default()
        };
        assert_eq!(
            pick(&related, "s1", "第一季", true),
            Destination::Series {
                id: "s3".into(),
                kind: SeriesKind::Season
            }
        );
        related.works = vec![work("current", "", 10), work("legacy", "", 20)];
        assert_eq!(
            pick(&related, "current", "", true),
            Destination::Series {
                id: "legacy".into(),
                kind: SeriesKind::Season
            }
        );
        related.works[1].episode_cnt = 0;
        assert_eq!(pick(&related, "current", "", true), Destination::Feed);
    }
    #[test]
    fn feed_precedes_guess_and_standalone_falls_back_to_guess_then_last() {
        let related = RelatedSeries {
            guess: vec![work("guess", "", 10)],
            ..Default::default()
        };
        assert_eq!(pick(&related, "now", "", true), Destination::Feed);
        assert_eq!(
            pick(&related, "now", "", false),
            Destination::Series {
                id: "guess".into(),
                kind: SeriesKind::Guess
            }
        );
        assert_eq!(
            pick(&RelatedSeries::default(), "now", "", false),
            Destination::Last
        );
    }
    #[test]
    fn duplicate_ends_and_stale_source_or_account_completions_cannot_double_advance() {
        let mut gate = Gate::default();
        let old = gate.begin(source("one", "lease-one"), 0, false, 0).unwrap();
        assert!(
            gate.begin(source("one", "lease-one"), 0, false, 0)
                .is_none()
        );
        gate.observe(Some((source("one", "lease-two"), 0)));
        let new = gate.begin(source("one", "lease-two"), 0, false, 0).unwrap();
        assert!(!gate.finish(&old, true));
        assert!(gate.matches(&new));
        gate.observe(Some((source("one", "lease-two"), 1)));
        assert!(!gate.finish(&new, true));
        let current = gate.begin(source("one", "lease-two"), 1, false, 0).unwrap();
        assert!(gate.finish(&current, true));
        assert!(
            gate.begin(source("one", "lease-two"), 1, false, 0)
                .is_none()
        );
        gate.observe(None);
        assert!(
            gate.begin(source("two", "lease-three"), 1, false, 0)
                .is_some()
        );
    }
}
