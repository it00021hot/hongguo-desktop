use super::{Intent, MergeView};
use crate::{
    i18n::{format_bytes, tr},
    ui,
};
use hongguo_common::model::{MergeStatus, MergeTask};
use nsh_core::prelude::*;

pub(super) fn content(model: MergeView, dispatch: Stored<impl Fn(Intent) + 'static>) -> View {
    ui::maintenance_card(ui::maintenance_title(move || tr("merge.taskList"))).child(
        virtual_list(
            move || model.snapshot.with(|snapshot| snapshot.tasks.len()),
            move |at| {
                let task = computed(move || {
                    model
                        .snapshot
                        .with(|snapshot| snapshot.tasks.get(at.index()).cloned())
                });
                row(model, task, dispatch)
            },
        )
        .test_id("merge-tasks")
        .viewport(nsh_core::ListViewport::Parent)
        .row_gap(px(8.0))
        .keys(move || {
            model.snapshot.with(|snapshot| {
                snapshot
                    .tasks
                    .iter()
                    .map(|task| task.id.clone())
                    .collect::<Vec<_>>()
            })
        })
        .estimated_item_size(move || px(if is_desktop() { 54.0 } else { 66.0 })),
    )
}

fn row(
    model: MergeView,
    task: Memo<Option<MergeTask>>,
    dispatch: Stored<impl Fn(Intent) + 'static>,
) -> View {
    let id = computed(move || {
        task.with(|task| {
            task.as_ref()
                .map(|task| task.id.clone())
                .unwrap_or_default()
        })
    });
    let title = computed(move || {
        task.with(|task| {
            task.as_ref()
                .map(|task| task.output_name.clone())
                .unwrap_or_default()
        })
    });
    let status = computed(move || task.with(|task| task.as_ref().map(|task| task.status)));
    let size = computed(move || task.with(|task| task.as_ref().map_or(0, |task| task.output_size)));
    let percent =
        computed(move || task.with(|task| task.as_ref().map_or(0.0, |task| task.percent)));
    let can_reveal = computed(move || {
        task.with(|task| {
            task.as_ref().is_some_and(|task| {
                task.status == MergeStatus::Completed && !task.output_path.is_empty()
            })
        })
    });
    ui::maintenance_row()
        .role(nsh_core::AccessibilityRole::Group)
        .label(move || title.get())
        .style_when(move || id.get().is_empty(), |style| style.hidden())
        .child(ui::maintenance_name(move || title.get()))
        .child(ui::maintenance_badge(
            move || {
                status
                    .get()
                    .map(|status| tr(super::super::model::status_key(status)))
                    .unwrap_or_default()
            },
            move || match status.get() {
                Some(MergeStatus::Completed) => ui::MaintenanceTone::Success,
                Some(MergeStatus::Failed) => ui::MaintenanceTone::Destructive,
                _ => ui::MaintenanceTone::Secondary,
            },
        ))
        .child(when(
            move || size.get() > 0,
            move || {
                ui::maintenance_bytes(move || format_bytes(size.get())).font_family("monospace")
            },
        ))
        .child(when(
            move || status.get() == Some(MergeStatus::Running),
            move || {
                view()
                    .w_28()
                    .flex_shrink_0()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        ui::maintenance_progress(move || percent.get() as f32)
                            .w_full()
                            .flex_shrink(1.0),
                    )
                    .child(
                        ui::maintenance_bytes(move || format!("{}%", percent.get().round()))
                            .w_9()
                            .flex_shrink(1.0)
                            .text_right()
                            .font_family("monospace"),
                    )
            },
        ))
        .child(when(can_reveal, move || {
            ui::maintenance_icon_action(
                move || {
                    tr(if is_desktop() {
                        "merge.actions.openOutput"
                    } else {
                        "nsh.exportFile"
                    })
                },
                ui::MaintenanceIcon::Folder,
            )
            .disabled(model.busy)
            .on_press(move || dispatch.with(|send| send(Intent::Reveal(id.get_untracked()))))
        }))
        .child(
            ui::maintenance_icon_action(
                move || tr("merge.actions.remove"),
                ui::MaintenanceIcon::Delete,
            )
            .disabled(move || model.busy.get() || model.confirming.get())
            .on_press(move || dispatch.with(|send| send(Intent::Remove(id.get_untracked())))),
        )
}
