use super::{Intent, MergeView};
use crate::{
    i18n::{format_bytes, tr},
    ui,
};
use hongguo_common::model::MergeMode;
use nsh_core::prelude::*;

pub(super) fn content(model: MergeView, dispatch: impl Fn(Intent) + 'static) -> View {
    let dispatch = stored(dispatch);
    let output = computed(move || {
        model
            .snapshot
            .with(|snapshot| model.draft.output(&snapshot.candidates))
    });
    let mode = computed(move || {
        model
            .preflight
            .with(|preflight| model.draft.effective_mode(preflight.as_ref()))
    });
    let quick_unavailable = computed(move || {
        model
            .preflight
            .with(|preflight| super::super::model::quick_unavailable(preflight.as_ref()))
    });
    let unavailable = computed(move || model.busy.get() || model.confirming.get());
    let output_busy = computed(move || {
        model
            .snapshot
            .with(|snapshot| super::super::model::output_busy(&output.get(), &snapshot.tasks))
    });
    let options = computed(move || {
        model.snapshot.with(|snapshot| {
            snapshot
                .candidates
                .iter()
                .map(|candidate| {
                    ui::SelectOption::new(
                        candidate.series_id.clone(),
                        format!(
                            "{} · {}",
                            candidate.series_title,
                            tr("common.episodeCount")
                                .replace("{count}", &candidate.episode_count.to_string())
                        ),
                    )
                })
                .collect()
        })
    });
    ui::form(
        ui::maintenance_page()
            .child(
                ui::maintenance_card(ui::maintenance_title(move || tr("merge.title"))).child(
                    view()
                        .gap_4()
                        .child(
                            view()
                                .gap_2()
                                .child(ui::form_label(move || tr("merge.selectSeries")))
                                .child(
                                    view().test_id("merge-series").child(
                                        ui::maintenance_select(
                                            move || tr("merge.selectSeries"),
                                            options,
                                            computed(move || {
                                                (!model.draft.selected.get().is_empty())
                                                    .then(|| model.draft.selected.get().into())
                                            }),
                                            move || tr("merge.pickSeries"),
                                        )
                                        .disabled(move || {
                                            model.loading.get() || options.with(Vec::is_empty)
                                        })
                                        .on_change(
                                            move |value: &Option<SharedString>| {
                                                model
                                                    .draft
                                                    .selected
                                                    .set(value.as_deref().unwrap_or("").into())
                                            },
                                        ),
                                    ),
                                )
                                .child(when(
                                    move || !model.loading.get() && options.with(Vec::is_empty),
                                    move || ui::form_hint(move || tr("merge.noCandidates")),
                                )),
                        )
                        .child(
                            view()
                                .gap_2()
                                .child(ui::form_label(move || tr("merge.outputName")))
                                .child(
                                    ui::field(move || tr("merge.outputName"))
                                        .test_id("merge-output")
                                        .placeholder("")
                                        .value(output)
                                        .on_change(move |value: &str| {
                                            model.draft.output_name.set(value.into())
                                        }),
                                ),
                        )
                        .child(
                            view()
                                .gap_2()
                                .child(
                                    ui::maintenance_modes()
                                        .child(
                                            ui::maintenance_mode(
                                                move || tr("merge.quick"),
                                                ui::MaintenanceIcon::Quick,
                                            )
                                            .selected(move || mode.get() == MergeMode::Quick)
                                            .disabled(quick_unavailable)
                                            .on_press(
                                                move || {
                                                    model.draft.preferred_mode.set(MergeMode::Quick)
                                                },
                                            ),
                                        )
                                        .child(
                                            ui::maintenance_mode(
                                                move || tr("merge.compat"),
                                                ui::MaintenanceIcon::Compat,
                                            )
                                            .selected(move || mode.get() == MergeMode::Compat)
                                            .on_press(
                                                move || {
                                                    model
                                                        .draft
                                                        .preferred_mode
                                                        .set(MergeMode::Compat)
                                                },
                                            ),
                                        ),
                                )
                                .child(
                                    ui::form_hint(move || {
                                        tr(if mode.get() == MergeMode::Quick {
                                            "merge.quickDesc"
                                        } else {
                                            "merge.compatDesc"
                                        })
                                    })
                                    .pt_2(),
                                ),
                        )
                        .child(when(
                            move || model.preflight.with(Option::is_some),
                            move || preflight(model),
                        ))
                        .child(
                            ui::maintenance_action(
                                move || {
                                    tr(if output_busy.get() {
                                        "merge.running"
                                    } else {
                                        "merge.start"
                                    })
                                },
                                ui::MaintenanceIcon::Merge,
                            )
                            .disabled(move || {
                                unavailable.get()
                                    || output_busy.get()
                                    || model.draft.selected.get().is_empty()
                                    || output.get().is_empty()
                                    || !model.preflight.with(|preflight| {
                                        preflight
                                            .as_ref()
                                            .is_some_and(|preflight| preflight.episode_count > 0)
                                    })
                            })
                            .on_press(move || dispatch.with(|send| send(Intent::Start))),
                        ),
                ),
            )
            .child(when(
                move || model.snapshot.with(|snapshot| !snapshot.tasks.is_empty()),
                move || super::tasks::content(model, dispatch),
            ))
            .child(ui::status(model.message)),
    )
}

fn preflight(model: MergeView) -> View {
    ui::maintenance_alert(move || {
        model
            .preflight
            .with(|preflight| preflight.as_ref().is_some_and(|preflight| !preflight.ok))
    })
    .child(when(
        move || {
            model.preflight.with(|preflight| {
                preflight
                    .as_ref()
                    .is_some_and(|preflight| preflight.episode_count > 0)
            })
        },
        move || {
            text(move || {
                model.preflight.with(|preflight| {
                    preflight
                        .as_ref()
                        .map(|preflight| {
                            let mut text = format!(
                                "{} · {}",
                                tr("common.episodeCount")
                                    .replace("{count}", &preflight.episode_count.to_string()),
                                format_bytes(preflight.estimated_size)
                            );
                            if let Some(free) = preflight.free_space {
                                text.push_str(" · ");
                                text.push_str(
                                    &tr("merge.freeSpace").replace("{size}", &format_bytes(free)),
                                );
                            }
                            text
                        })
                        .unwrap_or_default()
                })
            })
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
        },
    ))
    .child(when(
        move || {
            model.preflight.with(|preflight| {
                preflight
                    .as_ref()
                    .is_some_and(|preflight| !preflight.warnings.is_empty())
            })
        },
        move || {
            ui::form_hint(move || {
                model.preflight.with(|preflight| {
                    preflight
                        .as_ref()
                        .map(|preflight| {
                            preflight
                                .warnings
                                .iter()
                                .map(|key| {
                                    tr(key).replace(
                                        "{episode}",
                                        &preflight.codec_mismatch_episode.unwrap_or(0).to_string(),
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("；")
                        })
                        .unwrap_or_default()
                })
            })
        },
    ))
}
