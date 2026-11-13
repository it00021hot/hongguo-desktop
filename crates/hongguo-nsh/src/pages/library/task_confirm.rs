use crate::{
    i18n::{format_bytes as bytes, tr},
    ui,
};
use hongguo_common::model::{DownloadTask, TaskStatus};
use nsh_base::theme;
use nsh_core::prelude::*;

pub(super) fn build(
    items: Memo<Vec<DownloadTask>>,
    ids: Signal<Vec<String>>,
    open: Signal<bool>,
    with_files: Signal<bool>,
    remove: impl Fn(Vec<String>, bool) + Copy + 'static,
) -> impl IntoElement {
    let completed = computed(move || {
        items
            .get()
            .into_iter()
            .filter(|item| {
                ids.with(|ids| ids.contains(&item.id)) && item.status == TaskStatus::Completed
            })
            .collect::<Vec<_>>()
    });
    let freeable =
        computed(move || completed.with(|items| items.iter().map(|item| item.total).sum::<u64>()));
    ui::confirmation(
        open,
        move || -> SharedString {
            tr("tasks.removeConfirm")
                .replace("{count}", &ids.with(Vec::len).to_string())
                .into()
        },
        move || {
            tr(if with_files.get() {
                "tasks.removeAndFiles"
            } else {
                "tasks.removeOnly"
            })
        },
        move || {
            view()
                .gap_3()
                .child(
                    text(move || {
                        tr("tasks.removeSummary")
                            .replace("{count}", &ids.with(Vec::len).to_string())
                            .replace("{done}", &completed.with(Vec::len).to_string())
                            .replace("{size}", &bytes(freeable.get()))
                    })
                    .text_sm()
                    .line_height(px(20.0))
                    .text_color(move || theme().muted_foreground),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            ui::library_check(move || tr("tasks.removeFiles"), with_files, false)
                                .on_change(move |checked: &bool| with_files.set(*checked)),
                        )
                        .child(
                            text(move || {
                                if freeable.get() > 0 {
                                    tr("tasks.removeFilesWithSize")
                                        .replace("{size}", &bytes(freeable.get()))
                                        .into()
                                } else {
                                    tr("tasks.removeFiles")
                                }
                            })
                            .text_sm()
                            .line_height(px(14.0))
                            .text_color(move || theme().muted_foreground),
                        ),
                )
                .child(
                    text(move || tr("tasks.removeKeepHint"))
                        .text_sm()
                        .line_height(px(20.0))
                        .text_color(move || theme().muted_foreground),
                )
        },
        move || remove(ids.get_untracked(), with_files.get_untracked()),
    )
}
