use nsh_core::{element::StyleRefinement, prelude::*};

pub(crate) fn surface(style: StyleRefinement) -> StyleRefinement {
    crate::ui::palette::popup(style).border(px(0.0))
}
