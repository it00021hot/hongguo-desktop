use crate::i18n::tr;
use nsh_core::prelude::*;

pub(crate) fn copy_button(
    label_key: &'static str,
    value: impl Fn() -> String + Copy + 'static,
) -> View {
    let message = signal(SharedString::default());
    let pending = signal(false);
    let label = move || tr(label_key);
    view()
        .flex_shrink_0()
        .child(
            super::controls::surface_height(super::ButtonVariant::Quiet, 24.0)
                .size_6()
                .p_0()
                .accessibility_label(label)
                .disabled(move || pending.get())
                .child(super::icon(super::Icon::Copy).size_3p5().accessible(false))
                .child(super::hint(label, Placement::Top))
                .on_press(move || {
                    let value = value();
                    if value.is_empty() {
                        return;
                    }
                    pending.set(true);
                    message.set(SharedString::default());
                    spawn(async move {
                        message.set(copy_feedback(value).await);
                        pending.set(false);
                    });
                }),
        )
        .child(super::transient_notice(message))
}

pub(crate) async fn copy_feedback(value: String) -> SharedString {
    tr(match set_clipboard(value).await {
        ClipboardWrite::Written | ClipboardWrite::Requested => "common.copied",
        ClipboardWrite::Failed => "common.copyFailed",
    })
}
