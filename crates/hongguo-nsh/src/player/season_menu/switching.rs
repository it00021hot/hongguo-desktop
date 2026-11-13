use crate::{bridge, events::SessionHandle, player::relay::SourceEpoch};
use hongguo_common::{AppError, model::Series};
use nsh_core::prelude::*;
use std::{convert::Infallible, future::Future, pin::Pin, rc::Rc};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Origin {
    pub identity: u64,
    pub selection: (String, u32),
    pub source: Option<SourceEpoch>,
}

type LoadFuture = Pin<Box<dyn Future<Output = Result<(Series, u32), bridge::Failure>>>>;
pub(super) type Loader = Rc<dyn Fn(String) -> LoadFuture>;
type Select = Box<dyn Fn(Series, u32)>;

#[derive(Clone, PartialEq, Eq)]
struct Request {
    serial: u64,
    origin: Origin,
    id: String,
}

#[derive(Clone)]
struct Outcome {
    request: Request,
    result: Result<(Series, u32), bridge::Failure>,
}

#[derive(Clone, Copy)]
pub(super) struct Controller {
    origin: Memo<Origin>,
    request: Signal<Option<Request>>,
    serial: Stored<u64>,
    selected: Stored<Option<Select>>,
    pending: Signal<bool>,
    message: Signal<SharedString>,
}

impl Controller {
    pub fn new(
        origin: Memo<Origin>,
        pending: Signal<bool>,
        message: Signal<SharedString>,
        load: Loader,
    ) -> Self {
        let controller = Self {
            origin,
            pending,
            message,
            request: signal(None),
            serial: stored(0),
            selected: stored(None),
        };
        effect(move || controller.reconcile());
        let work: Resource<Outcome, Infallible> = resource(
            move || {
                let origin = controller.origin.get();
                controller
                    .request
                    .get()
                    .filter(|request| request.origin == origin && origin.source.is_some())
            },
            move |request: Request, _| {
                let load = load.clone();
                async move {
                    let result = load(request.id.clone()).await;
                    Ok(Outcome { request, result })
                }
            },
        );
        effect(move || {
            if work.loading() {
                return;
            }
            let Some(outcome) = work.with(|outcome| outcome.cloned()) else {
                return;
            };
            if controller.origin.get() != outcome.request.origin
                || !controller.request.with(|request| {
                    request
                        .as_ref()
                        .is_some_and(|request| request.serial == outcome.request.serial)
                })
            {
                return;
            }
            controller.request.set(None);
            controller.pending.set(false);
            let selected = controller.selected.with(Option::take);
            match outcome.result {
                Ok((series, episode)) => {
                    if let Some(selected) = selected {
                        selected(series, episode);
                    }
                }
                Err(error) => controller.message.set(error.text()),
            }
        });
        controller
    }

    fn reconcile(self) {
        let current = self.origin.get();
        if self
            .request
            .get()
            .is_some_and(|request| request.origin != current)
        {
            self.request.set(None);
            self.selected.replace(None);
            // Only this switch owns pending here. A reservation with no switch
            // request retains its own busy flag until its response settles.
            self.pending.set(false);
        }
    }

    pub fn start(self, id: String, selected: impl Fn(Series, u32) + 'static) {
        self.reconcile();
        let origin = self.origin.get_untracked();
        if origin.source.is_none() || self.pending.get_untracked() {
            return;
        }
        let serial = self.serial.with(|serial| {
            *serial = serial.wrapping_add(1);
            *serial
        });
        batch(|| {
            self.pending.set(true);
            self.message.set(SharedString::default());
            self.selected.replace(Some(Box::new(selected)));
            self.request.set(Some(Request { serial, origin, id }));
        });
    }
}

pub(super) fn loader(session: SessionHandle) -> Loader {
    Rc::new(move |id| {
        let state = session.with(|session| session.clone());
        Box::pin(bridge::request(async move {
            let (id, episode) = super::super::resolve_start(state.clone(), id).await?;
            let store = state.state.store.clone();
            let key = id.clone();
            let series = hongguo_service::runtime::spawn_blocking(move || store.series_by_id(&key))
                .await
                .map_err(|error| AppError::Internal(error.to_string()))??
                .ok_or(AppError::NotFound(id))?;
            Ok((series, episode))
        }))
    })
}

#[cfg(test)]
mod tests;
