use crate::i18n::tr;
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) const MOBILE_HEIGHT_RATIO: f32 = 0.64;

pub(crate) fn comment_panel(
    open: Signal<bool>,
    total: Signal<i64>,
    geometry: crate::features::player::presentation::PlayerGeometry,
    content: impl Fn() -> View + 'static,
) -> View {
    let content = stored(std::rc::Rc::new(content));
    let close = move || {
        open.set(false);
        geometry.comment_trigger.focus();
    };
    view()
        .pointer_events(PointerEvents::BoxNone)
        .child(when(
            move || is_desktop() && open.get(),
            move || {
                overlay()
                    .anchor(&geometry.comment_anchor)
                    .placement(Placement::Top)
                    .align(Align::End)
                    .offset(px(0.0))
                    .margin(px(0.0))
                    .on_cancel(close)
                    .child(
                        view()
                            .w(move || px(geometry.comment_width()))
                            .h(move || px(geometry.stage.get().1))
                            .bg(move || theme().card)
                            .text_color(move || theme().foreground)
                            .border_l_1()
                            .border_color(move || theme().border)
                            .child(
                                view()
                                    .h_12()
                                    .min_h(px(48.0))
                                    .flex_shrink_0()
                                    .px_4()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        text(move || tr("player.comments.title"))
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD),
                                    )
                                    .child(when(
                                        move || total.get() > 0,
                                        move || {
                                            text(move || total.get().to_string())
                                                .text_xs()
                                                .font_weight(FontWeight::NORMAL)
                                                .text_color(move || theme().muted_foreground)
                                        },
                                    ))
                                    .child(view().flex_1())
                                    .child(
                                        crate::ui::button("", crate::ui::ButtonVariant::Quiet)
                                            .accessibility_label(move || tr("common.close"))
                                            .size_8()
                                            .min_h_8()
                                            .p_0()
                                            .child(crate::ui::icon(crate::ui::Icon::Close))
                                            .on_press(close),
                                    ),
                            )
                            .child((content.get())()),
                    )
            },
        ))
        .child(when(
            move || !is_desktop(),
            move || {
                crate::ui::sheet(
                    open,
                    move || format!("{} {}", total.get(), tr("player.comments.title")),
                    move || (content.get())(),
                )
                .backdrop_style(|style| style.bg(move || theme().background.opacity(0.0)))
                .popup_style(|style| {
                    style
                        .h(move || px(window().height.0 * MOBILE_HEIGHT_RATIO))
                        .bg(move || theme().card)
                })
            },
        ))
}
