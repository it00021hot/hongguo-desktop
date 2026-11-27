use nsh_base::theme;
use nsh_core::prelude::*;
use std::time::Duration;

pub(crate) fn switch_cue(target: Memo<Option<SharedString>>) -> View {
    let waiting = signal(false);
    effect(move || {
        let target = target.get();
        waiting.set(false);
        if target.is_some() {
            let timer = set_timeout(Duration::from_millis(220), move || waiting.set(true));
            cleanup(move || timer.cancel());
        }
    });
    view()
        .absolute()
        .top_4()
        .left_4()
        .right_4()
        .z_index(40)
        .items_center()
        .pointer_events(PointerEvents::None)
        .child(when(
            move || target.get().is_some(),
            move || {
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .max_w(percentage(1.0))
                    .px_3()
                    .py_2()
                    .rounded_full()
                    .bg(move || {
                        theme()
                            .color("video-surface")
                            .expect("video surface")
                            .opacity(0.82)
                    })
                    .text_color(move || theme().color("video-controls").expect("video controls"))
                    .child(when(
                        move || waiting.get(),
                        || spinner().size_3().accessible(false),
                    ))
                    .child(
                        text(move || {
                            let prefix = crate::i18n::tr("player.switching");
                            match target.get().filter(|label| !label.is_empty()) {
                                Some(label) => format!("{prefix}  {label}").into(),
                                None => prefix,
                            }
                        })
                        .text_sm()
                        .truncate(),
                    )
            },
        ))
}

pub(crate) fn spinner() -> nsh_core::element::Svg {
    svg(nsh_core::element::ImageSrc::svg_markup(include_str!(
        "icons/loader-circle.svg"
    )))
    .size_4()
    .fill_none()
    .stroke_current()
    .stroke_width(px(2.0))
    .animate_spin()
    .style_when(reduce_motion, |style| style.animate_none())
}

pub(crate) fn indicator(key: &'static str) -> View {
    view()
        .role(AccessibilityRole::Group)
        .label(move || crate::i18n::tr(key))
        .items_center()
        .justify_center()
        .min_h(px(32.0))
        .text_color(move || theme().muted_foreground)
        .child(spinner().accessible(false))
}

pub(crate) fn skeleton() -> View {
    view()
        .bg(move || theme().accent)
        .rounded(crate::ui::radii::control())
        .animate_pulse()
        .style_when(reduce_motion, |style| style.animate_none())
        .accessible(false)
}
