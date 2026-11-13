use super::{ControlState, Info, RailState, controls, info, rail, surface};
use nsh_core::{element::ImageSrc, prelude::*};

/// The same stage, information, transport and rail as production, with fixed
/// review data. No player, session, database or request is created here.
pub(crate) fn fixture(frame: ImageSrc, on_open_details: impl Fn() + Copy + 'static) -> View {
    build(frame, on_open_details, FixtureMode::Static)
}

pub(crate) fn fixture_with_motion(
    frame: ImageSrc,
    on_open_details: impl Fn() + Copy + 'static,
) -> View {
    build(frame, on_open_details, FixtureMode::Motion)
}

pub(crate) fn fixture_with_window_modes(
    frame: ImageSrc,
    on_open_details: impl Fn() + Copy + 'static,
) -> View {
    build(frame, on_open_details, FixtureMode::Window)
}

#[derive(Clone, Copy)]
enum FixtureMode {
    Static,
    Motion,
    Window,
}

fn build(frame: ImageSrc, on_open_details: impl Fn() + Copy + 'static, mode: FixtureMode) -> View {
    let geometry =
        use_context::<crate::ui::PlayerGeometry>().unwrap_or_else(|| crate::ui::PlayerGeometry {
            source: signal((720.0, 1280.0)),
            viewport: signal((0.0, 0.0)),
            comment_anchor: element_ref(),
            comment_trigger: element_ref(),
        });
    provide_context(geometry);
    let modes = matches!(mode, FixtureMode::Window)
        .then(crate::window_modes::WindowModes::current)
        .flatten();
    let position = signal((42.0, 169.0));
    let paused = signal(true);
    let rate = signal(1.0_f32);
    let definition = signal(Some(720_u32));
    let current = signal(1_u32);
    let in_binge = signal(false);
    let download_open = signal(false);
    let volume = signal(1.0_f32);
    let muted = signal(false);
    let menu: serde_json::Value =
        serde_json::from_str(include_str!("../../../preview/fixtures/player-menu.json"))
            .expect("player menu fixture");
    let seasons = signal(
        menu["seasons"]
            .as_array()
            .expect("fixture seasons")
            .iter()
            .map(|item| super::Season {
                id: item["id"].as_str().expect("fixture id").into(),
                title: item["title"].as_str().expect("fixture title").into(),
                cover: concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../target/nsh-layout-fixtures/player-frame.png"
                )
                .into(),
                tag: item["tag"].as_str().expect("fixture tag").into(),
                episodes: item["episodes"].as_u64().expect("fixture episodes") as u32,
                play_count: item["playCount"].as_i64().expect("fixture plays"),
                current: item["current"].as_bool().expect("fixture current"),
                reserved: item["reserved"].as_bool().expect("fixture reserved"),
            })
            .collect::<Vec<_>>(),
    );
    let definitions = stored(
        serde_json::from_value::<Vec<u32>>(menu["definitions"].clone())
            .expect("fixture definitions"),
    );
    let downloaded = stored(
        serde_json::from_value::<Vec<u32>>(menu["downloaded"].clone()).expect("fixture downloads"),
    );
    let model = signal(Info {
        title: "装穷提亲，娶到全家嫌弃的她".into(),
        episode: 1,
        total: 80,
        intro: "一次平凡的相遇，让两个原本毫无交集的人走到一起。他隐藏身份上门提亲，却意外发现了这个家庭不为人知的故事。".into(),
        heat: "4105万热度".into(),
        season: "第1季".into(),
        badge: String::new(),
    });
    let rail_model = signal(RailState {
        likes: 340_000,
        collections: 1_700_000,
        comments: 698,
        ..Default::default()
    });
    let controls_model = ControlState {
        geometry,
        caption: computed(move || model.get().into()),
        paused,
        rate,
        position,
        quality: computed(move || format!("{}P", definition.get().unwrap_or(720)).into()),
        busy: computed(|| false),
        seek_pending: computed(|| false),
        has_previous: computed(|| false),
        has_next: computed(|| true),
        episode_count: computed(|| 80),
        can_binge: true,
        in_binge: computed(move || in_binge.get()),
        advanced: super::AdvancedState {
            enhancement: computed(|| false),
            window_modes: computed(move || modes.is_some_and(|modes| !modes.pending())),
            incognito: computed(move || modes.is_some_and(|modes| modes.state().incognito)),
        },
        danmaku_settings: crate::ui::DanmakuSettingsState::new(),
        danmaku_enabled: signal(true),
        menus: super::MenuState {
            open: signal(None),
            definitions: computed(move || {
                definitions.with(|values| {
                    values
                        .iter()
                        .map(|value| hongguo_common::model::VideoDefinition {
                            value: *value,
                            width: *value,
                            height: *value * 16 / 9,
                        })
                        .collect()
                })
            }),
            selected_definition: computed(move || definition.get()),
            seasons: super::SeasonState {
                items: computed(move || seasons.get()),
                loading: computed(|| false),
                error: computed(SharedString::default),
                reserving: computed(|| false),
                downloaded: computed(move || downloaded.get()),
            },
            episodes: computed(|| (1..=80).collect()),
            current_episode: computed(move || current.get()),
            volume,
            muted,
        },
    };
    let frame = stored(frame);
    let resize_mode = crate::ui::player_resize_mode(
        geometry.source,
        computed(move || {
            modes.is_some_and(|modes| modes.state().mode == crate::window_modes::Mode::Mini)
        }),
    );
    let frame = image(move || {
        let (width, height) = geometry.source.get();
        if width > height {
            ImageSrc::svg_markup(include_str!("../../../preview/fixtures/landscape.svg"))
        } else {
            frame.get()
        }
    })
    .size_full()
    .object_fit(move || match resize_mode.get() {
        hongguo_nsh_video_api::ResizeMode::Cover => nsh_core::ResizeMode::Cover,
        hongguo_nsh_video_api::ResizeMode::Contain => nsh_core::ResizeMode::Contain,
        hongguo_nsh_video_api::ResizeMode::Stretch => nsh_core::ResizeMode::Stretch,
    });
    let video = if matches!(mode, FixtureMode::Motion) {
        let ready = signal(false);
        paused.set(false);
        rate.set(2.0);
        set_interval(std::time::Duration::from_millis(250), move || {
            if ready.get_untracked() && !paused.get_untracked() {
                position.update(|position| position.0 += f64::from(rate.get_untracked()) * 0.25);
            }
        });
        view()
            .size_full()
            .child(frame)
            .child(super::super::interaction::danmaku::preview_layer(
                controls_model.danmaku_settings,
                controls_model.danmaku_enabled,
                position,
                paused,
                ready,
            ))
            .into_element()
    } else {
        frame.into_element()
    };
    let action = move |action| {
        use super::ControlAction;
        match action {
            ControlAction::EnterBinge => in_binge.set(true),
            ControlAction::Seek(at) => position.update(|position| position.0 = at),
            ControlAction::SetRate(value) => rate.set(value),
            ControlAction::SetDefinition(value) => definition.set(value),
            ControlAction::SelectEpisode(value) => {
                current.set(value);
                model.update(|model| model.episode = value);
                position.update(|position| position.0 = 0.0);
            }
            ControlAction::SetVolume(value) => {
                volume.set(value);
                muted.set(false);
            }
            ControlAction::ToggleMute => muted.update(|muted| *muted = !*muted),
            ControlAction::Download => download_open.set(true),
            ControlAction::Danmaku => controls_model
                .danmaku_enabled
                .update(|enabled| *enabled = !*enabled),
            ControlAction::Advanced(super::AdvancedControl::DanmakuSettings) => controls_model
                .danmaku_settings
                .open
                .update(|open| *open = !*open),
            ControlAction::Next => {
                current.update(|episode| *episode = (*episode + 1).min(80));
                model.update(|model| model.episode = current.get_untracked());
                position.update(|position| position.0 = 0.0);
            }
            ControlAction::Previous => {
                current.update(|episode| *episode = episode.saturating_sub(1).max(1));
                model.update(|model| model.episode = current.get_untracked());
                position.update(|position| position.0 = 0.0);
            }
            ControlAction::Fullscreen => {
                if let Some(modes) = modes {
                    let (width, height) = geometry.source.get_untracked();
                    if width > height {
                        modes.set_landscape(!modes.state().landscape);
                    } else {
                        modes.toggle_fullscreen();
                    }
                }
            }
            ControlAction::Advanced(control) => {
                if let Some(modes) = modes {
                    controls_model.menus.open.set(None);
                    controls_model.danmaku_settings.open.set(false);
                    match control {
                        super::AdvancedControl::MiniWindow => modes.toggle_mini(),
                        super::AdvancedControl::Incognito => modes.toggle_incognito(),
                        super::AdvancedControl::Portrait => modes
                            .toggle_portrait(crate::window_modes::portrait_aspect(720.0, 1280.0)),
                        super::AdvancedControl::WebFullscreen => modes.toggle_frameless(),
                        _ => {}
                    }
                }
            }
            ControlAction::ReserveSeason(id) => seasons.update(|items| {
                if let Some(item) = items.iter_mut().find(|item| item.id == id) {
                    item.reserved = true;
                }
            }),
            ControlAction::SwitchSeason(id) => {
                if let Some(item) =
                    seasons.with(|items| items.iter().find(|item| item.id == id).cloned())
                {
                    model.update(|model| {
                        model.title = item.title;
                        model.episode = 1;
                        model.total = item.episodes;
                    });
                    current.set(1);
                    seasons.update(|items| {
                        for item in items {
                            item.current = item.id == id;
                        }
                    });
                    controls_model.menus.open.set(None);
                }
            }
            _ => {}
        }
    };
    surface(
        video,
        info(model, on_open_details),
        rail(rail_model, move |action| {
            use super::RailAction;
            rail_model.update(|model| match action {
                RailAction::Like => model.liked = !model.liked,
                RailAction::Collect => model.collected = !model.collected,
                _ => {}
            });
        }),
        view()
            .absolute()
            .inset_0()
            .pointer_events(PointerEvents::BoxNone)
            .child(
                controls(controls_model, action, move |previous| {
                    let draft = signal(String::new());
                    let editor = crate::emoji::Editor::new(draft, 100);
                    let next = element_ref();
                    super::composer(
                        editor,
                        false,
                        super::ComposerEvents {
                            send: Box::new(|| {}),
                            emoji: Some(crate::emoji::system_picker(editor, next).into_element()),
                            previous,
                            next,
                        },
                    )
                })
                .style_when(
                    move || {
                        modes.is_some_and(|modes| {
                            modes.state().mode != crate::window_modes::Mode::Normal
                        })
                    },
                    |style| style.hidden(),
                ),
            )
            .child(if modes.is_some() {
                super::super::window_controls::controls(
                    controls_model,
                    computed(move || model.get()),
                    signal((720.0, 1280.0)),
                    action,
                )
            } else {
                view()
            }),
        view(),
    )
    .child(super::super::download::fixture(download_open, current))
}
