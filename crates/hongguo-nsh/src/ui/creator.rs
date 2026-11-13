use super::{ButtonVariant, Icon, controls::surface, icon};
use crate::i18n::tr;
use nsh_base::theme;
use nsh_core::{element::ImageSrc, prelude::*};

mod cards;
mod chips;
pub(crate) use cards::{CreatorCard, creator_grid, creator_skeleton_grid};
pub(crate) use chips::{CreatorChip, creator_chip};

#[derive(Clone, Default, PartialEq)]
pub(crate) struct CreatorProfileData {
    pub name: String,
    pub avatar: String,
    pub description: String,
}

pub(crate) fn creator_page() -> View {
    view()
        .w_full()
        .max_w(px(1152.0))
        .self_center()
        .px_8()
        .py_6()
}

pub(crate) fn creator_back() -> nsh_base::ButtonElement {
    surface(ButtonVariant::Quiet)
        .min_h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .px_2()
        .py_1()
        .ml(px(-8.0))
        .self_start()
        .flex_row()
        .gap_1p5()
        .text_color(move || theme().muted_foreground)
        .child(icon(Icon::Back))
        .child(
            text(move || tr("detail.back"))
                .text_sm()
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn creator_profile(data: Memo<CreatorProfileData>) -> View {
    view()
        .test_id("creator-profile")
        .mt_4()
        .flex_row()
        .items_center()
        .gap_4()
        .child(when(
            move || data.with(|data| !data.avatar.is_empty()),
            move || {
                image(move || ImageSrc::from(data.with(|data| data.avatar.clone())))
                    .size_16()
                    .flex_shrink_0()
                    .rounded_full()
                    .object_fit(nsh_core::ResizeMode::Cover)
                    .accessible(false)
            },
        ))
        .child(
            view()
                .min_w(px(0.0))
                .flex_1()
                .child(
                    text(move || data.with(|data| data.name.clone()))
                        .truncate()
                        .text_xl()
                        .font_weight(FontWeight::BOLD),
                )
                .child(when(
                    move || data.with(|data| !data.description.is_empty()),
                    move || {
                        text(move || data.with(|data| data.description.clone()))
                            .mt_1()
                            .text_sm()
                            .font_weight(FontWeight::NORMAL)
                            .text_color(move || theme().muted_foreground)
                            .number_of_lines(2)
                    },
                )),
        )
}

pub(crate) fn creator_profile_skeleton() -> View {
    view()
        .test_id("creator-profile-loading")
        .mt_4()
        .flex_row()
        .items_center()
        .gap_4()
        .child(super::browse_loading::skeleton().size_16().rounded_full())
        .child(
            view()
                .gap_2()
                .child(super::browse_loading::skeleton().h_6().w_36())
                .child(super::browse_loading::skeleton().h_4().w_64()),
        )
}

pub(crate) fn creator_works_heading(total: impl IntoReactive<i64>) -> Text {
    let total = total.into_reactive();
    text(move || tr("creator.works").replace("{count}", &total.get().to_string()))
        .mt_6()
        .text_base()
        .font_weight(FontWeight::SEMIBOLD)
}

pub(crate) fn creator_empty() -> Text {
    text(move || tr("creator.empty"))
        .mt_6()
        .text_sm()
        .font_weight(FontWeight::NORMAL)
        .text_color(move || theme().muted_foreground)
}

pub(crate) fn creator_loading_more() -> View {
    view()
        .mt_3()
        .py_2()
        .items_center()
        .justify_center()
        .role(nsh_core::AccessibilityRole::Progressbar)
        .label(move || tr("common.loading"))
        .child(
            super::browse_loading::browse_spinner()
                .size_5()
                .text_color(move || theme().muted_foreground)
                .accessible(false),
        )
}
