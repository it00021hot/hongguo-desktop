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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{ButtonVariant, button};
    use nsh_engine::{EngineConfig, Root};
    use nsh_testing::Headless;
    use std::time::Duration;

    #[derive(Clone, Copy)]
    struct Builds(Signal<u32>);

    #[component]
    fn Form() -> impl IntoElement {
        let builds = use_context::<Builds>().unwrap();
        let (count, value) = cached("fixture", || {
            builds.0.update(|count| *count += 1);
            let count = signal(0_u32);
            let value: Resource<u32, String> = resource(
                move || Some(count.get()),
                |count, _| std::future::ready(Ok(count)),
            );
            (count, value)
        });
        view()
            .child(text(move || format!("value:{:?}", value.get())))
            .child(
                button("Increment model", ButtonVariant::Quiet)
                    .on_press(move || count.update(|count| *count += 1)),
            )
    }

    #[component]
    fn Window() -> impl IntoElement {
        provide_models();
        let builds = signal(0);
        provide_context(Builds(builds));
        let shown = signal(true);
        view()
            .child(text(move || format!("builds:{}", builds.get())))
            .child(button("Hide model", ButtonVariant::Quiet).on_press(move || shown.set(false)))
            .child(button("Show model", ButtonVariant::Quiet).on_press(move || shown.set(true)))
            .child(when(shown, || Form::element(())))
    }

    fn start() -> Result<Headless, String> {
        let mut app = Headless::start(EngineConfig::new(Root::of::<Window>()))
            .map_err(|error| error.to_string())?;
        app.wait_for("text:value:Some(0)")?;
        Ok(app)
    }

    #[test]
    fn resources_and_their_inputs_survive_page_disposal_under_one_session_owner()
    -> Result<(), String> {
        let mut app = start()?;
        app.press("Increment model")?;
        app.wait_for("text:value:Some(1)")?;
        app.press("Hide model")?;
        app.wait_for_gone("Increment model", Duration::from_secs(1))?;
        app.press("Show model")?;
        app.wait_for("text:value:Some(1)")?;
        app.wait_for("text:builds:1")?;
        app.press("Increment model")?;
        app.wait_for("text:value:Some(2)")?;
        let mut another = start()?;
        another.wait_for("text:builds:1")?;
        another.wait_for("text:value:Some(0)")?;
        Ok(())
    }
}
