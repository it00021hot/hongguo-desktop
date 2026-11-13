use nsh_core::prelude::*;
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub(super) struct PopupScroll {
    pub node: ElementRef,
    viewport: Signal<f32>,
    offset: Signal<f32>,
    selected: Signal<usize>,
    rows: Stored<HashMap<usize, Bounds<Points>>>,
}

impl PopupScroll {
    pub fn new(selected: Signal<usize>) -> Self {
        let state = Self {
            node: element_ref(),
            viewport: signal(0.0),
            offset: signal(0.0),
            selected,
            rows: stored(HashMap::new()),
        };
        effect(move || {
            let index = selected.get();
            let _height = state.viewport.get();
            state.reveal(index);
        });
        state
    }

    pub fn layout(self, bounds: &Bounds<Points>) {
        self.viewport.set(bounds.size.height.0);
    }

    pub fn moved(self, event: &nsh_core::element::ScrollEvent) {
        self.offset.set(event.offset.y.0);
    }

    pub fn row(self, index: usize, bounds: &Bounds<Points>) {
        self.rows.with(|rows| {
            rows.insert(index, *bounds);
        });
        if self.selected.get_untracked() == index {
            self.reveal(index);
        }
    }

    fn reveal(self, index: usize) {
        let height = self.viewport.get_untracked();
        let Some(row) = self.rows.with(|rows| rows.get(&index).copied()) else {
            return;
        };
        if height <= 0.0 {
            return;
        }
        let offset = self.offset.get_untracked();
        let top = row.origin.y.0;
        let bottom = top + row.size.height.0;
        let next = if top < offset {
            top
        } else if bottom > offset + height {
            bottom - height
        } else {
            return;
        };
        let next = next.max(0.0);
        self.offset.set(next);
        self.node.scroll_to(Point::new(px(0.0), px(next)), false);
    }
}
