//! NSH 0.4 Menu supplies popup/selection semantics; this adds scoped keyboard focus.
use nsh_core::prelude::*;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone)]
struct Entry {
    node: ElementRef,
    checked: Memo<bool>,
}

#[derive(Clone)]
pub(super) struct Navigation {
    entries: Rc<RefCell<Vec<Option<Entry>>>>,
    cursor: Stored<usize>,
}

impl Navigation {
    pub(super) fn register(&self, node: ElementRef, checked: Memo<bool>) {
        let index = self.entries.borrow().len();
        self.entries
            .borrow_mut()
            .push(Some(Entry { node, checked }));
        let entries = self.entries.clone();
        cleanup(move || {
            if let Some(entry) = entries.borrow_mut().get_mut(index) {
                *entry = None;
            }
        });
    }

    fn focus(&self, at: usize) {
        let node = self
            .entries
            .borrow()
            .get(at)
            .and_then(|entry| entry.as_ref())
            .map(|entry| entry.node);
        if let Some(node) = node {
            self.cursor.replace(at);
            node.focus();
        }
    }

    fn step(&self, delta: isize) {
        let indices = self
            .entries
            .borrow()
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| entry.as_ref().map(|_| index))
            .collect::<Vec<_>>();
        if indices.is_empty() {
            return;
        }
        let at = indices
            .iter()
            .position(|index| *index == self.cursor.get())
            .unwrap_or_default();
        let next = (at as isize + delta).rem_euclid(indices.len() as isize) as usize;
        self.focus(indices[next]);
    }

    fn initial(&self) {
        let at = self
            .entries
            .borrow()
            .iter()
            .position(|entry| {
                entry
                    .as_ref()
                    .is_some_and(|entry| entry.checked.get_untracked())
            })
            .unwrap_or_default();
        self.focus(at);
    }
}

pub(super) fn body(
    open: Memo<bool>,
    close: impl Fn() + Copy + 'static,
    content: impl Fn() -> View,
) -> View {
    let navigation = Navigation {
        entries: Rc::new(RefCell::new(Vec::new())),
        cursor: stored(0),
    };
    provide_context(navigation.clone());
    let keys = navigation.clone();
    effect(move || {
        if !open.get() {
            return;
        }
        for (key, delta) in [("up", -1_isize), ("down", 1)] {
            let keys = keys.clone();
            on_key_with(
                key,
                KeyOptions {
                    when: nsh_core::keymap::When::NotEditing,
                    repeat: true,
                    ..Default::default()
                },
                move |_| keys.step(delta),
            );
        }
        for key in ["left", "right"] {
            on_key(key, |_| {});
        }
        on_key("tab", move |_| close());
        on_key("shift+tab", move |_| close());
    });
    let initialized = stored(false);
    view()
        .on_layout(move |_: &Bounds<Points>| {
            if !initialized.replace(true) {
                navigation.initial();
            }
        })
        .child(content())
}
