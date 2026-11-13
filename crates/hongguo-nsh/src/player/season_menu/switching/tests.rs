use super::*;
use crate::player::relay::SourceKey;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn SwitchHost() -> impl IntoElement {
    let current = signal(Origin {
        identity: 0,
        selection: ("one".into(), 8),
        source: Some(SourceEpoch {
            key: SourceKey {
                series_id: "one".into(),
                episode: 8,
                media: "lease".into(),
            },
            feed_revision: 0,
        }),
    });
    let pending = signal(false);
    let message = signal(SharedString::default());
    let selected = signal(String::new());
    let sender = stored(Vec::<tokio::sync::oneshot::Sender<()>>::new());
    let requests = signal(0_u32);
    let loader: Loader = Rc::new(move |_id| {
        let (send, receive) = tokio::sync::oneshot::channel();
        sender.with(|senders| senders.push(send));
        requests.update(|requests| *requests += 1);
        Box::pin(async move {
            receive.await.unwrap();
            Ok((
                Series {
                    series_id: "second".into(),
                    ..Default::default()
                },
                9,
            ))
        })
    });
    let controller = Controller::new(computed(move || current.get()), pending, message, loader);
    view()
        .child(
            crate::ui::button("Switch", crate::ui::ButtonVariant::Quiet).on_press(move || {
                controller.start("second".into(), move |series, episode| {
                    selected.set(format!("{}:{episode}", series.series_id))
                })
            }),
        )
        .child(
            crate::ui::button("Complete", crate::ui::ButtonVariant::Quiet).on_press(move || {
                if let Some(sender) =
                    sender.with(|senders| (!senders.is_empty()).then(|| senders.remove(0)))
                {
                    let _ = sender.send(());
                }
            }),
        )
        .child(
            crate::ui::button("Media", crate::ui::ButtonVariant::Quiet).on_press(move || {
                current
                    .update(|current| current.source.as_mut().unwrap().key.media.push_str("-new"))
            }),
        )
        .child(
            crate::ui::button("Feed", crate::ui::ButtonVariant::Quiet).on_press(move || {
                current.update(|current| current.source.as_mut().unwrap().feed_revision += 1)
            }),
        )
        .child(
            crate::ui::button("Episode", crate::ui::ButtonVariant::Quiet)
                .on_press(move || current.update(|current| current.selection.1 += 1)),
        )
        .child(
            crate::ui::button("Account", crate::ui::ButtonVariant::Quiet)
                .on_press(move || current.update(|current| current.identity += 1)),
        )
        .child(
            crate::ui::button("Leave", crate::ui::ButtonVariant::Quiet)
                .on_press(move || current.update(|current| current.source = None)),
        )
        .child(text(move || {
            format!("pending:{} selected:{}", pending.get(), selected.get())
        }))
        .child(text(move || format!("requests:{}", requests.get())))
}

fn app() -> Result<Headless, String> {
    let mut app = Headless::start(EngineConfig::new(Root::of::<SwitchHost>()))
        .map_err(|error| error.to_string())?;
    app.wait_for("Switch")?;
    Ok(app)
}

#[test]
fn a_slow_manual_season_uses_full_archive_and_rejects_later_source_episode_account_or_route()
-> Result<(), String> {
    for change in [
        None,
        Some("Media"),
        Some("Episode"),
        Some("Account"),
        Some("Leave"),
        Some("Feed"),
    ] {
        let mut app = app()?;
        app.press("Switch")?;
        app.wait_for("text:pending:true selected:")?;
        app.press("Switch")?;
        if let Some(change) = change {
            app.press(change)?;
        }
        app.press("Complete")?;
        app.wait_for(if change.is_some() {
            "text:pending:false selected:"
        } else {
            "text:pending:false selected:second:9"
        })?;
    }
    Ok(())
}

#[test]
fn source_change_releases_old_busy_immediately_and_old_completion_cannot_clear_new_busy()
-> Result<(), String> {
    for change in ["Media", "Episode", "Account", "Feed"] {
        let mut app = app()?;
        app.press("Switch")?;
        app.wait_for("text:requests:1")?;
        app.press(change)?;
        app.press("Switch")?;
        app.wait_for("text:requests:2")?;
        app.press("Complete")?;
        app.wait_for("text:pending:true selected:")?;
        app.press("Complete")?;
        app.wait_for("text:pending:false selected:second:9")?;
    }
    Ok(())
}
