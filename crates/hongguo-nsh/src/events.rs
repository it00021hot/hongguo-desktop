use crate::bootstrap::Session;
use hongguo_service::events::ServiceEvent;
use hongguo_service::login_service::events::LoginMfaStateEvent;
use nsh_core::prelude::*;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;

#[derive(Clone, Copy)]
pub(crate) struct SessionHandle {
    session: Stored<Arc<Session>>,
    identity: Signal<u64>,
    downloads: Signal<u64>,
    merges: Signal<u64>,
    library: Signal<u64>,
    follow_notices: Signal<u64>,
    notice_preferences: crate::library::NoticePreferences,
    notifications: crate::notifications::Notifications,
    login_notice: Signal<SharedString>,
    playback: crate::player::PreferenceSignals,
    social: crate::player::SocialMemory,
    emoji_collections: crate::emoji::CollectionMemory,
    danmaku: crate::player::DanmakuPreferences,
}

impl SessionHandle {
    pub(crate) fn new(session: Arc<Session>) -> Self {
        let mut events = session.state.events().subscribe();
        session.start_notice_checks();
        let handle = Self {
            session: stored(session),
            identity: signal(0),
            downloads: signal(0),
            merges: signal(0),
            library: signal(0),
            follow_notices: signal(0),
            notice_preferences: crate::library::NoticePreferences::new(),
            notifications: crate::notifications::Notifications::current(),
            login_notice: signal(SharedString::default()),
            playback: crate::player::PreferenceSignals::new(),
            social: crate::player::SocialMemory::new(),
            emoji_collections: crate::emoji::CollectionMemory::new(),
            danmaku: crate::player::DanmakuPreferences::new(),
        };
        spawn(async move {
            loop {
                match events.recv().await {
                    Ok(ServiceEvent::DeviceRegistered(_)) => {
                        handle.identity.update(|version| *version += 1)
                    }
                    Ok(ServiceEvent::LoginMfa(event)) => {
                        handle.login_notice.set(match event {
                            LoginMfaStateEvent::Success { name } => {
                                crate::i18n::tr("settings.loginSuccess")
                                    .replace("{name}", &name)
                                    .into()
                            }
                            LoginMfaStateEvent::Waiting { server_tip } => server_tip.into(),
                            LoginMfaStateEvent::Failed {
                                message_kind,
                                message,
                            } => {
                                let localized = crate::i18n::tr(&message_kind);
                                if localized.as_str() == message_kind && !message.is_empty() {
                                    message.into()
                                } else {
                                    localized
                                }
                            }
                        });
                        handle.notify_identity();
                    }
                    Ok(
                        ServiceEvent::DownloadTaskAdded(_)
                        | ServiceEvent::DownloadProgress(_)
                        | ServiceEvent::DownloadCompleted(_)
                        | ServiceEvent::DownloadFailed(_)
                        | ServiceEvent::DownloadStopped(_)
                        | ServiceEvent::DownloadQueueChanged(_),
                    ) => handle.notify_downloads(),
                    Ok(
                        ServiceEvent::MergeTaskAdded(_)
                        | ServiceEvent::MergeProgress(_)
                        | ServiceEvent::MergeCompleted(_)
                        | ServiceEvent::MergeFailed(_),
                    ) => handle.notify_merges(),
                    Ok(ServiceEvent::FollowNotices(event)) => {
                        handle.notify_follow_notices();
                        if handle.notice_preferences.enabled.get_untracked() {
                            spawn(async move {
                                for notice in event.notices {
                                    if let Err(error) = handle
                                        .notifications
                                        .send(
                                            &format!("follow-{}", notice.id),
                                            &notice.title,
                                            &notice.detail,
                                        )
                                        .await
                                    {
                                        log::warn!(
                                            "[NSH follow notices] system delivery failed: {error}"
                                        );
                                    }
                                }
                            });
                        }
                    }
                    Ok(_) => {}
                    Err(RecvError::Lagged(count)) => {
                        log::warn!(
                            "[NSH events] subscriber lagged by {count}; refreshing snapshots"
                        );
                        handle.notify_identity();
                        handle.notify_downloads();
                        handle.notify_merges();
                        handle.notify_library();
                        handle.notify_follow_notices();
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });
        handle
    }

    pub(crate) fn with<R>(self, read: impl FnOnce(&mut Arc<Session>) -> R) -> R {
        self.session.with(read)
    }
    pub(crate) fn identity(self) -> u64 {
        self.identity.get()
    }
    pub(crate) fn playback_preferences(self) -> crate::player::PreferenceSignals {
        self.playback
    }
    pub(crate) fn social_memory(self) -> crate::player::SocialMemory {
        self.social
    }
    pub(crate) fn emoji_collections(self) -> crate::emoji::CollectionMemory {
        self.emoji_collections
    }
    pub(crate) fn danmaku_preferences(self) -> crate::player::DanmakuPreferences {
        self.danmaku
    }
    pub(crate) fn downloads(self) -> u64 {
        self.downloads.get()
    }
    pub(crate) fn notify_downloads(self) {
        self.downloads.update(|version| *version += 1);
    }
    pub(crate) fn merges(self) -> u64 {
        self.merges.get()
    }
    pub(crate) fn notify_merges(self) {
        self.merges.update(|version| *version += 1);
    }
    pub(crate) fn library(self) -> u64 {
        self.library.get()
    }
    pub(crate) fn follow_notices(self) -> u64 {
        self.follow_notices.get()
    }
    pub(crate) fn notice_preferences(self) -> crate::library::NoticePreferences {
        self.notice_preferences
    }
    pub(crate) fn notifications(self) -> crate::notifications::Notifications {
        self.notifications
    }
    pub(crate) fn notify_follow_notices(self) {
        self.follow_notices.update(|version| *version += 1);
        self.notify_library();
    }
    pub(crate) fn notify_identity(self) {
        self.identity.update(|version| *version += 1);
    }
    pub(crate) fn notify_library(self) {
        self.library.update(|version| *version += 1);
    }
    pub(crate) fn login_notice(self) -> SharedString {
        self.login_notice.get()
    }
    pub(crate) fn clear_login_notice(self) {
        self.login_notice.set(SharedString::default());
    }
}
