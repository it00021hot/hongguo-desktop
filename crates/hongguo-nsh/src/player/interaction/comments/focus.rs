use super::model::Feed;
use nsh_core::prelude::*;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

type MountedRows = Rc<RefCell<HashMap<String, Rc<Stops>>>>;

#[derive(Clone, Copy)]
pub(super) struct Focus {
    rows: Stored<MountedRows>,
    pub list: ElementRef,
    pub more: ElementRef,
}

struct Stops {
    reply: ElementRef,
    delete: ElementRef,
    mine: Memo<bool>,
    pending: Signal<bool>,
}

impl Focus {
    pub fn new() -> Self {
        Self {
            rows: stored(Rc::new(RefCell::new(HashMap::new()))),
            list: element_ref(),
            more: element_ref(),
        }
    }

    pub fn register(
        self,
        id: String,
        mine: Memo<bool>,
        pending: Signal<bool>,
    ) -> (ElementRef, ElementRef) {
        let stops = Rc::new(Stops {
            reply: element_ref(),
            delete: element_ref(),
            mine,
            pending,
        });
        let rows = self.rows.get();
        rows.borrow_mut().insert(id.clone(), stops.clone());
        let active = stops.clone();
        cleanup(move || {
            let mut rows = rows.borrow_mut();
            if rows
                .get(&id)
                .is_some_and(|current| Rc::ptr_eq(current, &active))
            {
                rows.remove(&id);
            }
        });
        (stops.reply, stops.delete)
    }

    pub fn previous(self, feed: Signal<Feed>) -> ElementRef {
        if self.more.node().is_some() {
            return self.more;
        }
        let rows = self.rows.get();
        let rows = rows.borrow();
        for item in feed.get_untracked().items.iter().rev() {
            if let Some(stops) = rows.get(item.id()) {
                if stops.mine.get_untracked()
                    && !stops.pending.get_untracked()
                    && stops.delete.node().is_some()
                {
                    return stops.delete;
                }
                if stops.reply.node().is_some() {
                    return stops.reply;
                }
            }
        }
        self.list
    }
}
