use super::{
    relay::Related,
    stage::{Panel, Season, SeasonState},
};
use crate::{
    bridge,
    navigation::{Navigation, Route},
    pages::SessionHandle,
};
use hongguo_api::detail::RelatedItem;
use hongguo_common::model::season::season_no_from_tag as season_number;
use hongguo_common::model::{Series, TaskStatus};
use nsh_core::prelude::*;
use std::collections::HashSet;
mod switching;

#[derive(Clone, Copy)]
pub(super) struct Controller {
    pub state: SeasonState,
    source: Resource<Vec<Season>, bridge::Failure>,
    message: Signal<SharedString>,
    pending: Signal<bool>,
    related: Related,
    switching: switching::Controller,
}

pub(super) fn observe(
    session: SessionHandle,
    selection: Signal<(String, u32)>,
    open: Signal<Option<Panel>>,
    related: Related,
    source_key: Memo<Option<super::relay::SourceEpoch>>,
) -> Controller {
    let source = resource(
        move || {
            if open.get() != Some(Panel::Episodes) || related.loading() {
                return None;
            }
            let id = selection.get().0;
            let identity = session.identity();
            related
                .with(|snapshot| {
                    snapshot
                        .filter(|snapshot| snapshot.id == id && snapshot.identity == identity)
                        .cloned()
                })
                .map(|snapshot| (id, identity, session.library(), snapshot))
        },
        move |(id, _, _, snapshot), _| {
            let state = session.with(|session| session.state.clone());
            bridge::request(async move {
                let reserved = if state.settings().account.is_some() {
                    hongguo_api::reservation::fetch_reservations(false, &state.api_env())
                        .await?
                        .items
                        .into_iter()
                        .map(|item| item.series_id)
                        .collect()
                } else {
                    HashSet::new()
                };
                Ok(seasons(
                    snapshot.related.works,
                    snapshot.current,
                    &id,
                    &reserved,
                ))
            })
        },
    );
    let pending = signal(false);
    let message = signal(SharedString::default());
    let switching = switching::Controller::new(
        computed(move || switching::Origin {
            identity: session.identity(),
            selection: selection.get(),
            source: source_key.get(),
        }),
        pending,
        message,
        switching::loader(session),
    );
    Controller {
        state: SeasonState {
            items: computed(move || source.with(|items| items.cloned().unwrap_or_default())),
            loading: computed(move || source.loading() || related.loading()),
            error: computed(move || {
                source
                    .error()
                    .or_else(|| related.error())
                    .map_or_else(|| message.get(), |error| error.text())
            }),
            reserving: computed(move || pending.get()),
            downloaded: computed(move || {
                session.downloads();
                let id = selection.get().0;
                session
                    .with(|session| session.state.queue().all())
                    .into_iter()
                    .filter(|task| task.series_id == id && task.status == TaskStatus::Completed)
                    .map(|task| task.vid_index)
                    .collect()
            }),
        },
        source,
        message,
        pending,
        related,
        switching,
    }
}

impl Controller {
    pub fn switch(self, id: String, selected: impl Fn(Series, u32) + 'static) {
        self.switching.start(id, selected);
    }
    pub fn retry(self) {
        self.message.set(SharedString::default());
        if self.related.error().is_some() {
            self.related.refetch();
        } else {
            self.source.refetch();
        }
    }
    pub fn reserve(self, session: SessionHandle, nav: Navigation, id: String) {
        if self.pending.get_untracked() {
            return;
        }
        let state = session.with(|session| session.state.clone());
        if state.settings().account.is_none() {
            nav.push(Route::Account);
            return;
        }
        if !self.state.items.with_untracked(|items| {
            items
                .iter()
                .any(|item| item.id == id && item.episodes == 0 && !item.reserved)
        }) {
            return;
        }
        self.pending.set(true);
        self.message.set(SharedString::default());
        let identity = session.identity();
        spawn(async move {
            let outcome = bridge::request(async move {
                hongguo_api::reservation::reserve_series(&id, true, &state.api_env()).await
            })
            .await;
            self.pending.set(false);
            if identity != session.identity() {
                return;
            }
            match outcome {
                Ok(()) => session.notify_library(),
                Err(error) => self.message.set(error.text()),
            }
        });
    }
}

fn seasons(
    works: Vec<RelatedItem>,
    current: Option<Series>,
    id: &str,
    reserved: &HashSet<String>,
) -> Vec<Season> {
    let mut works = works
        .into_iter()
        .filter(|item| season_number(&item.tag).is_some())
        .collect::<Vec<_>>();
    if !works.iter().any(|item| item.series_id == id)
        && let Some(current) = current
    {
        works.insert(
            0,
            RelatedItem {
                series_id: current.series_id,
                title: current.title,
                cover: current.cover,
                episode_cnt: current.episode_count,
                ..Default::default()
            },
        );
    }
    works.sort_by_key(|item| season_number(&item.tag).unwrap_or(0));
    works
        .into_iter()
        .map(|item| Season {
            current: item.series_id == id,
            reserved: reserved.contains(&item.series_id),
            id: item.series_id,
            title: item.title,
            cover: item.cover,
            tag: item.tag,
            episodes: item.episode_cnt,
            play_count: item.play_cnt,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seasons_keep_current_first_filter_same_ip_and_order_actual_seasons() {
        let works = vec![
            RelatedItem {
                series_id: "3".into(),
                tag: "第3季".into(),
                ..Default::default()
            },
            RelatedItem {
                series_id: "ip".into(),
                tag: "同IP".into(),
                ..Default::default()
            },
            RelatedItem {
                series_id: "2".into(),
                tag: "第2季".into(),
                ..Default::default()
            },
        ];
        let items = seasons(
            works,
            Some(Series {
                series_id: "1".into(),
                episode_count: 80,
                ..Default::default()
            }),
            "1",
            &HashSet::from(["3".into()]),
        );
        assert_eq!(
            items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["1", "2", "3"]
        );
        assert!(items[0].current);
        assert!(items[2].reserved);
        assert_eq!(items[0].episodes, 80);
    }
    #[test]
    fn missing_series_does_not_invent_a_current_season() {
        assert!(seasons(Vec::new(), None, "1", &HashSet::new()).is_empty());
        assert_eq!(season_number("第100季"), None);
        assert_eq!(season_number("第三季"), None);
    }
}
