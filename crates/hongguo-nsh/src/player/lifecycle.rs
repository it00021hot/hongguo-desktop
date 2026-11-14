use crate::{bridge, i18n::tr, pages::SessionHandle};
use nsh_core::platform::Lifecycle;
use nsh_core::prelude::*;

pub(super) fn may_play(
    desktop: bool,
    lifecycle: Lifecycle,
    current_route: bool,
    pause_in_background: bool,
    exposed: bool,
) -> bool {
    let foreground = if desktop {
        lifecycle != Lifecycle::Background && exposed
    } else {
        lifecycle == Lifecycle::Active
    };
    current_route && (foreground || desktop && !pause_in_background)
}

pub(super) fn delete_finished(
    session: SessionHandle,
    series: String,
    episode: u32,
    notice: Signal<SharedString>,
) {
    if !session.with(|session| session.state.settings().auto_delete_after_play) {
        return;
    }
    let state = session.with(|session| session.state.clone());
    // Begin the accepted storage operation before auto-next can dispose the
    // video's UI scope. Its completion must not depend on that scope surviving.
    let deletion = hongguo_service::runtime::spawn_blocking(move || {
        let result =
            hongguo_service::storage_service::cleanup::delete_episode(&state, &series, episode);
        if result.is_ok() {
            state
                .events()
                .publish(hongguo_service::events::ServiceEvent::DownloadQueueChanged(
                    hongguo_service::download_service::events::QueueChangedEvent {},
                ));
        }
        result
    });
    spawn(async move {
        let result = bridge::request(async move {
            deletion.await.map_err(|error| {
                hongguo_common::AppError::Internal(format!("删除已播完分集任务失败: {error}"))
            })?
        })
        .await;
        match result {
            Ok(true) => {
                notice.set(
                    tr("player.autoDeleteDone")
                        .replace("{index}", &episode.to_string())
                        .into(),
                );
                session.notify_library();
            }
            Ok(false) => {}
            Err(error) => notice.set(error.text()),
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn desktop_background_playback_respects_the_preference_but_other_routes_pause() {
        use super::Lifecycle::*;
        assert!(!super::may_play(true, Background, true, true, false));
        assert!(super::may_play(true, Background, true, false, false));
        assert!(!super::may_play(true, Active, false, false, true));
        assert!(super::may_play(true, Active, true, true, true));
        assert!(super::may_play(true, Inactive, true, true, true));
        assert!(!super::may_play(true, Inactive, true, true, false));
        assert!(!super::may_play(true, Active, true, true, false));
        assert!(super::may_play(true, Inactive, true, false, false));
    }
    #[test]
    fn mobile_background_always_pauses_and_foreground_can_resume_without_changing_user_pause() {
        use super::Lifecycle::*;
        assert!(!super::may_play(false, Background, true, false, true));
        assert!(!super::may_play(false, Background, true, true, true));
        assert!(!super::may_play(false, Inactive, true, false, true));
        assert!(super::may_play(false, Active, true, false, true));
    }
}
