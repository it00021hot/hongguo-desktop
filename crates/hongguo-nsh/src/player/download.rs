mod range;
#[cfg(test)]
mod tests;

use crate::{
    bridge,
    i18n::tr,
    navigation::{Navigation, Route},
    pages::SessionHandle,
    ui,
};
use nsh_core::prelude::*;
use std::collections::BTreeSet;

#[derive(Clone, PartialEq)]
struct Target {
    id: String,
    indices: Vec<u32>,
    current: u32,
}

pub(super) fn sheet(
    session: SessionHandle,
    nav: Navigation,
    open: Signal<bool>,
    source: Resource<super::prepare::Playback, bridge::Failure>,
) -> impl IntoElement {
    let target = computed(move || {
        source.with(|source| {
            source.map(|source| Target {
                id: source.history.series_id.clone(),
                indices: source.episode_indices.clone(),
                current: source.history.vid_index,
            })
        })
    });
    presenter(open, target, move |id, indices, busy, message| {
        let state = session.with(|session| session.state.clone());
        let identity = session.identity();
        spawn(async move {
            let outcome = bridge::request(async move {
                hongguo_service::runtime::spawn_blocking(move || {
                    hongguo_service::download_service::actions::enqueue_batch(&state, &id, &indices)
                })
                .await
                .map_err(|error| hongguo_common::AppError::Internal(error.to_string()))?
            })
            .await;
            busy.set(false);
            if session.identity() != identity {
                return;
            }
            match outcome {
                Ok(_) => {
                    open.set(false);
                    session.notify_downloads();
                    nav.push(Route::Tasks);
                }
                Err(error) => message.set(error.text()),
            }
        });
    })
}

pub(super) fn fixture(open: Signal<bool>, current: Signal<u32>) -> impl IntoElement {
    presenter(
        open,
        computed(move || {
            Some(Target {
                id: "preview".into(),
                indices: (1..=80).collect(),
                current: current.get(),
            })
        }),
        move |_, _, busy, _| {
            busy.set(false);
            open.set(false);
        },
    )
}

fn presenter(
    open: Signal<bool>,
    target: Memo<Option<Target>>,
    submit: impl Fn(String, Vec<u32>, Signal<bool>, Signal<SharedString>) + 'static,
) -> impl IntoElement {
    let submit = stored(submit);
    ui::download_sheet(open, move || {
        let target = target.get_untracked();
        let selected = signal(
            target
                .as_ref()
                .map(|target| BTreeSet::from([target.current]))
                .unwrap_or_default(),
        );
        let target = stored(target);
        let busy = signal(false);
        let message = signal(SharedString::default());
        let range_text = signal(String::new());
        let invalid = signal(false);
        let apply = move |input: String| {
            let parsed = target.with(|target| {
                target
                    .as_ref()
                    .map(|target| range::parse_range(&input, &target.indices))
                    .unwrap_or_default()
            });
            let error = !input.trim().is_empty() && parsed.is_empty();
            range_text.set(input);
            invalid.set(error);
            if !error {
                selected.set(parsed.into_iter().collect());
            }
        };
        view()
            .gap_4()
            .child(
                view()
                    .py_3()
                    .gap_4()
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(ui::form_label(move || tr("download.selectEpisodes")))
                            .child(ui::download_count(move || selected.with(BTreeSet::len))),
                    )
                    .child(view().flex_row().flex_wrap().gap_2().child(each(
                        || {
                            vec![
                                ("download.selectAll", range::Preset::All, true),
                                ("download.invert", range::Preset::Invert, true),
                                ("download.clear", range::Preset::Clear, true),
                                ("download.first10", range::Preset::FirstTen, false),
                                ("download.first30", range::Preset::FirstThirty, false),
                                ("download.last30", range::Preset::LastThirty, false),
                            ]
                        },
                        |item| item.0,
                        move |item, _| {
                            let (label, preset, outline) = item.get_untracked();
                            ui::download_quick(move || tr(label), outline)
                                .disabled(busy)
                                .on_press(move || {
                                    target.with(|target| {
                                        if let Some(target) = target {
                                            selected.set(range::preset(
                                                preset,
                                                &target.indices,
                                                &selected.get_untracked(),
                                            ));
                                        }
                                    })
                                })
                        },
                    )))
                    .child(
                        view()
                            .gap_1p5()
                            .child(ui::form_label(move || tr("download.rangeLabel")))
                            .child(
                                view()
                                    .flex_row()
                                    .gap_2()
                                    .child(
                                        ui::field(move || tr("download.rangePlaceholder"))
                                            .flex_1()
                                            .test_id("download-range")
                                            .font_family("monospace")
                                            .value(range_text)
                                            .disabled(busy)
                                            .on_change(move |value: &str| apply(value.to_owned())),
                                    )
                                    .child(when(
                                        move || !selected.with(BTreeSet::is_empty),
                                        move || {
                                            ui::download_quick("", false)
                                                .w_9()
                                                .px_0()
                                                .accessibility_label(move || {
                                                    Some(tr("download.formatSelection"))
                                                })
                                                .child(ui::icon(ui::Icon::Check))
                                                .disabled(busy)
                                                .on_press(move || {
                                                    range_text.set(range::format_range(
                                                        &selected
                                                            .get_untracked()
                                                            .into_iter()
                                                            .collect::<Vec<_>>(),
                                                    ))
                                                })
                                        },
                                    )),
                            )
                            .child(when(invalid, ui::download_range_error)),
                    )
                    .child(
                        ui::download_grid().child(view().grid().grid_cols(8_u16).gap_1().child(
                            each(
                                move || {
                                    target.with(|target| {
                                        target
                                            .as_ref()
                                            .map(|target| target.indices.clone())
                                            .unwrap_or_default()
                                    })
                                },
                                |index| *index,
                                move |index, _| {
                                    let index = index.get_untracked();
                                    ui::download_episode(index, move || {
                                        selected.with(|selected| selected.contains(&index))
                                    })
                                    .disabled(busy)
                                    .on_press(move || {
                                        selected.update(|selected| {
                                            if !selected.remove(&index) {
                                                selected.insert(index);
                                            }
                                        })
                                    })
                                },
                            ),
                        )),
                    )
                    .child(when(
                        move || !message.get().is_empty(),
                        move || ui::status(message),
                    )),
            )
            .child(
                view()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .pb_2()
                    .child(ui::download_cancel().on_press(move || open.set(false)))
                    .child(
                        ui::download_submit()
                            .disabled(move || busy.get() || selected.with(BTreeSet::is_empty))
                            .on_press(move || {
                                if busy.get_untracked() {
                                    return;
                                }
                                if let Some(target) = target.get() {
                                    busy.set(true);
                                    message.set(SharedString::default());
                                    submit.with(|submit| {
                                        submit(
                                            target.id,
                                            selected.get_untracked().into_iter().collect(),
                                            busy,
                                            message,
                                        )
                                    });
                                }
                            }),
                    ),
            )
    })
}
