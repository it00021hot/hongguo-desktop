use crate::i18n::tr;
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn comment_panel(
    open: Signal<bool>,
    total: Signal<i64>,
    geometry: super::PlayerGeometry,
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
                            .test_id("player-comments-panel")
                            .w(move || px(380.0_f32.min(geometry.viewport.get().0 * 0.85)))
                            .h(move || px(geometry.viewport.get().1 * 0.68))
                            .rounded_tl(px(12.0))
                            .border_t_1()
                            .border_l_1()
                            .border_color(move || {
                                theme()
                                    .color("video-controls")
                                    .expect("controls")
                                    .opacity(0.1)
                            })
                            .bg(move || theme().color("video-sheet").expect("sheet").opacity(0.95))
                            .text_color(move || theme().color("video-picker-text").expect("text"))
                            .shadow_2xl()
                            .child(
                                view()
                                    .h_12()
                                    .min_h(px(48.0))
                                    .flex_shrink_0()
                                    .px_4()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .border_b_1()
                                    .border_color(move || {
                                        theme()
                                            .color("video-controls")
                                            .expect("controls")
                                            .opacity(0.1)
                                    })
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
                                                .text_color(move || {
                                                    theme()
                                                        .color("video-picker-muted")
                                                        .expect("muted")
                                                })
                                        },
                                    ))
                                    .child(view().flex_1())
                                    .child(
                                        super::player_icon_button(super::Icon::Close, move || {
                                            tr("common.close")
                                        })
                                        .size_7()
                                        .min_h_0()
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
                super::sheet(
                    open,
                    move || tr("player.comments.title"),
                    move || (content.get())(),
                )
            },
        ))
}
