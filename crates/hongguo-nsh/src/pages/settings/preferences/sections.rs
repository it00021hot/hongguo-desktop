use super::{
    controls::{choices, group, toggle},
    directory,
    model::Draft,
};
use crate::{i18n::tr, ui};
use hongguo_common::model::settings::NamingTemplate;
use nsh_core::prelude::*;

pub(super) fn download(
    draft: Signal<Draft>,
    busy: Signal<bool>,
    message: Signal<SharedString>,
    external: bool,
) -> View {
    group("settings.downloadDir").child(directory::field(
        "settings.downloadDir",
        draft,
        busy,
        message,
        external,
        |draft| draft.download_dir.clone(),
        |draft, value| draft.download_dir = value,
    ))
}

pub(super) fn naming(draft: Signal<Draft>, busy: Signal<bool>) -> View {
    group("settings.naming").child(
        view()
            .gap_4()
            .child(
                view()
                    .gap_2()
                    .child(ui::form_label(move || tr("settings.naming")))
                    .child(choices(
                        "settings.naming",
                        draft,
                        busy,
                        vec![
                            (
                                NamingTemplate::TitleIndex,
                                "titleIndex",
                                "settings.namingTitleIndex",
                            ),
                            (
                                NamingTemplate::TitleIndexEpisode,
                                "titleIndexEpisode",
                                "settings.namingTitleIndexEpisode",
                            ),
                            (
                                NamingTemplate::OnlyTitle,
                                "onlyTitle",
                                "settings.namingOnlyTitle",
                            ),
                        ],
                        |draft| draft.naming,
                        |draft, value| draft.naming = value,
                    )),
            )
            .child(
                view()
                    .gap_2()
                    .child(ui::form_label(move || {
                        format!(
                            "{} — {}",
                            tr("settings.concurrency"),
                            draft.with(|draft| draft.concurrency.clone())
                        )
                    }))
                    .child(
                        ui::slider(move || tr("settings.concurrency"))
                            .min(1.0)
                            .max(10.0)
                            .step(1.0)
                            .disabled(busy)
                            .value(move || {
                                Some(ui::SliderValue::Single(draft.with(|draft| {
                                    draft
                                        .concurrency
                                        .parse::<f64>()
                                        .expect("validated slider value")
                                })))
                            })
                            .on_change(move |value: &ui::SliderValue| {
                                draft.update(|draft| {
                                    draft.concurrency = (value.end() as u32).to_string()
                                })
                            }),
                    )
                    .child(ui::form_description(move || tr("settings.concurrencyDesc"))),
            ),
    )
}

pub(super) fn playback(draft: Signal<Draft>, busy: Signal<bool>, extra: impl IntoElement) -> View {
    group("settings.playback").child(
        view()
            .gap_3()
            .child(toggle(
                "settings.autoNext",
                draft,
                busy,
                |draft| draft.auto_next,
                |draft, value| draft.auto_next = value,
            ))
            .child(toggle(
                "settings.autoDelete",
                draft,
                busy,
                |draft| draft.auto_delete,
                |draft, value| draft.auto_delete = value,
            ))
            .child(when(is_desktop, move || {
                view()
                    .gap_1p5()
                    .child(toggle(
                        "settings.pauseWhenHidden",
                        draft,
                        busy,
                        |draft| draft.pause_on_minimize,
                        |draft, value| draft.pause_on_minimize = value,
                    ))
                    .child(ui::form_description(move || {
                        tr("settings.pauseWhenHiddenDesc")
                    }))
            }))
            .child(extra),
    )
}
