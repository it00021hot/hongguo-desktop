use std::time::Duration;

use nsh_base::{theme, toast};
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
struct WindowNotice(Signal<SharedString>);

impl Default for WindowNotice {
    fn default() -> Self {
        Self(signal(SharedString::default()))
    }
}

pub(crate) fn window_notice() -> Signal<SharedString> {
    global::<WindowNotice>().get_untracked().0
}

/// Brief feedback centered above the content, independent of the page layout.
pub(crate) fn transient_notice(message: Signal<SharedString>) -> impl IntoElement {
    effect(move || {
        if !message.get().is_empty() {
            let timer = set_timeout(Duration::from_secs(2), move || {
                message.set(SharedString::default());
            });
            cleanup(move || timer.cancel());
        }
    });
    when(
        move || !message.get().is_empty(),
        move || {
            overlay()
                .absolute()
                .left_0()
                .right_0()
                .top(move || px(safe_area().top.0 + if is_desktop() { 56.0 } else { 12.0 }))
                .px_4()
                .items_center()
                .pointer_events(nsh_core::PointerEvents::None)
                .child(
                    toast()
                        .accessibility_label(move || Some(message.get()))
                        .max_w(move || px((window().width.0 - 32.0).clamp(0.0, 360.0)))
                        .px_4()
                        .py_3()
                        .border_1()
                        .border_color(move || theme().border)
                        .rounded(crate::ui::radii::control)
                        .bg(move || theme().popover)
                        .text_color(move || theme().popover_foreground)
                        .shadow_lg()
                        .child(
                            text(message)
                                .text_size(px(13.0))
                                .line_height(px(19.5))
                                .font_weight(FontWeight::MEDIUM),
                        ),
                )
        },
    )
}
