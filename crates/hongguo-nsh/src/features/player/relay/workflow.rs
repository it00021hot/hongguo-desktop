use super::{
    backend::Resolve,
    model::{Destination, SeriesKind, Ticket},
    *,
};
use crate::bridge;
use hongguo_common::{AppError, model::Series};
use std::convert::Infallible;

#[derive(Clone, PartialEq, Eq)]
enum Action {
    Destination(Destination),
    Failure(SharedString),
}

#[derive(Clone, PartialEq, Eq)]
struct Job {
    ticket: Ticket,
    action: Action,
}

#[derive(Clone)]
enum Transition {
    Series(Series, u32, SeriesKind),
    Feed,
    Last,
}

#[derive(Clone)]
struct Outcome {
    ticket: Ticket,
    result: Result<Transition, SharedString>,
}

pub(super) fn install(
    controller: Controller,
    feed: Option<FeedNavigation>,
    notice: Signal<SharedString>,
    selected: impl Fn(Series, u32) + Copy + 'static,
    resolve: Resolve,
) {
    let jobs: Resource<Outcome, Infallible> = resource(
        move || {
            let ticket = controller.requested.get()?;
            if !controller.current(&ticket) || controller.related.loading() {
                return None;
            }
            let snapshot = controller.related.with(|snapshot| {
                snapshot
                    .filter(|snapshot| {
                        snapshot.id == ticket.source.series_id
                            && snapshot.identity == ticket.identity
                    })
                    .cloned()
            });
            let action = if let Some(snapshot) = snapshot {
                Action::Destination(model::pick(
                    &snapshot.related,
                    &snapshot.id,
                    snapshot
                        .current
                        .as_ref()
                        .map_or("", |series| series.title.as_str()),
                    feed.is_some(),
                ))
            } else {
                let error = controller.related.error()?;
                if feed.is_some() {
                    Action::Destination(Destination::Feed)
                } else {
                    Action::Failure(error.text())
                }
            };
            Some(Job { ticket, action })
        },
        move |job: Job, _| {
            let resolve = resolve.clone();
            async move {
                let result = match job.action {
                    Action::Failure(message) => Err(message),
                    Action::Destination(Destination::Feed) => Ok(Transition::Feed),
                    Action::Destination(Destination::Last) => Ok(Transition::Last),
                    Action::Destination(Destination::Series { id, kind }) => {
                        match resolve(id).await {
                            Ok(series) => first_episode(&series)
                                .map(|episode| Transition::Series(series, episode, kind))
                                .map_err(|error| bridge::Failure::from(error).text()),
                            Err(error) => Err(error.text()),
                        }
                    }
                };
                Ok(Outcome {
                    ticket: job.ticket,
                    result,
                })
            }
        },
    );
    effect(move || {
        if jobs.loading() {
            return;
        }
        let Some(outcome) = jobs.with(|outcome| outcome.cloned()) else {
            return;
        };
        if !controller.current(&outcome.ticket) {
            return;
        }
        let success = outcome.result.is_ok();
        if !controller
            .gate
            .with(|gate| gate.finish(&outcome.ticket, success))
        {
            return;
        }
        controller.requested.set(None);
        match outcome.result {
            Ok(Transition::Series(series, episode, kind)) => {
                notice.set(
                    tr(match kind {
                        SeriesKind::Season => "player.playNextSeason",
                        SeriesKind::Guess => "player.autoPlayRecommend",
                    })
                    .replace("{title}", &series.title)
                    .into(),
                );
                if let Some(feed) = feed {
                    feed.lock_series(series.series_id.clone());
                    if feed.request_goto(series.clone(), episode) {
                        return;
                    }
                }
                selected(series, episode);
            }
            Ok(Transition::Feed) => {
                if !feed.is_some_and(|feed| feed.advance_feed()) {
                    controller.gate.with(|gate| gate.retry(&outcome.ticket));
                    controller.state.paused.set(true);
                    notice.set(last_notice(outcome.ticket.source.episode));
                }
            }
            Ok(Transition::Last) => {
                controller.state.paused.set(true);
                notice.set(last_notice(outcome.ticket.source.episode));
            }
            Err(error) => {
                controller.state.paused.set(true);
                notice.set(error);
            }
        }
    });
}

fn first_episode(series: &Series) -> Result<u32, AppError> {
    series
        .episodes
        .iter()
        .map(|episode| episode.vid_index)
        .min()
        .ok_or_else(|| AppError::NotFound("剧集暂无可播放分集".into()))
}
