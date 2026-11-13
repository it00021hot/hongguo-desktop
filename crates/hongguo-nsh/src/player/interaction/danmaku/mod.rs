mod flight_model;
mod model;
mod motion;
pub(in crate::player) use motion::Motion;
mod overlay;
mod preview;
pub(in crate::player) use preview::layer as preview_layer;
mod preferences;
pub(crate) use preferences::DanmakuPreferences;
#[cfg(test)]
mod overlay_tests;
#[cfg(test)]
mod settings_tests;
use super::Target;
use crate::{bridge, pages::SessionHandle, ui};
use hongguo_api::danmaku::{self, Danmaku};
use hongguo_common::{AppError, AppResult};
use model::{Request, Timeline};
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(in crate::player) struct Handle {
    enabled: Signal<bool>,
    timeline: Signal<Timeline>,
    position: Signal<(f64, f64)>,
    motion: Motion,
    preferences: DanmakuPreferences,
    error: Signal<SharedString>,
    retry: Signal<u64>,
}

pub(in crate::player) fn observe(
    session: SessionHandle,
    target: impl IntoReactive<Option<Target>>,
    position: Signal<(f64, f64)>,
    motion: Motion,
    visible: Memo<bool>,
) -> Handle {
    let target = target.into_reactive();
    let preferences = session.danmaku_preferences();
    preferences.display.open.set(false);
    let handle = Handle {
        enabled: preferences.enabled,
        timeline: signal(Timeline::default()),
        position,
        motion,
        preferences,
        error: signal(SharedString::default()),
        retry: signal(0),
    };
    let query = signal(None::<Request>);
    let loaded = resource(
        move || query.get().map(|query| (query, handle.retry.get())),
        move |(request, _), _| {
            let state = session.with(|session| session.state.clone());
            bridge::request(async move {
                let page = danmaku::fetch_danmaku_window_page(
                    &request.vid,
                    &request.series,
                    request.start,
                    request.duration,
                    &request.cursor,
                    &state.api_env(),
                )
                .await?;
                Ok((request, page))
            })
        },
    );
    effect(move || {
        let Some(target) = target.get() else {
            query.set(None);
            handle.timeline.set(Timeline::default());
            return;
        };
        if handle.timeline.get().vid != target.vid
            || handle.timeline.get().series != target.series_id
        {
            handle
                .timeline
                .set(Timeline::new(target.series_id, target.vid));
            handle.error.set(SharedString::default());
            query.set(None);
            return;
        }
        if !handle.enabled.get() || !visible.get() {
            query.set(None);
            return;
        }
        let (at, duration) = position.get();
        if !loaded.loading()
            && let Some(next) = handle.timeline.get().request(millis(at), millis(duration))
            && query.get_untracked().as_ref() != Some(&next)
        {
            query.set(Some(next));
        }
    });
    effect(move || {
        if !loaded.loading()
            && let Some((request, page)) = loaded.get()
        {
            handle
                .timeline
                .update(|timeline| timeline.accept(request, page));
        }
        if let Some(error) = loaded.error() {
            handle.error.set(error.text());
        }
    });
    handle
}

impl Handle {
    pub(in crate::player) fn overlay(self) -> impl IntoElement {
        overlay::build(self)
    }

    pub(in crate::player) fn settings(self) -> ui::DanmakuSettingsState {
        self.preferences.display
    }
    pub(in crate::player) fn enabled(self) -> Signal<bool> {
        self.enabled
    }
    pub(in crate::player) fn toggle_settings(self) {
        self.preferences.display.open.update(|open| *open = !*open);
    }

    pub(in crate::player) fn controls(
        self,
        session: SessionHandle,
        previous: Memo<ElementRef>,
    ) -> impl IntoElement {
        let logged_in = computed(move || {
            session.identity();
            session.with(|session| session.state.settings().account.is_some())
        });
        let draft = signal(String::new());
        let editor = crate::emoji::Editor::new(draft, 100);
        let next = element_ref();
        let pending = signal(false);
        crate::player::stage::composer(
            editor,
            move || !logged_in.get() || pending.get(),
            crate::player::stage::ComposerEvents {
                emoji: Some(crate::emoji::system_picker(editor, next).into_element()),
                previous,
                next,
                send: Box::new(move || {
                    if pending.get_untracked() || !logged_in.get_untracked() {
                        return;
                    }
                    let text = draft.get_untracked();
                    if text.trim().is_empty() {
                        return;
                    }
                    let timeline = self.timeline.get_untracked();
                    let offset = millis(self.position.get_untracked().0);
                    let state = session.with(|session| session.state.clone());
                    pending.set(true);
                    self.error.set(SharedString::default());
                    spawn(async move {
                        let result = bridge::request(send(
                            state,
                            timeline.series.clone(),
                            timeline.vid.clone(),
                            text,
                            offset,
                        ))
                        .await;
                        pending.set(false);
                        match result {
                            Ok(item) => {
                                if self.timeline.get_untracked().vid == timeline.vid {
                                    self.timeline.update(|timeline| timeline.insert(item));
                                    draft.set(String::new());
                                }
                            }
                            Err(error) => self.error.set(error.text()),
                        }
                    });
                }),
            },
        )
    }
    pub(in crate::player) fn toggle(self) {
        self.enabled.update(|enabled| *enabled = !*enabled);
    }
    pub(in crate::player) fn notice(self) -> SharedString {
        let error = self.error.get();
        if error.is_empty() {
            self.preferences.error.get()
        } else {
            error
        }
    }
}

async fn send(
    state: hongguo_service::AppState,
    series: String,
    vid: String,
    text: String,
    offset: u64,
) -> AppResult<Danmaku> {
    if state.settings().account.is_none() {
        return Err(AppError::Auth("未登录".into()));
    }
    if vid.is_empty() {
        return Err(AppError::InvalidArgs("分集 ID 为空".into()));
    }
    let comment_id =
        hongguo_api::interact::send_danmaku(&vid, &series, &text, offset, &state.api_env()).await?;
    Ok(Danmaku {
        comment_id,
        text,
        offset_ms: offset,
        digg_count: 0,
    })
}

fn millis(seconds: f64) -> u64 {
    if seconds.is_finite() && seconds > 0.0 {
        (seconds * 1000.0) as u64
    } else {
        0
    }
}
