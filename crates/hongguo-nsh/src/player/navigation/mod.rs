mod model;
mod platform;
mod preview;
mod protocol;
pub(crate) use preview::build as preview;
#[cfg(test)]
mod tests;

use model::{Direction, Gate};
use nsh_core::prelude::*;

type StepAction = Stored<Box<dyn Fn(i32)>>;
type WakeAction = Stored<Option<Box<dyn Fn()>>>;
type FeedStep = Stored<Box<dyn Fn(i32) -> bool>>;
type FeedGoto = Stored<Option<Box<dyn Fn(hongguo_common::model::Series, u32)>>>;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) use platform::register;

#[derive(Clone, Copy)]
pub(crate) struct FeedNavigation {
    locked: Signal<Option<String>>,
    gate: Stored<Gate>,
    origin: nsh_core::time::Instant,
    step: FeedStep,
    goto: FeedGoto,
    revision: Signal<u64>,
    quiet_handoff: super::chrome::QuietHandoff,
    automatic_update: Stored<bool>,
}

impl FeedNavigation {
    pub fn new(step: impl Fn(i32) -> bool + 'static) -> Self {
        Self {
            locked: signal(None),
            gate: stored(Gate::default()),
            origin: nsh_core::time::now(),
            step: stored(Box::new(step)),
            goto: stored(None),
            revision: signal(0),
            quiet_handoff: super::chrome::QuietHandoff::new(),
            automatic_update: stored(false),
        }
    }

    pub fn lock_series(self, id: String) {
        self.locked.set(Some(id));
    }

    pub fn clear_lock(self) {
        self.locked.set(None);
    }

    pub fn on_goto(self, goto: impl Fn(hongguo_common::model::Series, u32) + 'static) -> Self {
        self.goto.replace(Some(Box::new(goto)));
        self
    }

    pub fn invalidate(self) {
        if !self.automatic_update.replace(false) {
            self.quiet_handoff.clear();
        }
        self.revision.update(|value| *value = value.wrapping_add(1));
    }
    pub fn revision(self) -> u64 {
        self.revision.get()
    }

    pub fn quiet_handoff(self) -> super::chrome::QuietHandoff {
        self.quiet_handoff
    }

    pub fn request_goto(self, series: hongguo_common::model::Series, episode: u32) -> bool {
        self.goto.with(|goto| {
            if let Some(goto) = goto {
                self.automatic_update.replace(true);
                goto(series, episode);
                true
            } else {
                false
            }
        })
    }

    pub fn advance_feed(self) -> bool {
        self.automatic_update.replace(true);
        let advanced = self.step.with(|step| step(1));
        if advanced {
            self.clear_lock();
        } else {
            self.automatic_update.replace(false);
            self.quiet_handoff.clear();
        }
        advanced
    }
}

#[derive(Clone, Copy)]
pub(crate) struct State {
    pub active: Memo<bool>,
    pub blocked: Memo<bool>,
    pub series: Memo<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct Controller {
    state: State,
    feed: Option<FeedNavigation>,
    gate: Stored<Gate>,
    origin: nsh_core::time::Instant,
    episode: StepAction,
    activity: WakeAction,
}

impl Controller {
    pub fn new(
        state: State,
        feed: Option<FeedNavigation>,
        episode: impl Fn(i32) + 'static,
    ) -> Self {
        Self {
            state,
            feed,
            gate: feed.map_or_else(|| stored(Gate::default()), |feed| feed.gate),
            origin: feed.map_or_else(nsh_core::time::now, |feed| feed.origin),
            episode: stored(Box::new(episode)),
            activity: stored(None),
        }
    }

    pub fn on_activity(self, wake: impl Fn() + 'static) -> Self {
        self.activity.replace(Some(Box::new(wake)));
        self
    }

    fn wake(self) {
        self.activity.with(|activity| {
            if let Some(wake) = activity {
                wake();
            }
        });
    }

    fn wheel(self, delta: f64) {
        if self.allowed() {
            self.wake();
            self.gesture(model::wheel(delta));
        }
    }

    pub fn in_binge(self) -> Memo<bool> {
        computed(move || {
            self.feed
                .is_some_and(|feed| feed.locked.get().as_ref() == Some(&self.state.series.get()))
        })
    }

    pub fn can_binge(self) -> bool {
        self.feed.is_some()
    }

    pub fn enter_binge(self) {
        if self.allowed()
            && let Some(feed) = self.feed
        {
            feed.lock_series(self.state.series.get_untracked());
        }
    }

    pub fn exit_binge(self) {
        if self.allowed()
            && let Some(feed) = self.feed
        {
            feed.clear_lock();
        }
    }

    fn allowed(self) -> bool {
        self.state.active.get_untracked() && !self.state.blocked.get_untracked()
    }

    fn step(self, direction: Direction) {
        if !self.allowed() {
            return;
        }
        if let Some(feed) = self.feed
            && feed.locked.get_untracked().as_ref() != Some(&self.state.series.get_untracked())
        {
            feed.step.with(|step| step(direction.step()));
        } else {
            self.episode.with(|step| step(direction.step()));
        }
    }

    pub fn step_keyboard(self, direction: i32) {
        self.step(if direction < 0 {
            Direction::Previous
        } else {
            Direction::Next
        });
    }

    fn gesture(self, direction: Option<Direction>) {
        if !self.allowed() {
            return;
        }
        if let Some(direction) = direction
            && self
                .gate
                .with(|gate| gate.admit(nsh_core::time::now().duration_since(self.origin)))
        {
            self.step(direction);
        }
    }

    pub fn bind(self, surface: View) -> View {
        let origin = stored(None::<nsh_core::Point<nsh_core::Points>>);
        let hovered = signal(false);
        surface
            .on_mouse_enter(move |_: &nsh_core::MouseEvent| hovered.set(true))
            .on_mouse_leave(move |_: &nsh_core::MouseEvent| hovered.set(false))
            .claim_touches(TouchClaim::on_move().ask(move |request: &ClaimRequest| {
                let gesture = request.event.gesture;
                if self.allowed()
                    && gesture.active_touches == 1
                    && model::swipe(gesture.delta.x.0, gesture.delta.y.0).is_some()
                {
                    origin.replace(Some(gesture.origin));
                    request.claim();
                }
            }))
            .keep_touches(true)
            .on_responder_release(move |event: &ResponderEvent| {
                if let Some(start) = origin.replace(None) {
                    let direction =
                        model::swipe(event.page.x.0 - start.x.0, event.page.y.0 - start.y.0);
                    if direction.is_some() && self.allowed() {
                        self.wake();
                        self.gesture(direction);
                    }
                }
            })
            .on_responder_move(move |event: &ResponderEvent| {
                if event.gesture.active_touches > 1 {
                    origin.replace(None);
                }
            })
            .on_responder_terminate(move |_: &ResponderEvent| {
                origin.replace(None);
            })
            .child(when(
                move || {
                    matches!(
                        os(),
                        nsh_core::platform::Os::MacOs | nsh_core::platform::Os::Windows
                    )
                },
                move || {
                    native(protocol::COMPONENT)
                        .absolute()
                        .inset_0()
                        .opacity(0.0)
                        .pointer_events(PointerEvents::None)
                        .prop(protocol::ENABLED, move || {
                            serde_json::Value::Bool(
                                hovered.get()
                                    && self.state.active.get()
                                    && !self.state.blocked.get(),
                            )
                        })
                        .on_native_event(move |event: &nsh_core::element::NativeEvent| {
                            if event.name.as_ref() == protocol::WHEEL {
                                match serde_json::from_value::<protocol::WheelEvent>(
                                    event.data.clone(),
                                ) {
                                    Ok(value) => self.wheel(value.delta_y),
                                    Err(error) => {
                                        log::error!("[NSH navigation] invalid wheel event: {error}")
                                    }
                                }
                            }
                        })
                },
            ))
    }
}
