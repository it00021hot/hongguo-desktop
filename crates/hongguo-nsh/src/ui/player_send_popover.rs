use nsh_base::PopoverEvent;
use nsh_core::prelude::*;

pub(crate) fn player_send_popover(
    open: Signal<bool>,
    content: impl Fn() -> View + 'static,
) -> View {
    let trigger = element_ref();
    view().flex_shrink_0().child(
        nsh_base::popover()
            .open(move || Some(open.get()))
            .placement(Placement::Top)
            .align(Align::Start)
            .offset(px(12.0))
            .margin(px(8.0))
            .on_open_change(move |event: &PopoverEvent| {
                if let PopoverEvent::OpenChange(next, _) = event {
                    open.set(*next);
                }
            })
            .on_close(move || trigger.focus())
            .trigger(
                view().pointer_events(PointerEvents::BoxNone).child(
                    super::video::player_surface()
                        .element_ref(&trigger)
                        .accessibility_label(move || crate::i18n::tr("player.interact.sendDanmaku"))
                        .size_8()
                        .p_0()
                        .on_press(move || open.update(|open| *open = !*open))
                        .child(super::player_hint(move || {
                            crate::i18n::tr("player.interact.sendDanmaku")
                        }))
                        .child(
                            view()
                                .relative()
                                .size_4()
                                .child(super::player_hint::danmaku_glyph())
                                .child(
                                    svg(nsh_core::element::ImageSrc::svg_markup(include_str!(
                                        "icons/danmaku-compose-pen.svg"
                                    )))
                                    .absolute()
                                    .right(px(-4.0))
                                    .bottom(px(-3.0))
                                    .size(px(10.0)),
                                ),
                        ),
                ),
            )
            .popup_style(|style| {
                super::player_popup::surface(style)
                    .p_3()
                    .rounded(super::radii::card())
                    .shadow_lg()
            })
            .child(when(open, move || {
                view().test_id("player-danmaku-send-popup").child(content())
            })),
    )
}
