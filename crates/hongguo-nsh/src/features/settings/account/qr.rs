use std::time::{Duration, Instant};

use hongguo_service::login_service::qr::{self, Status};
use nsh_base::theme;
use nsh_core::{
    element::{ImageSrc, Spawned},
    prelude::*,
};

use crate::{bridge, events::SessionHandle, i18n::tr, navigation::AccountPrompt, ui};

const POLL_INTERVAL: Duration = Duration::from_millis(1800);
const ATTEMPT_LIMIT: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Loading,
    Waiting,
    Scanned,
    Expired,
    Cancelled,
    Failed,
}

impl Phase {
    fn terminal(self) -> bool {
        matches!(self, Self::Expired | Self::Cancelled | Self::Failed)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Loading => "settings.loginQrLoading",
            Self::Waiting => "settings.loginQrWaiting",
            Self::Scanned => "settings.loginQrScanned",
            Self::Expired => "settings.loginQrExpired",
            Self::Cancelled => "settings.loginQrCancelled",
            Self::Failed => "settings.loginQrFailed",
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct State {
    owner: Owner,
    task: Stored<Option<Spawned<()>>>,
    selected: Signal<bool>,
    generation: Signal<u64>,
    phase: Signal<Phase>,
    image: Signal<SharedString>,
}

impl State {
    pub(super) fn new() -> Self {
        Self {
            owner: Owner::current(),
            task: stored(None),
            selected: signal(false),
            generation: signal(0),
            phase: signal(Phase::Loading),
            image: signal(SharedString::default()),
        }
    }

    pub(super) fn close(self) {
        self.cancel_request();
        self.reset();
    }

    fn reset(self) {
        self.generation.update(|generation| *generation += 1);
        self.selected.set(false);
        self.image.set(SharedString::default());
    }

    fn start(self, session: SessionHandle, prompt: AccountPrompt) {
        self.cancel_request();
        self.generation.update(|generation| *generation += 1);
        let generation = self.generation.get_untracked();
        self.phase.set(Phase::Loading);
        self.image.set(SharedString::default());
        self.selected.set(true);
        let app = session.with(|session| session.state.clone());
        let active = move || self.generation.get_untracked() == generation && prompt.open();
        let task = self.owner.run(move || {
            spawn(async move {
                let app_for_start = app.clone();
                let result = bridge::request(async move { qr::start(&app_for_start).await }).await;
                if !active() {
                    return;
                }
                let Ok((mut attempt, image)) = result else {
                    self.phase.set(Phase::Failed);
                    return;
                };
                self.image.set(image.into());
                self.phase.set(Phase::Waiting);
                let deadline = Instant::now() + ATTEMPT_LIMIT;
                while Instant::now() < deadline {
                    timer(POLL_INTERVAL).await;
                    if !active() {
                        return;
                    }
                    let poll_app = app.clone();
                    let result = bridge::request(async move {
                        let status = attempt.poll(&poll_app).await?;
                        Ok((attempt, status))
                    })
                    .await;
                    if !active() {
                        return;
                    }
                    let Ok((next, status)) = result else {
                        self.terminal(Phase::Failed);
                        return;
                    };
                    attempt = next;
                    match status {
                        Status::Waiting => self.phase.set(Phase::Waiting),
                        Status::Scanned => self.phase.set(Phase::Scanned),
                        Status::Rotated(image) => {
                            self.image.set(image.into());
                            self.phase.set(Phase::Waiting);
                        }
                        Status::Cancelled => {
                            self.terminal(Phase::Cancelled);
                            return;
                        }
                        Status::Expired => {
                            self.terminal(Phase::Expired);
                            return;
                        }
                        Status::Confirmed(confirmed) => {
                            if qr::finish(&app, confirmed).is_err() {
                                self.terminal(Phase::Failed);
                                return;
                            }
                            self.reset();
                            prompt.set_open(false);
                            session.notify_identity();
                            session.notify_library();
                            return;
                        }
                    }
                }
                if active() {
                    self.terminal(Phase::Expired);
                }
            })
        });
        self.task.replace(Some(task));
    }

    fn cancel_request(self) {
        if let Some(task) = self.task.replace(None) {
            task.cancel();
        }
    }

    fn terminal(self, phase: Phase) {
        self.image.set(SharedString::default());
        self.phase.set(phase);
    }
}

pub(super) fn login_methods(
    scan: State,
    sms: super::State,
    dispatch: impl Fn(super::Action) + Copy + 'static,
    session: SessionHandle,
    prompt: AccountPrompt,
) -> View {
    let selected = scan.selected;
    view()
        .gap_3()
        .child(when(
            move || sms.mfa.get().is_none(),
            move || {
                view()
                    .flex_row()
                    .gap_2()
                    .child(
                        ui::button(
                            move || tr("settings.loginByPhone"),
                            ui::ButtonVariant::Secondary,
                        )
                        .flex_1()
                        .disabled(move || !selected.get())
                        .on_press(move || scan.close()),
                    )
                    .child(
                        ui::button(
                            move || tr("settings.loginByQr"),
                            ui::ButtonVariant::Secondary,
                        )
                        .flex_1()
                        .disabled(move || selected.get() || sms.busy.get())
                        .on_press(move || scan.start(session, prompt)),
                    )
            },
        ))
        .child(when(
            move || !selected.get(),
            move || super::view::form(sms, dispatch),
        ))
        .child(when(
            move || selected.get(),
            move || {
                view()
                    .items_center()
                    .gap_3()
                    .child(when(
                        move || !scan.image.get().is_empty(),
                        move || {
                            view().p_2().bg(rgb(0xffffff)).child(
                                image(move || ImageSrc::from(scan.image.get()))
                                    .size(px(176.0))
                                    .accessible(false),
                            )
                        },
                    ))
                    .child(
                        text(move || tr(scan.phase.get().label()))
                            .text_sm()
                            .text_color(move || theme().muted_foreground),
                    )
                    .child(when(
                        move || scan.phase.get().terminal(),
                        move || {
                            ui::button(
                                move || tr("settings.loginQrRefresh"),
                                ui::ButtonVariant::Primary,
                            )
                            .on_press(move || scan.start(session, prompt))
                        },
                    ))
            },
        ))
}
