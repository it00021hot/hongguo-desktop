use super::{
    desktop::{Capture, Window},
    geometry::{Geometry, Plan},
    wire::Observation,
};
use nsh_core::{
    desktop::{WindowAttributes, WindowFrameStyle, WindowLevel, WindowMode, WindowPlacement},
    element::desktop::{WindowRef, displays},
    prelude::*,
};

pub(super) fn install(state: super::WindowModes) {
    let window = nsh_core::element::desktop::current_window();
    let outbox = stored(Vec::new());
    let controller = stored(super::desktop::Desktop::new(
        Box::new(FrameworkWindow {
            window: window.clone(),
            observation: state.observation,
            controls_hidden: state.native_controls_hidden,
            pinned: stored(false),
        }),
        move |event| outbox.with(|events| events.push(event)),
    ));
    let flush = move || {
        let events = outbox.with(std::mem::take);
        for event in events {
            state.desktop_event(event);
        }
    };
    effect(move || {
        if let Some(request) = state.request.get() {
            untracked(|| {
                controller.with(|controller| controller.request(request));
                flush();
            });
        }
    });
    effect(move || {
        let hidden = crate::boss_key::hidden();
        untracked(|| {
            controller.with(|controller| controller.boss(hidden));
            flush();
        });
    });
    effect(move || {
        let visible = state.mini_controls_visible.get();
        untracked(|| {
            controller.with(|controller| controller.set_mini_controls_visible(visible));
            flush();
        });
    });
    let tick = move || {
        untracked(|| {
            controller.with(super::desktop::Desktop::tick);
            flush();
        });
    };
    effect(move || {
        window.state();
        state.observation.get();
        tick();
    });
    effect(move || {
        if state.state.get().incognito || state.pending.get() {
            let interval = set_interval(
                std::time::Duration::from_secs_f64(super::desktop::TICK_SECONDS),
                tick,
            );
            cleanup(move || interval.cancel());
        }
    });
    cleanup(move || controller.with(super::desktop::Desktop::restore));
}

pub(super) struct FrameworkWindow {
    pub window: WindowRef,
    pub observation: Signal<Observation>,
    pub controls_hidden: Signal<bool>,
    pub pinned: Stored<bool>,
}

impl Window for FrameworkWindow {
    fn attached(&self) -> bool {
        untracked(|| self.window.state().is_some())
    }

    fn capture(&self) -> Result<Capture, String> {
        let state = untracked(|| self.window.state()).ok_or("window is not attached")?;
        let screens = untracked(displays);
        let screen = screens
            .iter()
            .find(|screen| Some(screen.id) == state.display)
            .or_else(|| screens.iter().find(|screen| screen.primary))
            .ok_or("window display is unavailable")?;
        Ok(Capture {
            geometry: Geometry {
                frame: state.placement.bounds.into(),
                work: screen.visible_bounds.into(),
                maximized: state.mode == WindowMode::Maximized,
                scale: f64::from(state.scale_factor),
            },
            placement: state.placement,
            fullscreen: state.mode == WindowMode::FullScreen,
            minimized: state.minimized,
            visible: state.visible,
            pointer_inside: self.observation.get_untracked().pointer_inside,
            pinned: self.pinned.get(),
        })
    }

    fn apply(&self, plan: Plan) -> Result<(), String> {
        let mut attributes = WindowAttributes::default();
        attributes.min_size = Some(Some(size(
            px(plan.minimum.0 as f32),
            px(plan.minimum.1 as f32),
        )));
        self.window.set(attributes);
        let mode = if plan.maximized {
            WindowMode::Maximized
        } else {
            WindowMode::Normal
        };
        self.window
            .set_placement(WindowPlacement::new(mode, plan.frame.into()));
        Ok(())
    }

    fn controls_hidden(&self, hidden: bool) -> Result<(), String> {
        if matches!(os(), nsh_core::platform::Os::Windows) {
            let mut attributes = WindowAttributes::default();
            attributes.frame = Some(if hidden {
                WindowFrameStyle::None
            } else {
                WindowFrameStyle::Default
            });
            self.window.set(attributes);
        } else {
            self.controls_hidden.try_set(hidden);
        }
        Ok(())
    }

    fn mini_controls_visible(&self, visible: bool) -> Result<(), String> {
        if matches!(os(), nsh_core::platform::Os::MacOs) {
            self.controls_hidden.try_set(!visible);
        }
        Ok(())
    }

    fn visible(&self, visible: bool, activate: bool) -> Result<(), String> {
        if !visible {
            self.window.hide();
        } else {
            let mut attributes = WindowAttributes::default();
            attributes.activate = Some(activate);
            self.window.set(attributes);
            if activate {
                self.window.focus();
            } else {
                self.window.show();
            }
        }
        Ok(())
    }

    fn pinned(&self, pinned: bool) -> Result<(), String> {
        let mut attributes = WindowAttributes::default();
        attributes.level = Some(if pinned {
            WindowLevel::Floating
        } else {
            WindowLevel::Normal
        });
        self.window.set(attributes);
        self.pinned.replace(pinned);
        Ok(())
    }
}
