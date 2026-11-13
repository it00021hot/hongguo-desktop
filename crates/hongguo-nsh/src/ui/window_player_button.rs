use super::super::{Icon, icon, radii, video};
use nsh_base::{ButtonElement, theme};
use nsh_core::{element::ImageSrc, prelude::*};

pub(super) fn surface(label: impl IntoReactive<SharedString>) -> ButtonElement {
    let label = label.into_reactive();
    video::player_surface()
        .accessibility_label(move || Some(label.get()))
        .h_7()
        .min_h(px(28.0))
        .min_w(px(28.0))
        .px_1()
        .py_0()
        .rounded(radii::control)
        .text_color(move || {
            theme()
                .color("video-controls")
                .expect("video controls")
                .opacity(0.85)
        })
        .hover(|style| {
            style
                .bg(move || theme().color("video-controls-hover").expect("video hover"))
                .text_color(move || theme().color("video-controls").expect("video controls"))
        })
        .selected_style(|style| {
            style.text_color(move || theme().color("video-controls").expect("video controls"))
        })
}
pub(super) fn glyph(glyph: Icon) -> Svg {
    icon(glyph).size_4()
}
pub(super) fn special(markup: &'static str) -> Svg {
    svg(ImageSrc::svg_markup(markup)).size_4().stroke_current()
}

pub(super) const EXPAND: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h6v6"/><path d="m21 3-7 7"/><path d="m3 21 7-7"/><path d="M9 21H3v-6"/></svg>"#;
pub(super) const COLLAPSE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 3v3a2 2 0 0 1-2 2H3"/><path d="M21 8h-3a2 2 0 0 1-2-2V3"/><path d="M3 16h3a2 2 0 0 1 2 2v3"/><path d="M16 21v-3a2 2 0 0 1 2-2h3"/></svg>"#;
pub(super) const LANDSCAPE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="20" height="12" x="2" y="6" rx="2"/></svg>"#;
