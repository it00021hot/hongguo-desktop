use crate::{bridge, features::SessionHandle, i18n::tr};
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
    let allowed = current_route && (foreground || desktop && !pause_in_background);
    log::debug!(
        "[NSH player] stage=playbackGate lifecycle={lifecycle:?} desktop={desktop} currentRoute={current_route} pauseInBackground={pause_in_background} exposed={exposed} allowed={allowed}"
    );
    allowed
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
