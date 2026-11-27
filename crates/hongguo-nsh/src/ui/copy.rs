use crate::i18n::tr;
use nsh_core::prelude::*;
use std::time::Duration;

pub(crate) fn copy_button(
    label_key: &'static str,
    value: impl Fn() -> String + Copy + 'static,
) -> View {
    let outcome = signal(None::<ClipboardWrite>);
    let pending = signal(false);
    let label = move || {
        tr(match outcome.get() {
            Some(ClipboardWrite::Written | ClipboardWrite::Requested) => "common.copied",
            Some(ClipboardWrite::Failed) => "common.copyFailed",
            None => label_key,
        })
    };
    effect(move || {
        if outcome.get().is_some() {
            let timer = set_timeout(Duration::from_secs(2), move || outcome.set(None));
            cleanup(move || timer.cancel());
        }
    });
    view().flex_shrink_0().child(
        super::controls::surface_height(super::ButtonVariant::Quiet, 24.0)
            .size_6()
            .p_0()
            .accessibility_label(label)
            .disabled(move || pending.get())
            .child(dynamic(move || {
                let icon = if matches!(
                    outcome.get(),
                    Some(ClipboardWrite::Written | ClipboardWrite::Requested)
                ) {
                    super::Icon::Check
                } else {
                    super::Icon::Copy
                };
                super::icon(icon).size_3p5().accessible(false)
            }))
            .child(super::hint(label, Placement::Top))
            .on_press(move || {
                let value = value();
                if value.is_empty() {
                    return;
                }
                pending.set(true);
                spawn(async move {
                    outcome.set(Some(set_clipboard(value).await));
                    pending.set(false);
                });
            }),
    )
}
