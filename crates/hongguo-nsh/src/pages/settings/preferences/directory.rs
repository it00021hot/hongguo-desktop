use super::model::Draft;
use crate::{i18n::tr, ui};
use nsh_core::{file_dialog::OpenDialog, prelude::*};

#[cfg(test)]
mod tests;

pub(super) fn field(
    label: &'static str,
    draft: Signal<Draft>,
    busy: Signal<bool>,
    message: Signal<SharedString>,
    external: bool,
    read: fn(&Draft) -> String,
    write: fn(&mut Draft, String),
) -> View {
    if !is_desktop() {
        let hint = if label == "settings.cacheDir" {
            "nsh.privateCacheDirectoryHint"
        } else {
            "nsh.privateDownloadDirectoryHint"
        };
        return view()
            .gap_1()
            .child(text(move || tr("nsh.privateDirectory")).text_base())
            .child(ui::form_description(move || tr(hint)));
    }
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            ui::directory_field(move || tr(label))
                .flex_1()
                .value(move || draft.with(read))
                .disabled(busy)
                .auto_correct(false)
                .on_change(move |value: &str| draft.update(|draft| write(draft, value.to_owned()))),
        )
        .child(
            ui::form_icon_action(move || tr("settings.choose"), ui::FormAction::Directory)
                .disabled(busy)
                .on_press(move || {
                    if !external {
                        message.set(
                            tr("nsh.previewAction")
                                .replace("{action}", &tr("settings.choose"))
                                .into(),
                        );
                        return;
                    }
                    busy.set(true);
                    let dialog = OpenDialog::folders()
                        .title(tr(label).to_string())
                        .button_label(tr("settings.choose").to_string());
                    spawn(async move {
                        match nsh_core::element::pick_paths(dialog).await {
                            Ok(Some(paths)) => {
                                if let Some(path) = paths.into_iter().next() {
                                    draft.update(|draft| {
                                        write(draft, path.to_string_lossy().into_owned())
                                    });
                                }
                            }
                            Ok(None) => {}
                            Err(error) => message.set(error.into()),
                        }
                        busy.set(false);
                    });
                }),
        )
}
