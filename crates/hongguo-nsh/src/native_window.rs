//! Standalone desktop chrome, using NSH's public window contract only.
use nsh_core::{
    desktop::{TitleBarStyle, WindowFrame},
    element::desktop,
    prelude::*,
};

pub(crate) fn install() {
    if !is_desktop() {
        return;
    }
    let content = window();
    desktop::set_title_bar(TitleBarStyle::Hidden, Some(px(44.0)));
    // AppKit expands the content under its former title bar. Preserve the
    // requested viewport as the new outer frame, without guessing decorations.
    if matches!(os(), nsh_core::platform::Os::MacOs) {
        desktop::set_frame(WindowFrame {
            width: Some(content.width),
            height: Some(content.height),
            ..Default::default()
        });
    }
}

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
