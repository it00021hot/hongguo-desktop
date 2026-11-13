use nsh_core::prelude::*;
mod models;
mod scroll;
pub(crate) use models::{cached, provide_models, track_identity};
pub(crate) use scroll::remember_scroll;
use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

type Key = (&'static str, &'static str, TypeId);
type Snapshots = HashMap<Key, Box<dyn Any>>;

#[derive(Clone, Copy)]
struct ViewMemory(Stored<Snapshots>);

#[derive(Clone, Copy)]
pub(crate) struct ViewScope {
    memory: Option<ViewMemory>,
    name: &'static str,
}

pub(crate) fn provide() {
    provide_context(ViewMemory(stored(HashMap::new())));
}

pub(crate) fn scope(name: &'static str) -> ViewScope {
    ViewScope {
        memory: use_context(),
        name,
    }
}

impl ViewScope {
    pub(crate) fn signal<T: Clone + PartialEq + 'static>(
        self,
        name: &'static str,
        initial: T,
    ) -> Signal<T> {
        self.projected(name, initial, Clone::clone)
    }

    /// Snapshots keep view values, never reactive handles or account results.
    pub(crate) fn projected<T: Clone + PartialEq + 'static>(
        self,
        name: &'static str,
        initial: T,
        snapshot: fn(&T) -> T,
    ) -> Signal<T> {
        let key = (self.name, name, TypeId::of::<T>());
        let saved = self.memory.and_then(|memory| {
            memory.0.with(|values| {
                values.get(&key).map(|value| {
                    value
                        .downcast_ref::<T>()
                        .expect("snapshot type matches its TypeId")
                        .clone()
                })
            })
        });
        let state = signal(saved.unwrap_or(initial));
        if let Some(memory) = self.memory {
            effect(move || {
                let value = state.with(snapshot);
                memory.0.with(|values| {
                    values.insert(key, Box::new(value));
                });
            });
        }
        state
    }
}

#[cfg(test)]
mod navigation_tests;
#[cfg(test)]
mod tests;
