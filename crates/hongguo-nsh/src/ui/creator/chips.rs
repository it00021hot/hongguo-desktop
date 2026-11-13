use nsh_base::theme;
use nsh_core::{element::ImageSrc, prelude::*};

#[derive(Clone, Default, PartialEq)]
pub(crate) struct CreatorChip {
    pub id: String,
    pub name: String,
    pub avatar: String,
    pub role: String,
}

pub(crate) fn creator_chip(data: Memo<CreatorChip>) -> nsh_base::ButtonElement {
    nsh_base::button()
        .accessibility_label(move || {
            Some(
                crate::i18n::tr("detail.creatorHome")
                    .replace("{name}", &data.with(|data| data.name.clone()))
                    .into(),
            )
        })
        .disabled(move || data.with(|data| data.id.is_empty()))
        .flex_row()
        .items_center()
        .gap_2()
        .rounded_full()
        .border_1()
        .border_color(move || theme().border)
        .pl_1()
        .pr_3()
        .py_1()
        .min_h(px(0.0))
        .bg(move || theme().background.opacity(0.0))
        .hover(|style| style.bg(move || theme().color("sidebar-accent").expect("sidebar accent")))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .transition()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                super::super::motion::FAST_MS
            }
        })
        .child(when(
            move || data.with(|data| !data.avatar.is_empty()),
            move || {
                image(move || ImageSrc::from(data.with(|data| data.avatar.clone())))
                    .size_7()
                    .rounded_full()
                    .object_fit(nsh_core::ResizeMode::Cover)
                    .accessible(false)
            },
        ))
        .child(when(
            move || data.with(|data| data.avatar.is_empty()),
            move || {
                view()
                    .size_7()
                    .rounded_full()
                    .bg(move || theme().muted)
                    .items_center()
                    .justify_center()
                    .child(
                        super::super::icon(super::super::Icon::Account)
                            .text_color(move || theme().muted_foreground)
                            .accessible(false),
                    )
            },
        ))
        .child(
            view()
                .items_start()
                .child(
                    text(move || data.with(|data| data.name.clone()))
                        .text_xs()
                        .line_height(px(15.0))
                        .font_weight(FontWeight::MEDIUM),
                )
                .child(when(
                    move || data.with(|data| !data.role.is_empty()),
                    move || {
                        text(move || data.with(|data| data.role.clone()))
                            .text_size(px(10.0))
                            .line_height(px(12.5))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(move || theme().muted_foreground)
                    },
                )),
        )
}
