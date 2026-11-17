//! Shared interaction colors. Features consume paired states rather than owning palettes.
use nsh_base::theme;
use nsh_core::{element::StyleRefinement, prelude::*};

pub(crate) fn selection_background() -> AdaptiveColor {
    theme().color("selection").expect("selection token")
}

pub(crate) fn selection_foreground() -> AdaptiveColor {
    theme()
        .color("selection-foreground")
        .expect("selection foreground token")
}

pub(crate) fn selected(style: StyleRefinement) -> StyleRefinement {
    style
        .bg(selection_background)
        .text_color(selection_foreground)
}

/// Keep selection legible while a selected item is hovered or keyboard focused.
pub(crate) fn highlighted(style: StyleRefinement) -> StyleRefinement {
    style
        .bg(move || theme().accent)
        .text_color(move || theme().accent_foreground)
        .selected(selected)
        .checked(selected)
        .aria_pressed(selected)
}

pub(crate) fn selected_label(style: StyleRefinement) -> StyleRefinement {
    style
        .bg(move || theme().background)
        .text_color(move || theme().primary)
}

pub(crate) fn highlighted_label(style: StyleRefinement) -> StyleRefinement {
    style
        .bg(move || theme().accent)
        .selected(selected_label)
        .checked(selected_label)
}

pub(crate) fn popup(style: StyleRefinement) -> StyleRefinement {
    style
        .bg(move || theme().popover)
        .text_color(move || theme().popover_foreground)
}

pub(crate) fn disabled(style: StyleRefinement) -> StyleRefinement {
    style
        .opacity(1.0)
        .bg(move || theme().muted)
        .text_color(move || theme().muted_foreground)
}
