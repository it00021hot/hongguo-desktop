use nsh_core::{
    element::desktop,
    notifications::{Notification, NotificationPermission},
    prelude::*,
};

#[derive(Clone, Copy)]
pub(crate) struct Notifications(Owner);

pub(crate) fn install() {
    provide_context(Notifications(Owner::current()));
    let window = desktop::current_window();
    spawn(async move {
        loop {
            next_notification_press().await;
            window.focus();
        }
    });
}

impl Notifications {
    pub(crate) fn current() -> Self {
        use_context::<Self>().expect("notifications installed before the session")
    }

    pub(crate) async fn request_permission(self) -> bool {
        let permission = self.0.run(request_notification_permission).await;
        permission == NotificationPermission::Granted
    }

    pub(crate) async fn send(self, id: &str, title: &str, body: &str) -> Result<(), String> {
        let shown = self
            .0
            .run(|| show_notification(Notification::new(id, title, body)))
            .await;
        if shown {
            Ok(())
        } else {
            Err("System notifications are disabled or unavailable".into())
        }
    }
}
