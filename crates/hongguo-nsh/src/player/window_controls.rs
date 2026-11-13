use super::{
    chrome::Chrome,
    stage::{ControlAction, ControlState, Info},
};
use crate::{
    ui,
    window_modes::{Mode, WindowModes, portrait_aspect},
};
use nsh_core::prelude::*;

pub(super) fn controls(
    state: ControlState,
    information: Memo<Info>,
    video_size: Signal<(f64, f64)>,
    action: impl Fn(ControlAction) + 'static,
) -> View {
    let Some(modes) = WindowModes::current() else {
        return view();
    };
    let controls = ui::window_player_controls(
        ui::WindowPlayerState {
            portrait: computed(move || modes.state().mode == Mode::Portrait),
            caption: computed(move || {
                let info = information.get();
                ui::WindowCaption {
                    title: info.title,
                    intro: info.intro,
                    episode: info.episode,
                    total: info.total,
                }
            }),
            paused: computed(move || state.paused.get()),
            rate: computed(move || state.rate.get()),
            muted: computed(move || state.menus.muted.get()),
            position: computed(move || state.position.get()),
            busy: state.busy,
            seek_pending: state.seek_pending,
            has_next: state.has_next,
            incognito: computed(move || modes.state().incognito),
            pinned: computed(move || modes.state().pinned),
            fill: computed(move || modes.state().portrait_fill),
        },
        move |intent| {
            if let Some(chrome) = Chrome::current() {
                chrome.wake();
            }
            use ui::WindowPlayerAction;
            let shared = match intent {
                WindowPlayerAction::PlayPause => {
                    state.paused.update(|paused| *paused = !*paused);
                    return;
                }
                WindowPlayerAction::Previous => ControlAction::Previous,
                WindowPlayerAction::Next => ControlAction::Next,
                WindowPlayerAction::SetRate(rate) => ControlAction::SetRate(rate),
                WindowPlayerAction::Seek(at) => ControlAction::Seek(at),
                WindowPlayerAction::ToggleMute => ControlAction::ToggleMute,
                WindowPlayerAction::ToggleIncognito => {
                    modes.toggle_incognito();
                    return;
                }
                WindowPlayerAction::TogglePinned => {
                    modes.toggle_pinned();
                    return;
                }
                WindowPlayerAction::ToggleFill => {
                    let (width, height) = video_size.get_untracked();
                    modes.toggle_portrait_fill(portrait_aspect(width, height));
                    return;
                }
                WindowPlayerAction::Exit => {
                    modes.exit();
                    return;
                }
            };
            action(shared);
        },
    );
    let controls = match Chrome::current() {
        Some(chrome) => chrome.bind_toolbar(controls),
        None => controls,
    };
    view()
        .absolute()
        .inset_0()
        .pointer_events(PointerEvents::BoxNone)
        .style_when(
            move || modes.state().mode == Mode::Normal,
            |style| style.hidden(),
        )
        .child(ui::window_drag_bar())
        .child(controls)
}
