use crate::{i18n::tr, ui};
use hongguo_service::library_service::BookshelfFolder;
use nsh_base::theme;
use nsh_core::prelude::*;

pub(super) fn summary(folder: &BookshelfFolder) -> SharedString {
    let count = folder
        .item_count
        .map(|count| tr("collections.folderItemCount").replace("{count}", &count.to_string()));
    let visibility = folder.published.map(|published| {
        tr(if published {
            "collections.folderPublic"
        } else {
            "collections.folderPrivate"
        })
        .to_string()
    });
    [count, visibility]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
        .into()
}

pub(super) fn card(
    folder: Memo<Option<BookshelfFolder>>,
    busy: Signal<bool>,
) -> nsh_base::ButtonElement {
    ui::controls::surface(ui::ButtonVariant::Quiet)
        .p_3()
        .items_stretch()
        .rounded(ui::radii::control)
        .bg(move || theme().card)
        .hover(|style| style.bg(move || theme().accent))
        .accessibility_label(move || {
            folder.with(|folder| folder.as_ref().map(|folder| folder.name.clone().into()))
        })
        .disabled(busy)
        .child(
            view()
                .w_full()
                .gap_3()
                .child(
                    view()
                        .flex_row()
                        .justify_between()
                        .gap_2()
                        .items_center()
                        .child(
                            text(move || {
                                folder.with(|f| {
                                    f.as_ref().map(|f| f.name.clone()).unwrap_or_default()
                                })
                            })
                            .font_weight(FontWeight::SEMIBOLD)
                            .number_of_lines(1),
                        )
                        .child(
                            text(move || {
                                folder.with(|f| f.as_ref().map(summary).unwrap_or_default())
                            })
                            .text_xs()
                            .text_color(move || theme().muted_foreground),
                        ),
                )
                .child(view().flex_row().gap_2().children((0..4).map(move |slot| {
                    let cover = computed(move || {
                        folder.with(|f| f.as_ref().and_then(|f| f.covers.get(slot).cloned()))
                    });
                    view()
                        .flex_1()
                        .h(move || px(if is_desktop() { 104.0 } else { 86.0 }))
                        .min_w(px(0.0))
                        .style_when(is_desktop, |style| style.max_w(px(78.0)))
                        .rounded(ui::radii::control)
                        .overflow_hidden()
                        .bg(move || theme().muted)
                        .child(when(
                            move || cover.with(Option::is_some),
                            move || {
                                image(move || {
                                    nsh_core::element::ImageSrc::from(
                                        cover.get().unwrap_or_default(),
                                    )
                                })
                                .size_full()
                                .object_fit(nsh_core::ResizeMode::Cover)
                            },
                        ))
                        .child(when(
                            move || cover.with(Option::is_none),
                            move || {
                                view()
                                    .size_full()
                                    .items_center()
                                    .justify_center()
                                    .child(text("+").text_color(move || theme().muted_foreground))
                            },
                        ))
                }))),
        )
}
