pub(crate) use crate::features::DetailPrefill;
use nsh_core::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AlbumEntry {
    pub(crate) series_id: String,
    pub(crate) vid: String,
    pub(crate) episode: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Route {
    Catalog,
    Detail(String),
    DetailWithPrefill(DetailPrefill),
    Creator(String),
    AlbumPlayer(String, AlbumEntry),
    Player(String, u32),
    History,
    Settings,
    Account,
    Tasks,
    Merge,
    Storage,
    Follows,
    Collections,
    Liked,
    Reservations,
    Rank,
    New,
    Browse,
}

impl Route {
    pub(crate) fn section_root(&self) -> Self {
        match self {
            Self::History
            | Self::Collections
            | Self::Liked
            | Self::Follows
            | Self::Reservations => Self::History,
            Self::Browse | Self::Rank | Self::New => Self::Browse,
            Self::Tasks | Self::Merge | Self::Storage => Self::Tasks,
            other => other.clone(),
        }
    }
    pub(crate) fn detail_id(&self) -> Option<&str> {
        match self {
            Self::Detail(id) => Some(id),
            Self::DetailWithPrefill(seed) => Some(&seed.series_id),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Navigation(
    Signal<Vec<Route>>,
    Signal<Vec<Route>>,
    Signal<Option<BackAction>>,
);

#[derive(Clone, Copy)]
struct BackAction {
    enabled: Memo<bool>,
    action: Stored<std::rc::Rc<dyn Fn()>>,
}

impl PartialEq for BackAction {
    fn eq(&self, other: &Self) -> bool {
        self.enabled == other.enabled
    }
}

#[derive(Clone, Copy)]
pub(crate) struct AccountPrompt(Signal<bool>);

impl AccountPrompt {
    pub(crate) fn provide() -> Self {
        if let Some(prompt) = use_context::<Self>() {
            return prompt;
        }
        let prompt = Self(signal(false));
        provide_context(prompt);
        prompt
    }
    pub(crate) fn open(self) -> bool {
        self.0.get()
    }
    pub(crate) fn set_open(self, open: bool) {
        self.0.set(open);
    }
    pub(crate) fn request(self) {
        self.set_open(true);
    }
}

impl Navigation {
    pub(crate) fn new() -> Self {
        Self(
            signal(vec![Route::Catalog]),
            signal(Vec::new()),
            signal(None),
        )
    }
    pub(crate) fn routes(self) -> Signal<Vec<Route>> {
        self.0
    }
    pub(crate) fn section(self) -> Route {
        self.0.with(|routes| {
            if matches!(
                routes.last(),
                Some(Route::Player(_, _) | Route::AlbumPlayer(_, _))
            ) {
                return Route::Catalog;
            }
            routes
                .iter()
                .rev()
                .find(|route| route.detail_id().is_none() && !matches!(route, Route::Player(_, _)))
                .cloned()
                .unwrap_or(Route::Catalog)
        })
    }
    pub(crate) fn is_selected(self, section: &Route) -> bool {
        self.0.with(|routes| match routes.last() {
            Some(Route::Detail(_) | Route::DetailWithPrefill(_) | Route::Creator(_)) => false,
            Some(Route::Player(_, _) | Route::AlbumPlayer(_, _)) => *section == Route::Catalog,
            _ => self.section() == *section,
        })
    }
    pub(crate) fn push(self, route: Route) {
        if route == Route::Account
            && let Some(prompt) = use_context::<AccountPrompt>()
        {
            prompt.request();
            return;
        }
        self.1.set(Vec::new());
        self.0.update(|routes| {
            if let Some(current) = routes.last_mut()
                && current.section_root() == route.section_root()
                && *current != route
            {
                *current = route;
                return;
            }
            if let Some(index) = routes.iter().position(|existing| {
                existing == &route
                    || route
                        .detail_id()
                        .is_some_and(|id| existing.detail_id() == Some(id))
            }) {
                routes[index] = route;
                routes.truncate(index + 1);
            } else {
                routes.push(route);
            }
        });
    }
    pub(crate) fn detail(self, prefill: DetailPrefill) {
        if !prefill.series_id.is_empty() {
            self.push(Route::DetailWithPrefill(prefill));
        }
    }
    pub(crate) fn back(self) {
        if let Some(back) = self.2.get_untracked()
            && back.enabled.get_untracked()
        {
            (back.action.get())();
            return;
        }
        self.0.update(|routes| {
            if routes.len() > 1
                && let Some(route) = routes.pop()
            {
                self.1.update(|future| future.push(route));
            }
        });
    }
    pub(crate) fn can_back(self) -> bool {
        self.0.with(Vec::len) > 1 || self.2.get().is_some_and(|back| back.enabled.get())
    }
    pub(crate) fn on_back(self, enabled: Memo<bool>, action: impl Fn() + 'static) {
        let back = BackAction {
            enabled,
            action: stored(std::rc::Rc::new(action) as std::rc::Rc<dyn Fn()>),
        };
        effect(move || {
            if enabled.get() {
                self.2.set(Some(back));
            } else if self
                .2
                .get_untracked()
                .is_some_and(|back| back.enabled == enabled)
            {
                self.2.set(None);
            }
        });
        cleanup(move || {
            if self
                .2
                .try_get()
                .flatten()
                .is_some_and(|back| back.enabled == enabled)
            {
                self.2.try_set(None);
            }
        });
    }
    pub(crate) fn can_forward(self) -> bool {
        self.1.with(|future| !future.is_empty())
    }
    pub(crate) fn forward(self) {
        self.1.update(|future| {
            if let Some(route) = future.pop() {
                self.0.update(|routes| routes.push(route));
            }
        });
    }
    pub(crate) fn dismiss(self, index: usize) {
        self.0.update(|routes| {
            let removed = routes.split_off(index.max(1).min(routes.len()));
            self.1
                .update(|future| future.extend(removed.into_iter().rev()));
        });
    }
}
