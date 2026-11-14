use crate::{
    ui,
    window_modes::{self, WindowModes},
};
use nsh_base::theme;
use nsh_core::{element::desktop, prelude::*};
use std::{
    process::{Child, Command},
    time::Duration,
};

struct Cover(Child);
impl Drop for Cover {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(crate) fn visibility_playback_probe() -> View {
    let enabled = !std::env::args().any(|value| value == "--disabled");
    let user_paused = std::env::args().any(|value| value == "--paused");
    let bridge = window_modes::install(computed(move || enabled));
    let modes = WindowModes::current().expect("probe window observer");
    let allowed = computed(move || {
        super::lifecycle::may_play(true, lifecycle(), true, enabled, modes.exposed())
    });
    let url = std::env::args().nth(1).expect("synthetic media URL");
    let directory =
        std::path::PathBuf::from(std::env::args().nth(2).expect("owned probe directory"));
    let position = signal(0.0_f32);
    let start = std::time::Instant::now();
    set_interval(Duration::from_millis(100), move || {
        use std::io::Write;
        let snapshot = serde_json::json!({"elapsed":start.elapsed().as_secs_f64(),"exposed":modes.exposed(),"allowed":allowed.get(),"position":position.get()});
        let mut log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("policy.jsonl"))
            .expect("owned policy log");
        writeln!(log, "{snapshot}").expect("write owned probe state");
    });
    let playback = super::renderer::mount(
        super::renderer::State {
            url,
            frame_source: None,
            codec_extension: None,
            control: super::renderer::Control::new(),
            rate: computed(|| 1.0),
            interval: computed(|| 100.0),
            volume: signal(0.0),
            muted: computed(|| true),
            opacity: computed(|| 1.0),
            paused: computed(move || user_paused || !allowed.get()),
            active: allowed,
            resize_mode: computed(|| hongguo_nsh_video_api::ResizeMode::Contain),
        },
        super::renderer::Callbacks {
            load: Box::new(|_| {}),
            progress: Box::new(move |event| position.set(event.current_time)),
            buffer: Box::new(|_| {}),
            error: Box::new(|error| log::error!("[NSH visibility probe] {error}")),
            end: Box::new(|| {}),
        },
    );
    ui::shell(crate::navigation::Navigation::new(), view(), playback).child(bridge)
}

pub(crate) fn visibility_preview() -> View {
    let enabled = signal(true);
    let user_paused = signal(false);
    let cover = stored(None::<Cover>);
    cleanup(move || {
        cover.replace(None);
    });
    let failure = signal(String::new());
    let bridge = window_modes::install(computed(move || enabled.get()));
    let modes = WindowModes::current().expect("visibility preview observer");
    let allowed = computed(move || {
        super::lifecycle::may_play(true, lifecycle(), true, enabled.get(), modes.exposed())
    });
    let main = desktop::current_window();
    let cover_main = main.clone();
    let minimize_main = main.clone();
    let hide_main = main;
    view()
        .flex_1()
        .p_6()
        .gap_4()
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .child(bridge)
        .child(text(move || format!("exposed:{}", modes.exposed())))
        .child(text(move || {
            format!(
                "playback:{}",
                if user_paused.get() {
                    "paused:user"
                } else if allowed.get() {
                    "playing"
                } else {
                    "paused:window"
                }
            )
        }))
        .child(text(move || failure.get()))
        .child(
            ui::button("Toggle user pause", ui::ButtonVariant::Secondary)
                .on_press(move || user_paused.update(|value| *value = !*value)),
        )
        .child(
            ui::button("Toggle auto pause", ui::ButtonVariant::Secondary)
                .on_press(move || enabled.update(|value| *value = !*value)),
        )
        .children(
            [("Cover partly", true), ("Cover fully", false)].map(move |(label, partial)| {
                let main = cover_main.clone();
                ui::button(label, ui::ButtonVariant::Secondary).on_press(move || {
                    let Some(window) = main.state() else {
                        return;
                    };
                    let bounds = window.bounds;
                    let width = if partial {
                        bounds.size.width.0 / 2.0
                    } else {
                        bounds.size.width.0 + 64.0
                    };
                    cover.replace(None);
                    match Command::new(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/../../target/nsh-visibility-cover"
                    ))
                    .args([
                        (bounds.origin.x.0 - 32.0).to_string(),
                        (bounds.origin.y.0 - 32.0).to_string(),
                        width.to_string(),
                        (bounds.size.height.0 + 64.0).to_string(),
                    ])
                    .spawn()
                    {
                        Ok(child) => {
                            cover.replace(Some(Cover(child)));
                        }
                        Err(error) => failure.set(error.to_string()),
                    }
                })
            }),
        )
        .child(
            ui::button("Minimize briefly", ui::ButtonVariant::Secondary).on_press(move || {
                let main = minimize_main.clone();
                main.minimize();
                set_timeout(Duration::from_millis(1500), move || main.restore());
            }),
        )
        .child(
            ui::button("Hide briefly", ui::ButtonVariant::Secondary).on_press(move || {
                let main = hide_main.clone();
                main.hide();
                set_timeout(Duration::from_millis(1500), move || main.show());
            }),
        )
}
