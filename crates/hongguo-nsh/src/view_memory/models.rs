use nsh_core::{element::Owner, prelude::*};
use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

type Models = HashMap<(&'static str, TypeId), Box<dyn Any>>;

#[derive(Clone, Copy)]
struct ModelMemory {
    owner: Owner,
    values: Stored<Models>,
}

pub(crate) fn provide_models() {
    provide_context(ModelMemory {
        owner: Owner::current(),
        values: stored(HashMap::new()),
    });
}

/// Builds data models under the session owner; mounted elements stay with their page.
pub(crate) fn cached<T: Clone + 'static>(name: &'static str, build: impl FnOnce() -> T) -> T {
    let Some(memory) = use_context::<ModelMemory>() else {
        return build();
    };
    let key = (name, TypeId::of::<T>());
    if let Some(value) = memory.values.with(|models| {
        models.get(&key).map(|model| {
            model
                .downcast_ref::<T>()
                .expect("model type matches TypeId")
                .clone()
        })
    }) {
        return value;
    }
    let value = memory.owner.run(build);
    memory.values.with(|models| {
        models.insert(key, Box::new(value.clone()));
    });
    value
}

pub(crate) fn track_identity<T: Clone + PartialEq + 'static>(
    identity: impl Fn() -> u64 + 'static,
    query: Signal<T>,
    reset: fn(&mut T, u64),
) {
    let seen = stored(None::<u64>);
    effect(move || {
        let identity = identity();
        if seen.replace(Some(identity)) != Some(identity) {
            query.update(|query| reset(query, identity));
        }
    });
}
