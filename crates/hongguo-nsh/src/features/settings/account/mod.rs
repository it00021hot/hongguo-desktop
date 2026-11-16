mod card;
mod controller;
pub(super) fn card() -> View {
    card::build()
}
mod model;
mod view;

use crate::{
    events::SessionHandle,
    navigation::{AccountPrompt, Navigation},
};
use model::CodeDeadlines;
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(super) enum Action {
    Send,
    Login,
    CancelMfa,
}

#[derive(Clone, Copy)]
pub(super) struct State {
    mobile: Signal<String>,
    code: Signal<String>,
    busy: Signal<bool>,
    feedback: Signal<SharedString>,
    now: Signal<i64>,
    deadlines: Signal<CodeDeadlines>,
    mfa: Memo<Option<SharedString>>,
    code_focus: Signal<u64>,
}

impl State {
    fn new(mfa: Memo<Option<SharedString>>) -> Self {
        Self {
            mobile: signal(String::new()),
            code: signal(String::new()),
            busy: signal(false),
            feedback: signal(SharedString::default()),
            now: signal(hongguo_common::utils::time::now_ms()),
            deadlines: signal(CodeDeadlines::default()),
            mfa,
            code_focus: signal(0),
        }
    }
    fn countdown(self) -> u32 {
        self.deadlines
            .with(|deadlines| deadlines.remaining(&self.mobile.get(), self.now.get()))
    }
    fn can_login(self) -> bool {
        model::can_login(&self.mobile.get(), &self.code.get(), self.busy.get())
    }
}

pub(crate) fn dialogs(session: SessionHandle) -> View {
    super::Tasks::provide();
    let account_card = card::provide(session);
    let prompt = AccountPrompt::provide();
    let mfa = computed(move || {
        session.identity();
        session.with(|session| {
            session
                .state
                .login_mfa
                .read()
                .as_ref()
                .map(|flow| hongguo_service::login_service::polling::mfa_tips(&flow.ctx).into())
        })
    });
    let state = State::new(mfa);
    let dispatch = controller::dispatch(session, state, prompt);
    clock(state, prompt);
    effect(move || {
        session.identity();
        if session.with(|session| session.state.settings().account.is_some()) {
            state.code.set(String::new());
            prompt.set_open(false);
        }
    });
    effect(move || {
        let notice = session.login_notice();
        if !notice.is_empty() {
            state.feedback.set(notice);
        }
    });
    crate::features::settings::presentation::login_dialog(
        computed(move || prompt.open()),
        move || prompt.set_open(false),
        move || view::form(state, dispatch),
    )
    .child(account_card)
}

fn clock(state: State, prompt: AccountPrompt) {
    effect(move || {
        if prompt.open() {
            state.now.set(hongguo_common::utils::time::now_ms());
            state.feedback.set(SharedString::default());
        }
    });
    set_interval(std::time::Duration::from_secs(1), move || {
        if prompt.open() && state.countdown() > 0 {
            state.now.set(hongguo_common::utils::time::now_ms());
        }
    });
}

pub(super) fn fallback(session: SessionHandle, nav: Navigation) -> Screen {
    let dialogs = dialogs(session);
    AccountPrompt::provide().request();
    crate::shell::page(nav, move || crate::i18n::tr("settings.account")).child(dialogs)
}
