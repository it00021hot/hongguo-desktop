use crate::{i18n::tr, pages::SessionHandle};
use hongguo_service::{events::ServiceEvent, play_service::compat_play::CompatPhase};
use nsh_core::prelude::*;
use tokio::sync::broadcast::error::RecvError;

pub(super) fn observe(
    session: SessionHandle,
    selection: Signal<(String, u32)>,
    definition: Signal<Option<u32>>,
    loading: impl IntoReactive<bool>,
) -> Signal<SharedString> {
    let loading = loading.into_reactive();
    let loading = computed(move || loading.get());
    let status = signal(SharedString::default());
    let mut events = session.with(|session| session.state.events().subscribe());
    effect(move || {
        selection.get();
        definition.get();
        status.set(SharedString::default());
    });
    spawn(async move {
        loop {
            match events.recv().await {
                Ok(ServiceEvent::CompatProgress(event)) => {
                    let (id, episode) = selection.get_untracked();
                    if !loading.get_untracked()
                        || !matches(&event.key, &id, episode, definition.get_untracked())
                    {
                        continue;
                    }
                    status.set(match event.phase {
                        CompatPhase::Downloading => tr("player.compatFetching"),
                        CompatPhase::Transcoding => tr("player.compatTranscoding")
                            .replace(
                                "{percent}",
                                &format!("{:.0}", event.percent.clamp(0.0, 100.0)),
                            )
                            .into(),
                        CompatPhase::Ready | CompatPhase::Completed | CompatPhase::Failed => {
                            SharedString::default()
                        }
                    });
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            }
        }
    });
    status
}

fn matches(key: &str, id: &str, episode: u32, definition: Option<u32>) -> bool {
    let base = format!("{id}:{episode}");
    match definition {
        Some(definition) => key == format!("{base}:{definition}"),
        None => {
            key == base
                || key
                    .strip_prefix(&format!("{base}:"))
                    .is_some_and(|suffix| suffix.parse::<u32>().is_ok())
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn progress_cannot_leak_from_another_episode_or_selected_definition() {
        assert!(super::matches("series:2:3", "series", 2, Some(3)));
        assert!(!super::matches("series:2:4", "series", 2, Some(3)));
        assert!(!super::matches("series:20:3", "series", 2, None));
        assert!(!super::matches("other:2:3", "series", 2, None));
        assert!(super::matches("series:2", "series", 2, None));
        assert!(super::matches("series:2:3", "series", 2, None));
        assert!(!super::matches("series:2:3:4", "series", 2, None));
    }
}
