//! Standalone desktop chrome, using NSH's public window contract only.
use nsh_core::{element::desktop, prelude::*};

pub(crate) fn leading_inset() -> Points {
    px(desktop::title_bar()
        .left
        .0
        .max(if matches!(os(), nsh_core::platform::Os::MacOs) {
            84.0
        } else {
            12.0
        }))
}

pub(crate) fn trailing_inset() -> Points {
    px(16.0 + desktop::title_bar().right.0)
}
