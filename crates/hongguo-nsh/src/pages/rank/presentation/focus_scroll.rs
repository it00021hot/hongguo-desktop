use hongguo_api::rank::RankSubList;
use nsh_core::{element::ScrollEvent, prelude::*};
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub(super) struct FocusScroll {
    pub(super) node: ElementRef,
    horizontal: bool,
    viewport: Signal<f32>,
    offset: Stored<f32>,
    focused: Signal<Option<String>>,
    rows: Stored<HashMap<String, Bounds<Points>>>,
}

impl FocusScroll {
    pub(super) fn new(horizontal: bool) -> Self {
        let state = Self {
            node: element_ref(),
            horizontal,
            viewport: signal(0.0),
            offset: stored(0.0),
            focused: signal(None),
            rows: stored(HashMap::new()),
        };
        state.node.on_focus_out(move || state.focused.set(None));
        effect(move || {
            state.focused.get();
            state.viewport.get();
            state.reveal();
        });
        state
    }

    pub(super) fn focus(self, key: String) {
        self.focused.set(Some(key));
    }

    pub(super) fn retain(self, rows: &[RankSubList]) {
        self.rows
            .with(|bounds| bounds.retain(|id, _| rows.iter().any(|row| row.id == *id)));
        if self.focused.with_untracked(|key| {
            key.as_ref()
                .is_some_and(|key| !rows.iter().any(|row| row.id == *key))
        }) {
            self.focused.set(None);
        }
    }

    pub(super) fn layout(self, bounds: &Bounds<Points>) {
        self.viewport.set(if self.horizontal {
            bounds.size.width.0
        } else {
            bounds.size.height.0
        });
    }

    pub(super) fn moved(self, event: &ScrollEvent) {
        self.offset.replace(if self.horizontal {
            event.offset.x.0
        } else {
            event.offset.y.0
        });
    }

    pub(super) fn row(self, key: &str, bounds: &Bounds<Points>) {
        self.rows.with(|rows| {
            rows.insert(key.into(), *bounds);
        });
        if self
            .focused
            .with_untracked(|focused| focused.as_deref() == Some(key))
        {
            self.reveal();
        }
    }

    fn reveal(self) {
        let Some(key) = self.focused.get_untracked() else {
            return;
        };
        let Some(row) = self.rows.with(|rows| rows.get(&key).copied()) else {
            return;
        };
        let viewport = self.viewport.get_untracked();
        if viewport <= 0.0 {
            return;
        }
        let offset = self.offset.get();
        let (start, length) = if self.horizontal {
            (row.origin.x.0, row.size.width.0)
        } else {
            (row.origin.y.0, row.size.height.0)
        };
        let next = if start < offset {
            start
        } else if start + length > offset + viewport {
            start + length - viewport
        } else {
            return;
        };
        let next = next.max(0.0);
        self.offset.replace(next);
        self.node.scroll_to(
            if self.horizontal {
                Point::new(px(next), px(0.0))
            } else {
                Point::new(px(0.0), px(next))
            },
            false,
        );
    }
}
