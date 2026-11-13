use super::model::Draft;
use crate::{i18n::tr, ui};
use nsh_core::prelude::*;
use std::rc::Rc;

pub(super) fn group(title: &'static str) -> View {
    ui::form_card(move || tr(title))
}

pub(super) fn choices<T: Clone + PartialEq + 'static>(
    label: &'static str,
    draft: Signal<Draft>,
    busy: Signal<bool>,
    options: Vec<(T, &'static str, &'static str)>,
    read: fn(&Draft) -> T,
    write: fn(&mut Draft, T),
) -> ui::SelectControl {
    let options = Rc::new(options);
    let items = options.clone();
    let selected = options.clone();
    ui::select(
        move || tr(label),
        move || {
            items
                .iter()
                .map(|(_, id, key)| ui::SelectOption::new(*id, tr(key)))
                .collect::<Vec<_>>()
        },
        move || {
            let value = draft.with(read);
            selected
                .iter()
                .find(|(candidate, _, _)| *candidate == value)
                .map(|(_, id, _)| SharedString::from(*id))
        },
    )
    .disabled(busy)
    .on_change(move |value: &Option<SharedString>| {
        if let Some((next, _, _)) = options
            .iter()
            .find(|(_, id, _)| value.as_deref() == Some(*id))
        {
            draft.update(|draft| write(draft, next.clone()));
        }
    })
}

pub(super) fn toggle(
    label: &'static str,
    draft: Signal<Draft>,
    busy: Signal<bool>,
    read: fn(&Draft) -> bool,
    write: fn(&mut Draft, bool),
) -> impl IntoElement {
    ui::switch(move || tr(label))
        .checked(move || draft.with(read))
        .disabled(busy)
        .on_change(move |value: &bool| draft.update(|draft| write(draft, *value)))
}
