use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn player_hint(label: impl IntoReactive<SharedString>) -> nsh_base::TooltipElement {
    nsh_base::tooltip()
        .disabled(move || !is_desktop())
        .placement(Placement::Top)
        .px_2()
        .py_1()
        .rounded(crate::ui::radii::control())
        .bg(move || theme().popover)
        .text_color(move || theme().popover_foreground)
        .child(text(label).text_xs().font_weight(FontWeight::NORMAL))
}

pub(crate) fn player_danmaku_settings(open: Signal<bool>) -> nsh_base::ButtonElement {
    crate::features::player::presentation::video::player_surface()
        .accessibility_label(move || crate::i18n::tr("player.danmakuSettings"))
        .size(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .p_0()
        .text_color(move || super::video::player_ink().opacity(if open.get() { 1.0 } else { 0.7 }))
        .child(player_hint(move || {
            crate::i18n::tr("player.danmakuSettings")
        }))
        .child(
            view().relative().size_4().child(danmaku_glyph()).child(
                view()
                    .absolute()
                    .right(px(-4.0))
                    .bottom(px(-4.0))
                    .size(px(9.0))
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(move || {
                        theme()
                            .color("video-surface")
                            .expect("video surface")
                            .opacity(0.7)
                    })
                    .child(
                        crate::ui::icon(crate::ui::Icon::Settings)
                            .size(px(7.0))
                            .stroke_width(px(2.8)),
                    ),
            ),
        )
}

pub(crate) fn player_danmaku_toggle(enabled: Signal<bool>) -> nsh_base::ButtonElement {
    let label = computed(move || {
        crate::i18n::tr(if enabled.get() {
            "player.danmakuOff"
        } else {
            "player.danmakuOn"
        })
    });
    crate::features::player::presentation::video::player_surface()
        .accessibility_label(move || label.get())
        .selected(enabled)
        .size(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .p_0()
        .text_color(move || {
            super::video::player_ink().opacity(if enabled.get() { 1.0 } else { 0.4 })
        })
        .selected_style(|style| style.text_color(super::video::player_ink))
        .child(player_hint(move || label.get()))
        .child(
            view()
                .relative()
                .size_4()
                .child(danmaku_glyph())
                .child(when(
                    move || !enabled.get(),
                    || {
                        svg(nsh_core::element::ImageSrc::svg_markup(include_str!(
                            concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui/icons/danmaku-off.svg")
                        )))
                        .absolute()
                        .top(px(-1.0))
                        .left(px(-1.0))
                        .size(px(18.0))
                    },
                )),
        )
}

pub(crate) fn danmaku_glyph() -> View {
    view()
        .absolute()
        .inset_0()
        .items_center()
        .justify_center()
        .child(
            text("弹")
                .text_size(px(11.0))
                .line_height(px(11.0))
                .font_weight(FontWeight::SEMIBOLD),
        )
}
