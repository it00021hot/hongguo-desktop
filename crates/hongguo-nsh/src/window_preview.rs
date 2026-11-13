use crate::{
    navigation::Navigation,
    ui,
    window_modes::{PreviewDriver, WindowModes, provide_preview},
};
use nsh_base::theme;
use nsh_core::prelude::*;

/// Shares the real shell and window transport. Native commands are recorded and
/// explicitly acknowledged, so this fixture never changes its own OS window.
pub(super) fn preview() -> View {
    let driver: PreviewDriver = provide_preview();
    let modes = WindowModes::current().expect("window preview context");
    let geometry = ui::PlayerGeometry {
        source: signal((720.0, 1280.0)),
        viewport: signal((0.0, 0.0)),
    };
    provide_context(geometry);
    on_key("mod+alt+l", move |_| geometry.source.set((1280.0, 720.0)));
    let diagnostics = signal(true);
    on_key("mod+alt+m", move |_| modes.toggle_mini());
    on_key("mod+alt+p", move |_| modes.toggle_portrait(9.0 / 19.5));
    on_key("mod+alt+f", move |_| modes.toggle_fullscreen());
    on_key("mod+alt+b", move |_| modes.toggle_frameless());
    on_key("mod+alt+x", move |_| modes.exit());
    on_key("mod+alt+a", move |_| driver.acknowledge());
    on_key("mod+alt+d", move |_| {
        driver.fail("Offline system rejection")
    });
    on_key("mod+alt+v", move |_| {
        diagnostics.update(|visible| *visible = !*visible)
    });
    let stage = crate::player::stage::fixture_with_window_modes(
        nsh_core::element::ImageSrc::from(super::frame_path()),
        || {},
    )
    .test_id("window-fixture-stage");
    view()
        .flex_1()
        .child(ui::shell(Navigation::new(), view(), stage))
        .child(when(diagnostics, move || {
            view()
                .absolute()
                .top(px(48.0))
                .right(px(8.0))
                .pointer_events(PointerEvents::None)
                .test_id("window-preview-state")
                .bg(move || theme().background)
                .child(
                    text(move || {
                        let state = modes.state();
                        format!(
                            "window:{:?} pending:{} fill:{} frame:{} pinned:{} error:{}",
                            state.mode,
                            driver.pending(),
                            state.portrait_fill,
                            state.frameless,
                            state.pinned,
                            modes.error()
                        )
                    })
                    .text_color(move || theme().foreground)
                    .text_size(px(10.0)),
                )
        }))
}
