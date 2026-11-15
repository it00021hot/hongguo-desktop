use super::MetaData;
use crate::ui;
use nsh_core::prelude::*;

pub(super) fn content(meta: MetaData, open: impl Fn(String) + Copy + 'static) -> View {
    let creators = computed(move || {
        meta.with(|meta| {
            meta.map(|meta| {
                meta.creators
                    .iter()
                    .map(|creator| ui::CreatorChip {
                        id: creator.user_id.clone(),
                        name: creator.name.clone(),
                        avatar: crate::image_url::cdn_image_url(&creator.avatar),
                        role: creator.role.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
        })
    });
    view()
        .mt_4()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_3()
        .style_when(move || creators.with(Vec::is_empty), |style| style.hidden())
        .child(each(
            move || creators.get(),
            |creator| format!("{}:{}{}", creator.id.len(), creator.id, creator.name),
            move |creator, _| {
                ui::creator_chip(computed(move || creator.get())).on_press(move || {
                    let id = creator.with(|creator| creator.id.clone());
                    if !id.is_empty() {
                        open(id);
                    }
                })
            },
        ))
}
