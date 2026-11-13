use super::{ButtonVariant, button};
use crate::i18n::tr;
use nsh_base::{SheetEvent, theme};
use nsh_core::prelude::*;

const DESKTOP_WIDTH: f32 = 480.0;
const MOBILE_HEIGHT_RATIO: f32 = 0.82;
const BACKDROP_OPACITY: f32 = 0.8;
const CONTENT_PADDING: f32 = 16.0;

pub(crate) fn sheet<E: IntoElement>(
    open: Signal<bool>,
    title: impl IntoReactive<SharedString>,
    content: impl Fn() -> E + 'static,
) -> nsh_base::SheetElement {
    let title = title.into_reactive();
    nsh_base::sheet()
        .open(move || Some(open.get()))
        .side(move || {
            if is_desktop() {
                Placement::Right
            } else {
                Placement::Bottom
            }
        })
        .close_on_swipe(move || !is_desktop())
        .on_open_change(move |event: &SheetEvent| {
            if let SheetEvent::OpenChange(next, _) = event {
                open.set(*next);
            }
        })
        .backdrop_style(|style| style.bg(move || theme().background.opacity(BACKDROP_OPACITY)))
        .popup_style(|style| {
            style
                .w(move || {
                    px(if is_desktop() {
                        window().width.0.min(DESKTOP_WIDTH)
                    } else {
                        window().width.0
                    })
                })
                .h(move || {
                    px(window().height.0
                        * if is_desktop() {
                            1.0
                        } else {
                            MOBILE_HEIGHT_RATIO
                        })
                })
                .rounded(move || theme().radius)
                .border_1()
                .border_color(move || theme().border)
                .bg(move || theme().popover)
                .text_color(move || theme().popover_foreground)
        })
        .child(dynamic(move || {
            let label = title.clone();
            view().flex_1().pb_keyboard().child(
                view()
                    .flex_1()
                    .gap_3()
                    .pt(move || {
                        px(CONTENT_PADDING + if is_desktop() { safe_area().top.0 } else { 0.0 })
                    })
                    .pr_safe_area_plus(px(CONTENT_PADDING))
                    .pb_safe_area_plus(px(CONTENT_PADDING))
                    .pl_safe_area_plus(px(CONTENT_PADDING))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .child(
                                text(move || label.get())
                                    .flex_1()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(
                                button(move || tr("common.close"), ButtonVariant::Quiet)
                                    .on_press(move || open.set(false)),
                            ),
                    )
                    .child(content()),
            )
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nsh_engine::{EngineConfig, Root};
    use nsh_testing::Headless;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        time::Duration,
    };

    #[derive(Default)]
    struct Layout {
        parents: HashMap<nsh_core::NodeId, nsh_core::NodeId>,
        frames: HashMap<nsh_core::NodeId, Bounds<Points>>,
        top_border: HashMap<nsh_core::NodeId, f32>,
    }

    impl Layout {
        fn top(&self, mut node: nsh_core::NodeId) -> f32 {
            let mut top = 0.0;
            loop {
                top += self.frames.get(&node).map_or(0.0, |frame| frame.origin.y.0);
                let Some(parent) = self.parents.get(&node) else {
                    return top;
                };
                node = *parent;
            }
        }
    }

    #[component]
    fn Panel() -> impl IntoElement {
        let open = signal(false);
        let loads = signal(0);
        view()
            .flex_1()
            .child(button("Open panel", ButtonVariant::Secondary).on_press(move || open.set(true)))
            .child(text(move || format!("loads:{}", loads.get())))
            .child(sheet(open, "Test panel", move || {
                loads.update(|count| *count += 1);
                text("Lazy panel body")
            }))
    }

    fn start() -> Headless {
        let mut config = EngineConfig::new(Root::of::<Panel>());
        config.environment.reduce_motion = true;
        Headless::start(config).unwrap()
    }

    #[test]
    fn the_panel_only_constructs_its_content_while_open() -> Result<(), String> {
        let mut app = start();
        app.wait_for("text:loads:0")?;
        assert!(!app.shows("Lazy panel body"));
        app.press("Open panel")?;
        app.wait_for("Lazy panel body")?;
        app.wait_for("text:loads:1")?;
        app.press("role:button[Close]")?;
        app.wait_for_gone("Lazy panel body", Duration::from_secs(1))?;
        app.press("Open panel")?;
        app.wait_for("text:loads:2")?;
        Ok(())
    }

    #[test]
    fn escape_dismisses_the_panel_and_updates_the_controlled_value() -> Result<(), String> {
        let mut app = start();
        app.wait_for("Open panel")?;
        app.press("Open panel")?;
        app.wait_for("Lazy panel body")?;
        app.press_key("Escape")?;
        app.wait_for_gone("Lazy panel body", Duration::from_secs(1))?;
        app.press("Open panel")?;
        app.wait_for("text:loads:2")?;
        Ok(())
    }

    #[test]
    fn bottom_sheet_header_does_not_repeat_the_screen_top_safe_area() -> Result<(), String> {
        const TOP_INSET: f32 = 59.0;
        const HEIGHT: f32 = 840.0;
        for os in [
            nsh_core::platform::Os::Ios,
            nsh_core::platform::Os::Android,
            nsh_core::platform::Os::MacOs,
        ] {
            let mut config = EngineConfig::new(Root::of::<Panel>());
            config.environment.os = os;
            config.environment.safe_area.top = px(TOP_INSET);
            config.environment.reduce_motion = true;
            config.viewport = nsh_engine::Size::new(px(480.0), px(HEIGHT));
            let observed = Arc::new(Mutex::new(Layout::default()));
            let layout = observed.clone();
            let mut app = Headless::start(config).map_err(|error| error.to_string())?;
            app.observe(move |_, batch| {
                let mut layout = observed.lock().unwrap();
                for item in &batch.items {
                    match item {
                        nsh_core::MountItem::Insert { parent, child, .. } => {
                            layout.parents.insert(*child, *parent);
                        }
                        nsh_core::MountItem::Frame { id, frame } => {
                            layout.frames.insert(*id, *frame);
                        }
                        nsh_core::MountItem::ViewStyle { id, style } => {
                            layout.top_border.insert(*id, style.borders.widths.top.0);
                        }
                        _ => {}
                    }
                }
            });
            app.wait_for("Open panel")?;
            app.press("Open panel")?;
            app.wait_for("Lazy panel body")?;
            let close = app
                .screen()
                .press_target("role:button[Close]")?
                .expect("real sheet close button");
            let layout = layout.lock().unwrap();
            let header = layout.parents[&close];
            let mut popup = header;
            while !layout
                .top_border
                .get(&popup)
                .is_some_and(|width| *width > 0.0)
            {
                popup = *layout
                    .parents
                    .get(&popup)
                    .expect("header belongs to the actual sheet popup");
            }
            assert!(layout.frames.contains_key(&popup));
            let padding = layout.top(header) - layout.top(popup) - layout.top_border[&popup];
            let expected = CONTENT_PADDING
                + if os == nsh_core::platform::Os::MacOs {
                    TOP_INSET
                } else {
                    0.0
                };
            assert!(
                (padding - expected).abs() < 0.1,
                "{os:?}: actual header inset {padding}, expected {expected}"
            );
        }
        Ok(())
    }
}
