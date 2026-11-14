use super::*;
use crate::window_modes::geometry::Rect;
use std::{cell::RefCell, rc::Rc};

struct Fake {
    attached: bool,
    geometry: Geometry,
    fullscreen: bool,
    controls_hidden: bool,
    visible: bool,
    capture_available: bool,
    pinned: bool,
}
impl Window for Rc<RefCell<Fake>> {
    fn exposed(&self) -> Result<bool, String> {
        Ok(self.borrow().visible)
    }
    fn attached(&self) -> bool {
        self.borrow().attached
    }
    fn capture(&self) -> Result<Capture, String> {
        let state = self.borrow();
        if !state.capture_available {
            return Err("window is not on a display".into());
        }
        Ok(Capture {
            geometry: state.geometry,
            fullscreen: state.fullscreen,
            minimized: false,
            pointer_inside: Some(true),
            pinned: state.pinned,
        })
    }
    fn apply(&self, plan: Plan) -> Result<(), String> {
        self.borrow_mut().geometry.frame = plan.frame;
        Ok(())
    }
    fn controls_hidden(&self, hidden: bool) -> Result<(), String> {
        self.borrow_mut().controls_hidden = hidden;
        Ok(())
    }
    fn visible(&self, visible: bool, _: bool) -> Result<(), String> {
        self.borrow_mut().visible = visible;
        Ok(())
    }
    fn pinned(&self, pinned: bool) -> Result<(), String> {
        self.borrow_mut().pinned = pinned;
        Ok(())
    }
}

type Events = Rc<RefCell<Vec<(String, Value)>>>;
fn fixture() -> (Desktop, Rc<RefCell<Fake>>, Events) {
    let state = Rc::new(RefCell::new(Fake {
        attached: true,
        geometry: Geometry {
            frame: Rect {
                x: 40.0,
                y: 50.0,
                width: 1000.0,
                height: 700.0,
            },
            work: Rect {
                x: 0.0,
                y: 24.0,
                width: 1920.0,
                height: 1056.0,
            },
            maximized: false,
            scale: 1.0,
        },
        fullscreen: false,
        controls_hidden: false,
        visible: true,
        capture_available: true,
        pinned: false,
    }));
    let events = Events::default();
    let sink = events.clone();
    (
        Desktop::new(Box::new(state.clone()), move |name, value| {
            sink.borrow_mut().push((name.into(), value));
        }),
        state,
        events,
    )
}

#[test]
fn visibility_observation_emits_changes_only_while_enabled() {
    let (mut desktop, window, events) = fixture();
    desktop.observe_visibility(true);
    desktop.tick();
    assert!(
        events
            .borrow()
            .iter()
            .any(|(name, value)| name == "visibility" && value == true)
    );
    window.borrow_mut().visible = false;
    desktop.observe_visibility(false);
    desktop.observe_visibility(true);
    desktop.tick();
    assert!(
        events
            .borrow()
            .iter()
            .any(|(name, value)| name == "visibility" && value == false)
    );
    events.borrow_mut().clear();
    desktop.observe_visibility(false);
    events.borrow_mut().clear();
    desktop.tick();
    assert!(!events.borrow().iter().any(|(name, _)| name == "visibility"));
}

#[test]
fn visibility_is_reported_even_when_window_geometry_is_unavailable() {
    let (mut desktop, window, events) = fixture();
    window.borrow_mut().capture_available = false;
    window.borrow_mut().visible = false;
    desktop.observe_visibility(true);
    desktop.tick();
    assert!(
        events
            .borrow()
            .iter()
            .any(|(name, value)| name == "visibility" && value == false)
    );
}
fn mini() -> Request {
    Request {
        serial: 9,
        action: Action::SetMode {
            mode: Mode::Mini,
            aspect: super::super::PHONE_ASPECT,
        },
    }
}

#[test]
fn fullscreen_is_acknowledged_only_after_actual_confirmation_and_has_a_deadline() {
    let (mut desktop, _, events) = fixture();
    desktop.request(Request {
        serial: 7,
        action: Action::ToggleFullscreen,
    });
    assert!(
        events
            .borrow()
            .iter()
            .any(|(name, _)| name == "toggleFullscreen")
    );
    assert!(!events.borrow().iter().any(|(name, _)| name == "applied"));
    desktop.pending.as_mut().unwrap().deadline = Instant::now() - Duration::from_millis(1);
    desktop.tick();
    let events = events.borrow();
    let response = events.iter().find(|(name, _)| name == "applied").unwrap();
    assert_eq!(response.1["serial"], 7);
    assert_eq!(response.1["state"]["fullscreen"], false);
    assert!(
        response.1["error"]
            .as_str()
            .unwrap()
            .contains("did not confirm")
    );
    assert!(desktop.pending.is_none());
}

#[test]
fn mini_waits_for_system_fullscreen_exit_before_applying_geometry() {
    let (mut desktop, window, events) = fixture();
    window.borrow_mut().fullscreen = true;
    desktop.request(mini());
    assert_eq!(window.borrow().geometry.frame.width, 1000.0);
    assert!(
        events
            .borrow()
            .iter()
            .any(|(name, _)| name == "exitFullscreen")
    );
    window.borrow_mut().fullscreen = false;
    desktop.tick();
    assert_eq!(window.borrow().geometry.frame.width, 480.0);
    assert!(window.borrow().controls_hidden);
    let events = events.borrow();
    let response = events.iter().find(|(name, _)| name == "applied").unwrap();
    assert_eq!(response.1["serial"], 9);
    assert_eq!(response.1["state"]["mode"], "mini");
    assert!(response.1["error"].is_null());
}

#[test]
fn cleanup_restores_original_geometry_and_visibility_without_emitting_disposed_ui_events() {
    let (mut desktop, window, events) = fixture();
    let original = window.borrow().geometry.frame;
    desktop.request(mini());
    desktop.boss(true);
    assert!(!window.borrow().visible);
    events.borrow_mut().clear();
    desktop.restore();
    assert_eq!(window.borrow().geometry.frame, original);
    assert!(window.borrow().visible);
    assert!(!window.borrow().controls_hidden);
    assert!(events.borrow().is_empty());
}

#[test]
fn cleanup_after_host_detach_does_not_reopen_or_resize_the_closed_window() {
    let (mut desktop, window, events) = fixture();
    desktop.request(mini());
    desktop.boss(true);
    window.borrow_mut().attached = false;
    let closed_frame = window.borrow().geometry.frame;
    events.borrow_mut().clear();
    desktop.restore();
    assert_eq!(window.borrow().geometry.frame, closed_frame);
    assert!(!window.borrow().visible);
    assert!(events.borrow().is_empty());
}
