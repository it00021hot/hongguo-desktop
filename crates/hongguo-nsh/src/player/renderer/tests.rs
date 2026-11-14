use super::*;
use hongguo_nsh_video_api::{EVENT_NAME, Enhancement, EnhancementPhase, Seek, VideoEvent};
use nsh_core::{Command, EventKind, EventPayload, NodeId};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Default)]
struct NativeBindings {
    props: Vec<(NodeId, hongguo_nsh_video_api::VideoProps)>,
    opacity: HashMap<NodeId, f32>,
}

#[test]
fn native_video_error_mounts_the_codec_extension_once_and_switches_seek_commands()
-> Result<(), String> {
    let native = Arc::new(Mutex::new(None));
    let custom = Arc::new(Mutex::new(None));
    let native_seen = native.clone();
    let custom_seen = custom.clone();
    let mut config = EngineConfig::new(Root::of::<NativeVideo>());
    config.clock = nsh_core::time::Clock::manual(std::time::SystemTime::UNIX_EPOCH);
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.observe(move |_, batch| {
        for item in &batch.items {
            match item {
                nsh_core::MountItem::NativeProps {
                    id,
                    props: nsh_core::NativeProps::Video(_),
                    ..
                } => *native_seen.lock().unwrap() = Some(*id),
                nsh_core::MountItem::ComponentProps { id, props } => {
                    for (key, value) in props {
                        if key.as_ref() == hongguo_nsh_video_api::STATE_PROP {
                            *custom_seen.lock().unwrap() = Some((
                                *id,
                                serde_json::from_value::<hongguo_nsh_video_api::VideoProps>(
                                    value.clone().unwrap(),
                                )
                                .unwrap(),
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
    });
    app.wait_for("text:extension:0")?;
    let native = native.lock().unwrap().unwrap();
    let report_error = |app: &mut Headless| {
        app.engine().control().dispatch_event(
            native,
            EventKind::Error,
            EventPayload::Error {
                message: "native codec unavailable".into(),
            },
        );
        app.run_for(Duration::from_millis(20))
    };
    report_error(&mut app)?;
    app.wait_for("text:extension:1")?;
    let (custom, props) = custom
        .lock()
        .unwrap()
        .clone()
        .expect("codec extension mounted");
    assert_ne!(native, custom);
    assert_eq!(
        props.source.unwrap().frame_source.as_deref(),
        Some("hongguo-frames://native-codec-fallback")
    );
    app.press("Seek native")?;
    let (target, request) = latest_request(&app);
    assert_eq!(target, custom);
    assert_eq!(request.position_ms, 12_500);
    report_error(&mut app)?;
    assert!(app.screen().matches("text:extension:1")?);
    Ok(())
}

#[component]
fn NativeVideo() -> impl IntoElement {
    let control = Control::new();
    let attempts = signal(0_u32);
    view()
        .child(text(move || {
            format!("extension:{count}", count = attempts.get())
        }))
        .child(
            crate::ui::button("Seek native", crate::ui::ButtonVariant::Quiet)
                .on_press(move || control.seek(12.5)),
        )
        .child(super::mount(
            State {
                url: "http://127.0.0.1:12345/media/native-fixture".into(),
                frame_source: None,
                codec_extension: Some(std::rc::Rc::new(move || {
                    attempts.update(|count| *count += 1);
                    Ok("hongguo-frames://native-codec-fallback".into())
                })),
                control,
                rate: computed(|| 1.5),
                interval: computed(|| 250.0),
                volume: signal(0.5),
                muted: computed(|| false),
                opacity: computed(|| 1.0),
                paused: computed(|| false),
                active: computed(|| true),
                resize_mode: computed(|| hongguo_nsh_video_api::ResizeMode::Contain),
            },
            Callbacks {
                load: Box::new(|_| {}),
                progress: Box::new(|_| {}),
                buffer: Box::new(|_| {}),
                error: Box::new(|_| {}),
                end: Box::new(|| {}),
            },
        ))
}

#[test]
fn ordinary_video_uses_the_nsh_video_host_and_native_seek() -> Result<(), String> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observation = seen.clone();
    let mut app = Headless::start(EngineConfig::new(Root::of::<NativeVideo>()))
        .map_err(|error| error.to_string())?;
    app.observe(move |_, batch| {
        for item in &batch.items {
            if let nsh_core::MountItem::NativeProps {
                id,
                props: nsh_core::NativeProps::Video(props),
                ..
            } = item
            {
                observation.lock().unwrap().push((*id, props.clone()));
            }
        }
    });
    app.wait_for("Seek native")?;
    let (node, props) = seen
        .lock()
        .unwrap()
        .last()
        .cloned()
        .expect("native Video mounted");
    assert_eq!(
        props.source.as_deref(),
        Some("http://127.0.0.1:12345/media/native-fixture")
    );
    assert_eq!(props.rate, 1.5);
    assert_eq!(props.volume, 0.5);
    app.press("Seek native")?;
    assert!(app.screen().commands().iter().any(|(id, command)| {
        *id == node && matches!(command, Command::Seek(seconds) if *seconds == 12.5)
    }));
    assert!(!app.screen().commands().iter().any(|(_, command)| {
        matches!(command, Command::Native { name, .. } if name.as_ref() == hongguo_nsh_video_api::SEEK_COMMAND)
    }));
    Ok(())
}

#[component]
fn ReplacedVideo() -> impl IntoElement {
    crate::ui::test_log::watch("native-source-replacement");
    let generation = signal(0_u32);
    let active = signal(true);
    view()
        .child(text(move || {
            format!("generation:{} active:{}", generation.get(), active.get())
        }))
        .child(
            crate::ui::button("Replace native source", crate::ui::ButtonVariant::Quiet).on_press(
                move || {
                    batch(|| {
                        generation.update(|generation| *generation += 1);
                        active.update(|active| *active = !*active);
                    })
                },
            ),
        )
        .child(
            crate::ui::button("Refresh native parent", crate::ui::ButtonVariant::Quiet)
                .on_press(move || active.update(|active| *active = !*active)),
        )
        .child(each(
            move || vec![generation.get()],
            |generation| *generation,
            move |generation, _| {
                let generation = generation.get_untracked();
                pixel::mount(
                    State {
                        url: format!("https://example.invalid/{generation}.mp4"),
                        frame_source: Some(format!("hongguo-frames://fixture-{generation}")),
                        codec_extension: None,
                        control: Control::new(),
                        rate: computed(|| 1.0),
                        interval: computed(|| 250.0),
                        volume: signal(1.0),
                        muted: computed(|| false),
                        opacity: computed(move || if active.get() { 1.0 } else { 0.0 }),
                        paused: computed(move || !active.get()),
                        active: computed(move || active.get()),
                        resize_mode: computed(|| hongguo_nsh_video_api::ResizeMode::Contain),
                    },
                    Callbacks {
                        load: Box::new(|_| {}),
                        progress: Box::new(|_| {}),
                        buffer: Box::new(|_| {}),
                        error: Box::new(|_| {}),
                        end: Box::new(|| {}),
                    },
                )
            },
        ))
}

#[test]
fn replacing_native_source_disposes_row_bindings_before_parent_updates() -> Result<(), String> {
    let bindings = Arc::new(Mutex::new(NativeBindings::default()));
    let observed = bindings.clone();
    let mut app = Headless::start(EngineConfig::new(Root::of::<ReplacedVideo>()))
        .map_err(|error| error.to_string())?;
    app.observe(move |_, batch| {
        let mut bindings = observed.lock().unwrap();
        for item in &batch.items {
            match item {
                nsh_core::MountItem::ComponentProps { id, props } => {
                    for (key, value) in props {
                        if key.as_ref() == hongguo_nsh_video_api::STATE_PROP
                            && let Some(value) = value
                        {
                            bindings.props.push((
                                *id,
                                serde_json::from_value(value.clone())
                                    .expect("typed pixel properties"),
                            ));
                        }
                    }
                }
                nsh_core::MountItem::ViewStyle { id, style } => {
                    bindings.opacity.insert(*id, style.opacity);
                }
                _ => {}
            }
        }
    });
    app.wait_for("text:generation:0 active:true")?;
    for generation in 1..=20 {
        app.press("Replace native source")?;
        app.wait_for(&format!("text:generation:{generation} active:false"))?;
        let current = {
            let bindings = bindings.lock().unwrap();
            let (id, props) = bindings.props.last().expect("replacement native props");
            assert_eq!(
                props.source.as_ref().unwrap().url,
                format!("https://example.invalid/{generation}.mp4")
            );
            assert_eq!(
                props.source.as_ref().unwrap().frame_source.as_deref(),
                Some(format!("hongguo-frames://fixture-{generation}").as_str())
            );
            assert!(!props.active);
            assert_eq!(bindings.opacity.get(id), Some(&0.0));
            *id
        };
        app.press("Refresh native parent")?;
        app.wait_for(&format!("text:generation:{generation} active:true"))?;
        let bindings = bindings.lock().unwrap();
        let (id, props) = bindings.props.last().unwrap();
        assert_eq!(*id, current);
        assert!(props.active);
        assert_eq!(bindings.opacity.get(id), Some(&1.0));
    }
    drop(app);
    let errors = crate::ui::test_log::take("native-source-replacement");
    assert!(errors.is_empty(), "native row errors: {errors:?}");
    Ok(())
}

#[component]
fn Video() -> impl IntoElement {
    let control = Control::new();
    let position = signal(0.0_f32);
    let metadata = signal((false, 0.0_f32));
    let active = signal(true);
    let mut settings = Enhancement::default();
    settings.anime4k_on = true;
    let enhancement = crate::player::enhancement::Controller::new(signal(settings));
    provide_context(enhancement);
    view()
        .child(text(move || format!("position:{}", position.get())))
        .child(text(move || {
            format!("loaded:{} duration:{}", metadata.get().0, metadata.get().1)
        }))
        .child(text(move || {
            format!("enhancement:{:?}", enhancement.phase.get())
        }))
        .child(
            crate::ui::button("Deactivate", crate::ui::ButtonVariant::Quiet)
                .on_press(move || active.set(false)),
        )
        .child(
            crate::ui::button("Activate", crate::ui::ButtonVariant::Quiet)
                .on_press(move || active.set(true)),
        )
        .child(
            crate::ui::button("Disable enhancement", crate::ui::ButtonVariant::Quiet)
                .on_press(move || enhancement.settings.set(Enhancement::default())),
        )
        .child(
            crate::ui::button("Change strength", crate::ui::ButtonVariant::Quiet).on_press(
                move || {
                    enhancement
                        .settings
                        .update(|settings| settings.strength = 100)
                },
            ),
        )
        .child(
            crate::ui::button("Seek first", crate::ui::ButtonVariant::Quiet)
                .on_press(move || control.seek(42.0)),
        )
        .child(
            crate::ui::button("Seek later", crate::ui::ButtonVariant::Quiet)
                .on_press(move || control.seek(84.0)),
        )
        .child(pixel::mount(
            State {
                url: "https://example.invalid/synthetic.mp4".into(),
                frame_source: None,
                codec_extension: None,
                control,
                rate: computed(|| 1.0),
                interval: computed(|| 250.0),
                volume: signal(1.0),
                muted: computed(|| false),
                opacity: computed(|| 1.0),
                paused: computed(|| true),
                active: computed(move || active.get()),
                resize_mode: computed(|| hongguo_nsh_video_api::ResizeMode::Contain),
            },
            Callbacks {
                load: Box::new(move |value| metadata.set((true, value.duration))),
                progress: Box::new(move |value| {
                    position.set(value.current_time);
                    metadata.update(|metadata| metadata.1 = value.duration);
                }),
                buffer: Box::new(|_| {}),
                error: Box::new(|_| {}),
                end: Box::new(|| {}),
            },
        ))
}

fn latest_request(app: &Headless) -> (NodeId, Seek) {
    app.screen()
        .commands()
        .iter()
        .rev()
        .find_map(|(node, command)| {
            if let Command::Native { name, args } = command
                && name.as_ref() == hongguo_nsh_video_api::SEEK_COMMAND
            {
                Some((
                    *node,
                    serde_json::from_value(args.clone()).expect("typed seek request"),
                ))
            } else {
                None
            }
        })
        .expect("seek was sent to the actual native component")
}

fn emit(app: &mut Headless, node: NodeId, event: VideoEvent) -> Result<(), String> {
    app.engine().control().dispatch_event(
        node,
        EventKind::NativeEvent,
        EventPayload::Native {
            name: EVENT_NAME.into(),
            data: serde_json::to_value(event).expect("typed video event"),
        },
    );
    app.run_for(Duration::from_millis(20))
}

fn status(source: &str, strength: u8, phase: EnhancementPhase) -> VideoEvent {
    let mut settings = Enhancement::default();
    settings.anime4k_on = true;
    settings.strength = strength;
    VideoEvent::EnhancementStatus {
        source_id: source.into(),
        phase,
        settings,
        code: None,
        detail: String::new(),
    }
}

fn app() -> Result<Headless, String> {
    let mut config = EngineConfig::new(Root::of::<Video>());
    config.clock = nsh_core::time::Clock::manual(std::time::SystemTime::UNIX_EPOCH);
    Headless::start(config).map_err(|error| error.to_string())
}

#[test]
fn unknown_native_duration_still_loads_and_reports_real_progress_then_accepts_finite_metadata()
-> Result<(), String> {
    let mut app = app()?;
    app.wait_for("text:loaded:false duration:0")?;
    app.press("Seek first")?;
    let (node, request) = latest_request(&app);
    emit(
        &mut app,
        node,
        VideoEvent::Load {
            source_id: request.source_id.clone(),
            duration_ms: None,
            width: 320,
            height: 180,
        },
    )?;
    app.wait_for("text:loaded:true duration:0")?;
    emit(
        &mut app,
        node,
        VideoEvent::Seeked {
            source_id: request.source_id.clone(),
            request_id: request.request_id,
            position_ms: 42_000,
        },
    )?;
    emit(
        &mut app,
        node,
        VideoEvent::Progress {
            source_id: request.source_id.clone(),
            duration_ms: None,
            position_ms: 43_000,
        },
    )?;
    app.wait_for("text:position:43")?;
    assert!(app.screen().matches("text:loaded:true duration:0")?);
    emit(
        &mut app,
        node,
        VideoEvent::Load {
            source_id: request.source_id,
            duration_ms: Some(169_000),
            width: 320,
            height: 180,
        },
    )?;
    app.wait_for("text:loaded:true duration:169")?;
    assert!(app.screen().matches("text:position:43")?);
    Ok(())
}

#[test]
fn inactive_or_all_off_video_clears_status_and_rejects_late_ready_or_error() -> Result<(), String> {
    let mut app = app()?;
    app.wait_for("text:position:0")?;
    app.press("Seek first")?;
    let (node, request) = latest_request(&app);
    emit(
        &mut app,
        node,
        status(&request.source_id, 50, EnhancementPhase::Ready),
    )?;
    app.wait_for("text:enhancement:Ready")?;
    app.press("Deactivate")?;
    app.wait_for("text:enhancement:Idle")?;
    emit(
        &mut app,
        node,
        status(&request.source_id, 50, EnhancementPhase::Ready),
    )?;
    assert!(app.screen().matches("text:enhancement:Idle")?);
    app.press("Activate")?;
    emit(
        &mut app,
        node,
        status(&request.source_id, 50, EnhancementPhase::Ready),
    )?;
    app.wait_for("text:enhancement:Ready")?;
    app.press("Disable enhancement")?;
    app.wait_for("text:enhancement:Idle")?;
    emit(
        &mut app,
        node,
        status(&request.source_id, 50, EnhancementPhase::Error),
    )?;
    assert!(app.screen().matches("text:enhancement:Idle")?);
    Ok(())
}

#[test]
fn same_source_status_from_old_settings_cannot_override_the_current_frame_state()
-> Result<(), String> {
    let mut app = app()?;
    app.wait_for("text:position:0")?;
    app.press("Seek first")?;
    let (node, request) = latest_request(&app);
    emit(
        &mut app,
        node,
        status(&request.source_id, 50, EnhancementPhase::Ready),
    )?;
    app.wait_for("text:enhancement:Ready")?;
    app.press("Change strength")?;
    emit(
        &mut app,
        node,
        status(&request.source_id, 50, EnhancementPhase::Error),
    )?;
    assert!(app.screen().matches("text:enhancement:Ready")?);
    emit(
        &mut app,
        node,
        status(&request.source_id, 100, EnhancementPhase::Error),
    )?;
    app.wait_for("text:enhancement:Error")?;
    Ok(())
}

#[test]
fn native_seek_requires_current_source_and_request_completion_and_rejects_queued_old_progress()
-> Result<(), String> {
    let mut app = app()?;
    app.wait_for("text:position:0")?;
    app.press("Seek first")?;
    let (node, first) = latest_request(&app);
    emit(
        &mut app,
        node,
        VideoEvent::Progress {
            source_id: first.source_id.clone(),
            position_ms: 42_000,
            duration_ms: Some(169_000),
        },
    )?;
    assert!(app.screen().matches("text:position:0")?);
    app.press("Seek later")?;
    let (_, later) = latest_request(&app);
    assert_ne!(first.request_id, later.request_id);
    emit(
        &mut app,
        node,
        VideoEvent::Seeked {
            source_id: first.source_id.clone(),
            request_id: first.request_id,
            position_ms: 42_000,
        },
    )?;
    emit(
        &mut app,
        node,
        VideoEvent::Seeked {
            source_id: "unmounted-source".into(),
            request_id: later.request_id.clone(),
            position_ms: 84_000,
        },
    )?;
    assert!(app.screen().matches("text:position:0")?);
    emit(
        &mut app,
        node,
        VideoEvent::Seeked {
            source_id: later.source_id.clone(),
            request_id: later.request_id.clone(),
            position_ms: 84_000,
        },
    )?;
    app.wait_for("text:position:84")?;
    emit(
        &mut app,
        node,
        VideoEvent::Seeked {
            source_id: later.source_id,
            request_id: later.request_id,
            position_ms: 42_000,
        },
    )?;
    assert!(app.screen().matches("text:position:84")?);
    Ok(())
}
