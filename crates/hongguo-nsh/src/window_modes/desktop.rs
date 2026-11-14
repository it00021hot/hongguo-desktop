use super::{
    Mode,
    geometry::{Geometry, Modes, Plan},
    privacy::{Effect, Privacy},
    wire::{Action, Request, Response, State},
};
use serde_json::Value;
use std::time::{Duration, Instant};

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod tests;

pub(super) const TICK_SECONDS: f64 = 0.04;
const CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(3);
const VISIBILITY_INTERVAL: Duration = Duration::from_millis(250);
type EventSink = Box<dyn Fn(&str, Value)>;

#[derive(Clone)]
struct Pending {
    request: Request,
    fullscreen: bool,
    deadline: Instant,
}

pub(super) struct Capture {
    pub geometry: Geometry,
    pub fullscreen: bool,
    pub minimized: bool,
    pub pointer_inside: Option<bool>,
    pub pinned: bool,
}
pub(super) trait Window {
    fn attached(&self) -> bool;
    fn exposed(&self) -> Result<bool, String>;
    fn capture(&self) -> Result<Capture, String>;
    fn apply(&self, plan: Plan) -> Result<(), String>;
    fn controls_hidden(&self, hidden: bool) -> Result<(), String>;
    fn visible(&self, visible: bool, activate: bool) -> Result<(), String>;
    fn pinned(&self, pinned: bool) -> Result<(), String>;
}

pub(super) struct Desktop {
    window: Box<dyn Window>,
    emit: EventSink,
    modes: Modes,
    privacy: Privacy,
    frameless: bool,
    boss: bool,
    privacy_paused: bool,
    start: Instant,
    pending: Option<Pending>,
    last: Option<State>,
    observe_visibility: bool,
    last_visibility: Option<bool>,
    visibility_checked_at: Option<Instant>,
}

impl Desktop {
    pub fn new(window: Box<dyn Window>, emit: impl Fn(&str, Value) + 'static) -> Self {
        Self {
            window,
            emit: Box::new(emit),
            modes: Modes::default(),
            privacy: Privacy::default(),
            frameless: false,
            boss: false,
            privacy_paused: false,
            start: Instant::now(),
            pending: None,
            last: None,
            observe_visibility: false,
            last_visibility: None,
            visibility_checked_at: None,
        }
    }
    pub fn observe_visibility(&mut self, enabled: bool) {
        if self.observe_visibility == enabled {
            return;
        }
        self.observe_visibility = enabled;
        log::debug!("[NSH visibility] observing={enabled}");
        self.visibility_checked_at = None;
        if !enabled && self.last_visibility.take() == Some(false) {
            (self.emit)("visibility", Value::Bool(true));
        }
    }
    fn sample_visibility(&mut self) {
        if !self.observe_visibility
            || self
                .visibility_checked_at
                .is_some_and(|checked| checked.elapsed() < VISIBILITY_INTERVAL)
        {
            return;
        }
        self.visibility_checked_at = Some(Instant::now());
        match self.window.exposed() {
            Ok(exposed) if self.last_visibility != Some(exposed) => {
                log::debug!("[NSH visibility] exposed={exposed}");
                self.last_visibility = Some(exposed);
                (self.emit)("visibility", Value::Bool(exposed));
            }
            Ok(_) => {}
            Err(error) => self.report(error),
        }
    }
    pub fn request(&mut self, request: Request) {
        let result = self.execute(&request);
        match result {
            Ok(true) => self.respond(request.serial, None),
            Ok(false) => {}
            Err(error) => self.respond(request.serial, Some(error)),
        }
    }
    fn execute(&mut self, request: &Request) -> Result<bool, String> {
        let capture = self.window.capture()?;
        let needs_windowed = matches!(
            request.action,
            Action::SetMode { .. }
                | Action::SetPortraitFill { .. }
                | Action::Refit { .. }
                | Action::SetFrameless { enabled: true }
                | Action::Exit
        );
        if needs_windowed && capture.fullscreen {
            self.wait_for_fullscreen(request, false);
            (self.emit)("exitFullscreen", Value::Null);
            return Ok(false);
        }
        let mut next = self.modes.clone();
        let plan = match request.action {
            Action::SetMode { mode, aspect } => next.enter(mode, capture.geometry, aspect)?,
            Action::SetPortraitFill { fill, aspect } => {
                next.set_fill(capture.geometry, fill, aspect)?
            }
            Action::Refit { aspect } => next.refit(capture.geometry, aspect)?,
            Action::Exit => next.enter(Mode::Normal, capture.geometry, super::PHONE_ASPECT)?,
            _ => None,
        };
        if let Some(plan) = plan {
            self.window.apply(plan)?;
        }
        let frameless = match request.action {
            Action::SetFrameless { enabled } => enabled,
            Action::Exit | Action::ToggleFullscreen => false,
            _ => self.frameless,
        };
        self.window
            .controls_hidden(frameless || next.mode == Mode::Mini)?;
        self.modes = next;
        self.frameless = frameless;
        match request.action {
            Action::SetIncognito { enabled } => self.privacy.enabled = enabled,
            Action::SetPinned { enabled } => self.window.pinned(enabled)?,
            Action::SetLandscape { .. } => {
                return Err("screen orientation is a mobile host operation".into());
            }
            Action::ToggleFullscreen => {
                self.wait_for_fullscreen(request, !capture.fullscreen);
                (self.emit)("toggleFullscreen", Value::Null);
                return Ok(false);
            }
            _ => {}
        }
        Ok(true)
    }
    fn wait_for_fullscreen(&mut self, request: &Request, fullscreen: bool) {
        self.pending = Some(Pending {
            request: request.clone(),
            fullscreen,
            deadline: Instant::now() + CONFIRMATION_TIMEOUT,
        });
    }
    pub fn boss(&mut self, hidden: bool) {
        if self.boss == hidden {
            return;
        }
        self.boss = hidden;
        if hidden {
            if let Err(error) = self.window.visible(false, false) {
                self.report(error);
            }
        } else {
            self.tick();
            if !self.privacy_paused
                && let Err(error) = self.window.visible(true, true)
            {
                self.report(error);
            }
        }
        self.publish();
    }
    pub fn tick(&mut self) {
        if let Some(pending) = &self.pending
            && Instant::now() >= pending.deadline
        {
            let pending = self.pending.take().expect("pending window operation");
            self.respond(
                pending.request.serial,
                Some("The system did not confirm the requested window mode".into()),
            );
        }
        self.sample_visibility();
        let Ok(capture) = self.window.capture() else {
            return;
        };
        if self
            .last
            .is_some_and(|state| state.fullscreen && !capture.fullscreen)
            && let Err(error) = self
                .window
                .controls_hidden(self.frameless || self.modes.mode == Mode::Mini)
        {
            self.report(error);
        }
        if let Some(pending) = self.pending.clone()
            && capture.fullscreen == pending.fullscreen
        {
            self.pending = None;
            if matches!(pending.request.action, Action::ToggleFullscreen) {
                self.respond(pending.request.serial, None);
            } else {
                self.request(pending.request);
            }
        }
        if let Some(inside) = capture.pointer_inside {
            for effect in self.privacy.sample(
                self.start.elapsed().as_millis() as u64,
                inside,
                capture.minimized,
                self.boss,
            ) {
                let result = match effect {
                    Effect::Pause => {
                        self.privacy_paused = true;
                        self.publish();
                        Ok(())
                    }
                    Effect::Hide => self.window.visible(false, false),
                    Effect::Show => {
                        self.privacy_paused = false;
                        self.window.visible(true, false)
                    }
                };
                if let Err(error) = result {
                    self.report(error);
                }
            }
        }
        self.publish();
    }
    /// Restore only the window retained by this component; cleanup never emits UI events.
    pub fn restore(&mut self) {
        self.pending = None;
        if !self.window.attached() {
            return;
        }
        let result = (|| {
            let capture = self.window.capture()?;
            if !capture.fullscreen {
                if let Some(plan) =
                    self.modes
                        .enter(Mode::Normal, capture.geometry, super::PHONE_ASPECT)?
                {
                    self.window.apply(plan)?;
                }
                self.window.controls_hidden(false)?;
            }
            if self.privacy_paused || self.boss {
                self.window.visible(true, false)?;
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            log::warn!("[NSH window] cleanup failed: {error}");
        }
    }
    fn snapshot(&self) -> State {
        let capture = self.window.capture().ok();
        State {
            mode: self.modes.mode,
            portrait_fill: self.modes.fill,
            frameless: self.frameless,
            fullscreen: capture.as_ref().is_some_and(|c| c.fullscreen),
            maximized: capture.as_ref().is_some_and(|c| c.geometry.maximized),
            incognito: self.privacy.enabled,
            visible: !self.privacy_paused && !self.boss,
            landscape: false,
            pinned: capture.as_ref().is_some_and(|c| c.pinned),
        }
    }
    fn publish(&mut self) {
        let state = self.snapshot();
        if self.last != Some(state) {
            self.last = Some(state);
            (self.emit)("state", serde_json::to_value(state).expect("window state"));
        }
    }
    fn respond(&mut self, serial: u32, error: Option<String>) {
        if let Some(error) = &error {
            log::warn!("[NSH window] command failed: {error}");
        }
        let state = self.snapshot();
        self.last = Some(state);
        (self.emit)(
            "applied",
            serde_json::to_value(Response {
                serial,
                state,
                error,
            })
            .expect("window response"),
        );
    }
    fn report(&self, error: String) {
        (self.emit)("failure", Value::String(error));
    }
}
