mod flight_model;
mod model;
mod motion;
pub(in crate::features::player) use motion::Motion;
mod overlay;

mod preferences;
use super::Target;
use crate::{bridge, features::SessionHandle};
use hongguo_common::{AppError, AppResult};
use hongguo_service::contracts::danmaku::Danmaku;
use model::{Request, Timeline};
use nsh_core::prelude::*;
pub(crate) use preferences::DanmakuPreferences;

#[derive(Clone, Copy)]
pub(in crate::features::player) struct Handle {
    enabled: Signal<bool>,
    timeline: Signal<Timeline>,
    position: Signal<(f64, f64)>,
    motion: Motion,
    preferences: DanmakuPreferences,
    error: Signal<SharedString>,
    retry: Signal<u64>,
}

pub(in crate::features::player) fn observe(
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
                let page = hongguo_service::catalog_service::danmaku(
                    &request.vid,
                    &request.series,
                    request.start,
                    request.duration,
                    &request.cursor,
                    &state,
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
    pub(in crate::features::player) fn overlay(self) -> impl IntoElement {
        overlay::build(self)
    }

    pub(in crate::features::player) fn settings(
        self,
    ) -> crate::features::player::presentation::DanmakuSettingsState {
        self.preferences.display
    }
    pub(in crate::features::player) fn enabled(self) -> Signal<bool> {
        self.enabled
    }
    pub(in crate::features::player) fn toggle_settings(self) {
        self.preferences.display.open.update(|open| *open = !*open);
    }

    pub(in crate::features::player) fn controls(
        self,
        session: SessionHandle,
        previous: Memo<ElementRef>,
    ) -> impl IntoElement {
        let logged_in = computed(move || {
            session.identity();
            session.with(|session| session.state.settings().account.is_some())
        });
        let draft = signal(String::new());
        let editor = crate::features::emoji::Editor::new(draft, 100);
        let next = element_ref();
        let pending = signal(false);
        crate::features::player::stage::composer(
            editor,
            move || pending.get(),
            crate::features::player::stage::ComposerEvents {
                emoji: Some(Box::new(move || {
                    crate::features::emoji::system_picker(editor, next).into_element()
                })),
                previous,
                next,
                send: Box::new(move || {
                    if pending.get_untracked() {
                        return;
                    }
                    let text = draft.get_untracked();
                    if text.trim().is_empty() {
                        return;
                    }
                    if !logged_in.get_untracked() {
                        self.error
                            .set(crate::i18n::tr("player.interact.loginRequired"));
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
    pub(in crate::features::player) fn toggle(self) {
        self.enabled.update(|enabled| *enabled = !*enabled);
    }
    pub(in crate::features::player) fn notice(self) -> SharedString {
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
        hongguo_service::interaction_service::send_danmaku(&vid, &series, &text, offset, &state)
            .await?;
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
