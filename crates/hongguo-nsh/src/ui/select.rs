use nsh_core::prelude::*;
mod presenter;

pub(crate) use nsh_base::SelectItem as SelectOption;
pub(crate) use presenter::ChoiceSelectElement as SelectControl;

pub(crate) fn select(
    label: impl IntoReactive<SharedString>,
    options: impl IntoReactive<Vec<SelectOption>>,
    value: impl IntoReactive<Option<SharedString>>,
) -> SelectControl {
    let label = label.into_reactive();
    let options = options.into_reactive();
    let value = value.into_reactive();
    presenter::choice_select()
        .label(move || label.get())
        .items(move || options.get())
        .value(move || value.get())
}
