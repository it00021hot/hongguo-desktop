use super::{Kind, feed, fetch, model::Query};
use crate::{bridge, events::SessionHandle, navigation::Navigation};
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(super) struct Model {
    pub online: Signal<bool>,
    pub query: Signal<Query>,
    pub search: Signal<String>,
    pub context: feed::Context,
    pub result: feed::Feed,
}

pub(super) fn load(session: SessionHandle, nav: Navigation, kind: Kind) -> Model {
    crate::view_memory::cached(kind.memory_name(), move || {
        let memory = crate::view_memory::scope(kind.memory_name());
        let online = memory.signal("online", true);
        let search = memory.signal("search", String::new());
        let query = signal(Query {
            online: online.get_untracked(),
            ..Default::default()
        });
        let context = feed::Context {
            logged_in: computed(move || {
                session.identity();
                session.with(|session| session.state.settings().account.is_some())
            }),
            active: computed(move || {
                nav.routes()
                    .with(|routes| routes.last() == Some(&kind.route()))
            }),
            identity: computed(move || session.identity()),
            revision: computed(move || session.library()),
            search: (kind == Kind::Reservations).then_some(search),
        };
        let result = feed::load(query, context, move |query| {
            let state = session.with(|session| session.state.clone());
            bridge::request(fetch::load(state, kind, query))
        });
        Model {
            online,
            query,
            search,
            context,
            result,
        }
    })
}
