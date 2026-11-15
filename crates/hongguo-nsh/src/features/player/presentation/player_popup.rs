use nsh_base::theme;
use nsh_core::{element::StyleRefinement, prelude::*};

pub(crate) fn color(role: &str) -> AdaptiveColor {
    theme().color(role).expect("player popup semantic token")
}

pub(crate) fn surface(style: StyleRefinement) -> StyleRefinement {
    style
        .bg(move || color("video-picker"))
        .text_color(move || color("video-controls"))
        .border(px(0.0))
}
