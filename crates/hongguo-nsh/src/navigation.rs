pub(crate) use crate::pages::DetailPrefill;
use nsh_core::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Route {
    Catalog,
    Detail(String),
    DetailWithPrefill(DetailPrefill),
    Creator(String),
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
    pub(crate) fn detail_id(&self) -> Option<&str> {
        match self {
            Self::Detail(id) => Some(id),
            Self::DetailWithPrefill(seed) => Some(&seed.series_id),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Navigation(Signal<Vec<Route>>, Signal<Vec<Route>>);

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
        Self(signal(vec![Route::Catalog]), signal(Vec::new()))
    }
    pub(crate) fn routes(self) -> Signal<Vec<Route>> {
        self.0
    }
    pub(crate) fn section(self) -> Route {
        self.0.with(|routes| {
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
            Some(Route::Player(_, _)) => *section == Route::Catalog,
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
        self.0.update(|routes| {
            if routes.len() > 1
                && let Some(route) = routes.pop()
            {
                self.1.update(|future| future.push(route));
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

#[cfg(test)]
mod tests {
    use super::*;
    #[component]
    fn History() -> impl IntoElement {
        let nav = Navigation::new();
        view()
            .child(text(move || format!("{:?}", nav.routes().get())))
            .child(
                crate::ui::button("History", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || nav.push(Route::History)),
            )
            .child(
                crate::ui::button("Settings", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || nav.push(Route::Settings)),
            )
            .child(
                crate::ui::button("Back", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || nav.back()),
            )
            .child(
                crate::ui::button("Forward", crate::ui::ButtonVariant::Quiet)
                    .disabled(move || !nav.can_forward())
                    .on_press(move || nav.forward()),
            )
    }
    #[test]
    fn forward_restores_the_back_stack_and_a_new_destination_discards_forward_history()
    -> Result<(), String> {
        let mut app =
            nsh_testing::Headless::start(nsh_engine::EngineConfig::new(nsh_engine::Root::of::<
                History,
            >()))
            .map_err(|error| error.to_string())?;
        app.wait_for("History")?;
        app.press("History")?;
        app.press("Settings")?;
        app.press("Back")?;
        app.wait_for("text:[Catalog, History]")?;
        app.press("Forward")?;
        app.wait_for("text:[Catalog, History, Settings]")?;
        app.press("Back")?;
        app.press("Back")?;
        app.press("Settings")?;
        app.wait_for("text:[Catalog, Settings]")?;
        assert!(!app.screen().enabled("Forward")?);
        Ok(())
    }

    #[component]
    fn SeededHistory() -> impl IntoElement {
        let nav = Navigation::new();
        view()
            .child(
                crate::ui::button("Open original detail", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || nav.push(Route::Detail("same-id".into()))),
            )
            .child(
                crate::ui::button("Open its creator", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || nav.push(Route::Creator("author".into()))),
            )
            .child(
                crate::ui::button("Open updated source card", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || {
                        nav.detail(DetailPrefill {
                            series_id: "same-id".into(),
                            title: "Updated source title".into(),
                            ..Default::default()
                        })
                    }),
            )
            .child(
                crate::ui::button("Back", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || nav.back()),
            )
            .child(text(move || {
                nav.routes().with(|routes| {
                    format!(
                        "depth:{} title:{} selected:{}",
                        routes.len(),
                        match routes.last() {
                            Some(Route::DetailWithPrefill(seed)) => seed.title.as_str(),
                            _ => "none",
                        },
                        nav.is_selected(&Route::Catalog)
                    )
                })
            }))
    }

    #[test]
    fn a_source_snapshot_updates_the_same_detail_entry_and_back_returns_to_the_source_section()
    -> Result<(), String> {
        let mut app =
            nsh_testing::Headless::start(nsh_engine::EngineConfig::new(nsh_engine::Root::of::<
                SeededHistory,
            >()))
            .map_err(|error| error.to_string())?;
        app.wait_for("Open original detail")?;
        app.press("Open original detail")?;
        app.press("Open its creator")?;
        app.press("Open updated source card")?;
        app.wait_for("text:depth:2 title:Updated source title selected:false")?;
        app.press("Back")?;
        app.wait_for("text:depth:1 title:none selected:true")?;
        Ok(())
    }
}
