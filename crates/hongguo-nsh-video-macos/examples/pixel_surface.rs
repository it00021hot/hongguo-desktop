#[cfg(target_os = "macos")]
#[path = "pixel_surface/fixture.rs"]
mod fixture;

#[cfg(target_os = "macos")]
mod probe {
    use hongguo_nsh_video_api::frames::{FrameInfo, FrameSource, PixelFrame, register_frames};
    use hongguo_nsh_video_api::{
        COMPONENT_NAME, SEEK_COMMAND, STATE_PROP, Seek, Source, VideoEvent, VideoProps,
    };
    use nsh_core::prelude::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    const WIDTH: u32 = 160;
    const HEIGHT: u32 = 90;
    const PAUSE_MS: u64 = 300;
    const PAUSE_TOLERANCE_MS: u64 = 100;
    const SEEK_MS: u64 = 1_000;
    const RESUMED_MS: u64 = 1_250;
    const RESIZE_POLL_MS: u64 = 100;
    const SIDEBAR_ANIMATION_MS: f32 = 120.0;

    struct Checker {
        frame: Arc<PixelFrame>,
        presented: AtomicBool,
    }

    impl Checker {
        fn new() -> Self {
            let mut pixels = Vec::new();
            for _ in 0..HEIGHT {
                for x in 0..WIDTH {
                    pixels.extend_from_slice(if x < WIDTH / 2 {
                        &[0, 255, 0, 255]
                    } else {
                        &[255, 0, 255, 255]
                    });
                }
            }
            Self {
                frame: Arc::new(PixelFrame {
                    serial: 1,
                    generation: 0,
                    pts_ms: 0,
                    width: WIDTH,
                    height: HEIGHT,
                    stride: WIDTH * 4,
                    pixels: pixels.into(),
                }),
                presented: AtomicBool::new(false),
            }
        }
    }

    impl FrameSource for Checker {
        fn info(&self) -> Result<Option<FrameInfo>, String> {
            Ok(Some(FrameInfo {
                width: WIDTH,
                height: HEIGHT,
            }))
        }
        fn frame_at(&self, _: u64) -> Result<Option<Arc<PixelFrame>>, String> {
            Ok(Some(self.frame.clone()))
        }
        fn seek(&self, _: u64) -> Result<u64, String> {
            self.presented.store(false, Ordering::Release);
            Ok(0)
        }
        fn presented(&self, _: u64, _: u64) -> Result<bool, String> {
            self.presented.store(true, Ordering::Release);
            Ok(true)
        }
        fn holding(&self) -> bool {
            !self.presented.load(Ordering::Acquire)
        }
        fn close(&self) {}
    }

    #[component]
    fn PixelSurface() -> impl IntoElement {
        let audio = std::env::args()
            .nth(1)
            .expect("pass an audio-only file URL");
        let mut source = Source::default();
        source.id = "pixel-surface".into();
        source.url = audio;
        source.frame_source = Some("hongguo-frames://pixel-surface".into());
        let mut props = VideoProps::default();
        props.source = Some(source);
        props.muted = true;
        props.active = true;
        props.paused = false;
        let props = signal(props);
        let resize_control = std::env::args().nth(3);
        let resizing = resize_control.is_some();
        let collapsed = signal(false);
        if let Some(control) = resize_control {
            set_interval(
                std::time::Duration::from_millis(RESIZE_POLL_MS),
                move || {
                    let value =
                        std::fs::read_to_string(&control).expect("read owned resize control");
                    let next = match value.as_str() {
                        "expanded" => false,
                        "collapsed" => true,
                        _ => panic!("invalid resize control state"),
                    };
                    if collapsed.get_untracked() != next {
                        collapsed.set(next);
                    }
                },
            );
        }
        let phase = signal(0_u8);
        let position = signal(0_u64);
        let reference = element_ref();
        let video = view()
            .flex_1()
            .on_layout(move |bounds: &Bounds<Points>| {
                if resizing {
                    log::info!(
                        "PIXEL_LAYOUT {}x{}",
                        bounds.size.width.0,
                        bounds.size.height.0
                    );
                }
            })
            .child(
                native(COMPONENT_NAME)
                    .flex_1()
                    .element_ref(&reference)
                    .prop(STATE_PROP, move || {
                        serde_json::to_value(props.get()).unwrap()
                    })
                    .on_native_event(move |event: &nsh_core::element::NativeEvent| {
                        match serde_json::from_value::<VideoEvent>(event.data.clone())
                            .expect("typed native event")
                        {
                            VideoEvent::Error { code, detail, .. } => {
                                panic!("pixel surface regression: {code:?}: {detail}")
                            }
                            VideoEvent::Progress { position_ms, .. } => {
                                position.set(position_ms);
                                if phase.get_untracked() == 0 && position_ms >= PAUSE_MS {
                                    phase.set(1);
                                    props.update(|props| props.paused = true);
                                    spawn(async move {
                                        timer(std::time::Duration::from_millis(PAUSE_MS)).await;
                                        assert!(
                                            position.get_untracked().abs_diff(position_ms)
                                                <= PAUSE_TOLERANCE_MS,
                                            "audio clock advanced while paused"
                                        );
                                        let mut request = Seek::default();
                                        request.source_id = "pixel-surface".into();
                                        request.request_id = "paused-seek".into();
                                        request.position_ms = SEEK_MS;
                                        reference.command(
                                            SEEK_COMMAND,
                                            serde_json::to_value(request).unwrap(),
                                        );
                                    });
                                } else if phase.get_untracked() == 2 && position_ms >= RESUMED_MS {
                                    phase.set(3);
                                    if resizing {
                                        props.update(|props| props.paused = true);
                                    }
                                    log::info!("PIXEL_SCENARIO_COMPLETE");
                                }
                            }
                            VideoEvent::Seeked {
                                request_id,
                                position_ms,
                                ..
                            } if request_id == "paused-seek" => {
                                assert!(
                                    position_ms.abs_diff(SEEK_MS) <= PAUSE_TOLERANCE_MS,
                                    "native seek did not reach the requested time"
                                );
                                assert!(props.get_untracked().paused);
                                spawn(async move {
                                    timer(std::time::Duration::from_millis(PAUSE_MS)).await;
                                    phase.set(2);
                                    props.update(|props| props.paused = false);
                                });
                            }
                            _ => {}
                        }
                    }),
            );
        if resizing {
            view()
                .flex_1()
                .flex_row()
                .child(
                    view()
                        .flex_shrink_0()
                        .w(move || px(if collapsed.get() { 56.0 } else { 224.0 }))
                        .transition_all()
                        .duration(move || {
                            if reduce_motion() {
                                0.0
                            } else {
                                SIDEBAR_ANIMATION_MS
                            }
                        }),
                )
                .child(video)
        } else {
            video
        }
    }

    pub fn run() -> impl std::process::Termination {
        env_logger::init();
        let frames: Arc<dyn FrameSource> = match std::env::args().nth(2).as_deref() {
            None | Some("checker") => Arc::new(Checker::new()),
            Some("hevc") => Arc::new(super::fixture::Softdec::new()),
            _ => panic!("mode must be checker or hevc"),
        };
        let _frames = register_frames("pixel-surface", frames).unwrap();
        nsh_platform_macos::set_components(hongguo_nsh_video_macos::register);
        nsh_platform_macos::run_app(
            nsh_platform_macos::AppSource::Rust(
                nsh_platform_macos::Root::of::<PixelSurface>()
                    .app_id("com.hongguo.nsh.pixel-surface"),
            ),
            nsh_platform_macos::AppOptions {
                title: "NSH pixel surface regression".into(),
                size: (640, 400),
                ..Default::default()
            },
        )
    }
}

#[cfg(target_os = "macos")]
fn main() -> impl std::process::Termination {
    probe::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {}
