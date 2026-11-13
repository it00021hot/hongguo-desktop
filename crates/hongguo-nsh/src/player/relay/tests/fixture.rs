use super::super::*;
use crate::{bridge, ui};
use hongguo_api::detail::{RelatedItem, RelatedSeries};
use hongguo_common::{AppError, model::Episode};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use std::rc::Rc;

#[derive(Clone, Copy)]
enum Mode {
    Season,
    Feed,
    Guess,
    Last,
    FeedError,
}

fn fixture(mode: Mode) -> View {
    let key = signal(Some(SourceKey {
        series_id: "one".into(),
        episode: 8,
        media: "lease-one".into(),
    }));
    let identity = signal(0_u64);
    let downloading = signal(false);
    let paused = signal(false);
    let active = signal(true);
    let calls = signal(Vec::<String>::new());
    let requests = signal(0_u32);
    let advanced = signal(0_u32);
    let notice = signal(SharedString::default());
    let discarded = signal(0_u32);
    let waiters = stored(Vec::<
        tokio::sync::oneshot::Sender<Result<Series, bridge::Failure>>,
    >::new());
    let related: Related = resource(
        move || Some(identity.get()),
        move |identity, _| {
            std::future::ready(if matches!(mode, Mode::FeedError) {
                Err(bridge::Failure::from(AppError::Internal(
                    "offline related failed".into(),
                )))
            } else {
                Ok(related::Snapshot {
                    id: "one".into(),
                    identity,
                    current: Some(Series {
                        series_id: "one".into(),
                        title: "第一季".into(),
                        ..Default::default()
                    }),
                    related: RelatedSeries {
                        works: if matches!(mode, Mode::Season) {
                            vec![RelatedItem {
                                series_id: "next".into(),
                                tag: "第2季".into(),
                                episode_cnt: 10,
                                ..Default::default()
                            }]
                        } else {
                            Vec::new()
                        },
                        guess: if matches!(mode, Mode::Feed | Mode::Guess) {
                            vec![RelatedItem {
                                series_id: "guess".into(),
                                episode_cnt: 10,
                                ..Default::default()
                            }]
                        } else {
                            Vec::new()
                        },
                    },
                })
            })
        },
    );
    let selected = move |series: Series, episode| {
        calls.update(|calls| calls.push(format!("{}:{episode}", series.series_id)));
        paused.set(false);
    };
    let feed = matches!(mode, Mode::Season | Mode::Feed | Mode::FeedError).then(|| {
        FeedNavigation::new(move |_| {
            advanced.update(|count| *count += 1);
            true
        })
        .on_goto(selected)
    });
    let controller = Controller::build(
        State {
            source: computed(move || key.get()),
            active: computed(move || active.get()),
            downloading: computed(move || downloading.get()),
            paused,
        },
        computed(move || identity.get()),
        related,
        computed(move || feed.map_or(0, |feed| feed.revision())),
    );
    let resolve: backend::Resolve = Rc::new(move |id| {
        requests.update(|count| *count += 1);
        let (send, receive) = tokio::sync::oneshot::channel();
        waiters.with(|waiters| waiters.push(send));
        Box::pin(async move {
            receive.await.unwrap_or_else(|_| {
                Err(bridge::Failure::from(AppError::Internal(format!(
                    "cancelled fake {id}"
                ))))
            })
        })
    });
    workflow::install(controller, feed, notice, selected, resolve);
    let pending = controller.pending();
    let complete = move |fail| {
        let sender = waiters.with(|waiters| (!waiters.is_empty()).then(|| waiters.remove(0)));
        if let Some(sender) = sender {
            let value = if fail {
                Err(bridge::Failure::from(AppError::Internal(
                    "offline resolve failed".into(),
                )))
            } else {
                Ok(Series {
                    series_id: "resolved".into(),
                    title: "第二季".into(),
                    episodes: vec![
                        Episode {
                            vid_index: 9,
                            ..Default::default()
                        },
                        Episode {
                            vid_index: 3,
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                })
            };
            if sender.send(value).is_err() {
                discarded.update(|count| *count += 1);
            }
        }
    };
    view()
        .flex_1()
        .child(
            ui::button("Advance", ui::ButtonVariant::Quiet).on_press(move || {
                controller.advance();
            }),
        )
        .child(ui::button("Complete", ui::ButtonVariant::Quiet).on_press(move || complete(false)))
        .child(ui::button("Fail", ui::ButtonVariant::Quiet).on_press(move || complete(true)))
        .child(
            ui::button("Source", ui::ButtonVariant::Quiet).on_press(move || {
                key.update(|key| {
                    if let Some(key) = key {
                        key.media.push_str("-new");
                    }
                })
            }),
        )
        .child(
            ui::button("Account", ui::ButtonVariant::Quiet)
                .on_press(move || identity.update(|identity| *identity += 1)),
        )
        .child(
            ui::button("Download", ui::ButtonVariant::Quiet)
                .on_press(move || downloading.set(true)),
        )
        .child(ui::button("Pause", ui::ButtonVariant::Quiet).on_press(move || paused.set(true)))
        .child(ui::button("Leave", ui::ButtonVariant::Quiet).on_press(move || active.set(false)))
        .child(
            ui::button("Feed context", ui::ButtonVariant::Quiet).on_press(move || {
                if let Some(feed) = feed {
                    feed.invalidate();
                }
            }),
        )
        .child(text(move || {
            format!(
                "requests:{} pending:{} targets:{} feed:{} paused:{} discarded:{}",
                requests.get(),
                pending.get(),
                calls.get().join(","),
                advanced.get(),
                paused.get(),
                discarded.get()
            )
        }))
        .child(text(notice))
}

#[component]
pub(super) fn SeasonHost() -> impl IntoElement {
    fixture(Mode::Season)
}
#[component]
pub(super) fn FeedHost() -> impl IntoElement {
    fixture(Mode::Feed)
}
#[component]
pub(super) fn GuessHost() -> impl IntoElement {
    fixture(Mode::Guess)
}
#[component]
pub(super) fn LastHost() -> impl IntoElement {
    fixture(Mode::Last)
}
#[component]
pub(super) fn FeedErrorHost() -> impl IntoElement {
    fixture(Mode::FeedError)
}

pub(super) fn start(root: Root) -> Headless {
    Headless::start(EngineConfig::new(
        root.tokens(
            nsh_base::tokens_of(Some(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/theme.json"
            ))))
            .unwrap(),
        ),
    ))
    .unwrap()
}
