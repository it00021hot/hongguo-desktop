use nsh_core::{prelude::*, time::Instant};
use std::time::Duration;

const HIDE_AFTER: Duration = Duration::from_secs(3);
const QUIET_WINDOW: Duration = Duration::from_secs(8);

/// A single auto-advance cue owned by the feed, which outlives its player row.
#[derive(Clone, Copy)]
pub(crate) struct QuietHandoff {
    until: Stored<Option<Instant>>,
}

impl QuietHandoff {
    pub(crate) fn new() -> Self {
        Self {
            until: stored(None),
        }
    }
    fn arm(self, until: Instant) {
        self.until.replace(Some(until));
    }
    pub(crate) fn clear(self) {
        self.until.replace(None);
    }
    fn take(self) -> Option<Instant> {
        self.until
            .replace(None)
            .filter(|until| *until > nsh_core::time::now())
    }
}

#[derive(Clone, Copy)]
pub(super) struct State {
    pub active: Memo<bool>,
    pub paused: Signal<bool>,
    pub panel_open: Memo<bool>,
}

#[derive(Clone, Copy)]
pub(crate) struct Chrome {
    shown: Memo<bool>,
    visible: Signal<bool>,
    hovered: Signal<bool>,
    focused: Signal<bool>,
    revision: Signal<u64>,
    quiet: Signal<bool>,
    quiet_until: Stored<Option<Instant>>,
    handoff: Stored<Option<QuietHandoff>>,
}

impl Chrome {
    pub fn current() -> Option<Self> {
        use_context()
    }

    pub(super) fn new(state: State) -> Self {
        let visible = signal(true);
        let hovered = signal(false);
        let focused = signal(false);
        let quiet = signal(false);
        let shown = computed(move || {
            state.active.get()
                && (state.panel_open.get()
                    || (!quiet.get()
                        && (state.paused.get() || visible.get() || hovered.get() || focused.get())))
        });
        let chrome = Self {
            shown,
            visible,
            hovered,
            focused,
            revision: signal(0),
            quiet,
            quiet_until: stored(None),
            handoff: stored(None),
        };
        effect(move || {
            chrome.revision.get();
            if !state.active.get() || state.paused.get() || hovered.get() || focused.get() {
                return;
            }
            let deadline = set_timeout(HIDE_AFTER, move || chrome.visible.set(false));
            cleanup(move || deadline.cancel());
        });
        chrome
    }

    pub fn shown(self) -> Memo<bool> {
        self.shown
    }

    pub fn with_handoff(self, handoff: Option<QuietHandoff>) -> Self {
        self.handoff.replace(handoff);
        if let Some(until) = handoff.and_then(QuietHandoff::take) {
            // Consume the original absolute deadline; do not arm a fresh cue.
            self.quiet_locally(until);
        }
        self
    }

    pub fn wake(self) {
        if let Some(handoff) = self.handoff.get() {
            handoff.clear();
        }
        self.quiet_until.replace(None);
        batch(|| {
            self.quiet.set(false);
            self.visible.set(true);
            self.revision.update(|revision| *revision += 1);
        });
    }

    pub fn moved(self) {
        if self.quiet.get_untracked()
            && self
                .quiet_until
                .with(|until| until.is_some_and(|until| nsh_core::time::now() < until))
        {
            return;
        }
        self.wake();
    }

    pub fn leave(self) {
        self.visible.set(false);
    }

    pub fn hovered(self, hovered: bool) {
        self.hovered.set(hovered);
        if hovered {
            self.wake();
        }
    }

    pub fn arm_quiet(self) {
        let until = nsh_core::time::now() + QUIET_WINDOW;
        if let Some(handoff) = self.handoff.get() {
            handoff.arm(until);
        }
        self.quiet_locally(until);
    }

    fn quiet_locally(self, until: Instant) {
        batch(|| {
            self.quiet.set(true);
            self.visible.set(false);
            self.hovered.set(false);
            self.quiet_until.replace(Some(until));
            self.revision.update(|revision| *revision += 1);
        });
    }

    pub fn mark_played(self) {
        if let Some(handoff) = self.handoff.get() {
            handoff.clear();
        }
        self.quiet_until.replace(None);
        if self.quiet.get_untracked() {
            self.quiet.set(false);
        }
    }

    pub fn bind_toolbar(self, view: View) -> View {
        view.on_mouse_enter(move |_: &nsh_core::MouseEvent| self.hovered(true))
            .on_mouse_leave(move |_: &nsh_core::MouseEvent| self.hovered(false))
    }

    pub fn bind_focus(self, view: View) -> View {
        let reference = element_ref();
        reference.on_focus_in(move || {
            self.focused.set(true);
            self.wake();
        });
        reference.on_focus_out(move || self.focused.set(false));
        view.element_ref(&reference)
    }
}

#[cfg(test)]
#[path = "chrome_tests.rs"]
mod tests;
