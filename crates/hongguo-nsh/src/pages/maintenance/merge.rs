use super::{
    backend, fixtures,
    model::{MergeDraft, MergeSnapshot, output_busy},
};
use crate::{
    bridge,
    events::SessionHandle,
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use hongguo_common::model::{MergeMode, MergePreflight};
use nsh_core::prelude::*;

mod actions;
mod tasks;
mod view;
use actions::dispatch;

#[derive(Clone)]
pub(super) enum Intent {
    Start,
    Remove(String),
    Reveal(String),
}

struct StartRequest {
    series: String,
    output: String,
    mode: MergeMode,
}

#[derive(Clone, Copy)]
pub(super) struct MergeView {
    pub files: Option<crate::file_access::FileAccess>,
    pub snapshot: Memo<MergeSnapshot>,
    pub preflight: Memo<Option<MergePreflight>>,
    pub loading: Memo<bool>,
    pub draft: MergeDraft,
    pub busy: Signal<bool>,
    pub confirming: Signal<bool>,
    pub message: Signal<SharedString>,
}

impl MergeView {
    fn new(
        snapshot: Memo<MergeSnapshot>,
        preflight: Memo<Option<MergePreflight>>,
        loading: Memo<bool>,
        draft: MergeDraft,
    ) -> Self {
        Self {
            files: crate::file_access::FileAccess::current(),
            snapshot,
            preflight,
            loading,
            draft,
            busy: signal(false),
            confirming: signal(false),
            message: signal(SharedString::default()),
        }
    }
    fn request(self) -> Option<StartRequest> {
        let series = self.draft.selected.get_untracked();
        let snapshot = self.snapshot.get_untracked();
        let output = self.draft.output(&snapshot.candidates);
        let preflight = self.preflight.get_untracked()?;
        if series.is_empty()
            || output.is_empty()
            || preflight.episode_count == 0
            || output_busy(&output, &snapshot.tasks)
        {
            return None;
        }
        Some(StartRequest {
            series,
            output,
            mode: self.draft.effective_mode(Some(&preflight)),
        })
    }
}

#[derive(Clone, Copy)]
enum Context {
    Production(Stored<SessionHandle>),
    Preview(Signal<MergeSnapshot>),
}

pub(in crate::pages) fn page(session: SessionHandle, nav: Navigation) -> Screen {
    let draft = MergeDraft::new();
    let completed = super::completed_inputs(session);
    let active = computed(move || {
        nav.routes()
            .with(|routes| routes.last() == Some(&Route::Merge))
    });
    let candidates = resource(
        move || active.get().then(|| (session.identity(), completed.get())),
        move |_, _| backend::candidates(session.with(|session| session.state.clone())),
    );
    let tasks = resource(
        move || active.get().then(|| (session.identity(), session.merges())),
        move |_, _| backend::tasks(session.with(|session| session.state.clone())),
    );
    let preflight = resource(
        move || {
            (active.get() && !draft.selected.get().is_empty())
                .then(|| (draft.selected.get(), session.identity(), completed.get()))
        },
        move |(id, _, _), _| {
            let state = session.with(|session| session.state.clone());
            backend::blocking(move || {
                hongguo_service::merge_service::prepare::preflight(&state, &id)
                    .map(|value| (id, value))
            })
        },
    );
    let snapshot = computed(move || MergeSnapshot {
        candidates: candidates.get().unwrap_or_default(),
        tasks: tasks.get().unwrap_or_default(),
    });
    let checked = computed(move || {
        preflight
            .get()
            .filter(|(id, _)| *id == draft.selected.get())
            .map(|(_, value)| value)
    });
    let model = MergeView::new(
        snapshot,
        checked,
        computed(move || candidates.loading()),
        draft,
    );
    let context = Context::Production(stored(session));
    ui::page(nav, move || tr("merge.title")).child(
        view::content(model, move |intent| dispatch(context, model, intent)).child(when(
            move || {
                candidates.error().is_some()
                    || tasks.error().is_some()
                    || preflight.error().is_some()
            },
            move || {
                crate::pages::error_state(
                    move || {
                        candidates
                            .error()
                            .or_else(|| tasks.error())
                            .or_else(|| preflight.error())
                            .map_or_else(SharedString::default, |error: bridge::Failure| {
                                error.text()
                            })
                    },
                    move || {
                        candidates.refetch();
                        tasks.refetch();
                        preflight.refetch();
                    },
                )
            },
        )),
    )
}

pub(in crate::pages) fn preview(nav: Navigation) -> Screen {
    let snapshot = signal(fixtures::merge());
    let draft = MergeDraft::new();
    let model = MergeView::new(
        computed(move || snapshot.get()),
        computed(move || fixtures::preflight(&draft.selected.get())),
        computed(|| false),
        draft,
    );
    ui::page(nav, move || tr("merge.title")).child(view::content(model, move |intent| {
        dispatch(Context::Preview(snapshot), model, intent)
    }))
}
