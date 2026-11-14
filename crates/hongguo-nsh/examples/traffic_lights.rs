#[cfg(target_os = "macos")]
mod probe {
    use nsh_core::{
        desktop::{TitleBarOptions, TitleBarStyle, WindowAttributes},
        prelude::*,
    };
    use nsh_platform_macos::{
        ComponentContext, Components, NativeComponent,
        objc2::{MainThreadMarker, rc::Retained},
        objc2_app_kit::{NSView, NSWindowButton},
        objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSTimer},
    };
    use serde_json::{Value, json};
    use std::{cell::Cell, rc::Rc};

    const RESTORE_CYCLES: u64 = 3;
    const SAMPLE_TICKS: u64 = 2;
    const TICK_SECONDS: f64 = 0.2;
    const ALIGNMENT_TOLERANCE: f64 = 0.5;

    #[component]
    pub fn ChromeProbe() -> impl IntoElement {
        let mut bar = TitleBarOptions::default();
        bar.style = TitleBarStyle::Hidden;
        bar.height = Some(px(44.0));
        let mut attributes = WindowAttributes::default();
        attributes.title_bar = Some(bar);
        nsh_core::element::desktop::current_window().set(attributes);
        let request = signal(Value::Null);
        let driver =
            native("hongguo.traffic-lights-probe").on_native_event(move |event: &NativeEvent| {
                if event.name.as_ref() == "step" {
                    let step = event.data.as_u64().expect("probe step");
                    request.set(json!({"serial": step, "action": {
                        "kind": "setMode", "mode": if step % 2 == 1 { "mini" } else { "normal" },
                        "aspect": 0.5625
                    }}));
                }
            });
        view()
            .flex_1()
            .child(text("Owned traffic lights regression probe"))
            .child(native("hongguo.window-modes").prop("request", move || request.get()))
            .child(driver)
    }

    struct Probe {
        view: Retained<NSView>,
        timer: Retained<NSTimer>,
    }

    impl NativeComponent for Probe {
        fn view(&self) -> Retained<NSView> {
            self.view.clone()
        }
        fn set_prop(&mut self, _: &str, _: Option<&Value>) {}
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            self.timer.invalidate();
        }
    }

    fn components(components: &Components) {
        hongguo_nsh::__nsh_native_components_macos(components);
        components.register(
            "hongguo.traffic-lights-probe",
            Rc::new(|context: ComponentContext| {
                let view = NSView::new(MainThreadMarker::new().expect("main thread"));
                let host = view.clone();
                let tick = Cell::new(0_u64);
                let baseline = Cell::new(None::<f64>);
                let block = block2::RcBlock::new(move |_| {
                    let Some(window) = host.window() else {
                        return;
                    };
                    let index = tick.get();
                    tick.set(index + 1);
                    if index.is_multiple_of(SAMPLE_TICKS) {
                        let step = index / SAMPLE_TICKS;
                        let hidden = !step.is_multiple_of(2);
                        for kind in [
                            NSWindowButton::CloseButton,
                            NSWindowButton::MiniaturizeButton,
                            NSWindowButton::ZoomButton,
                        ] {
                            assert_eq!(
                                window
                                    .standardWindowButton(kind)
                                    .expect("window control")
                                    .isHiddenOrHasHiddenAncestor(),
                                hidden
                            );
                        }
                        if !hidden {
                            let positions = [
                                NSWindowButton::CloseButton,
                                NSWindowButton::MiniaturizeButton,
                                NSWindowButton::ZoomButton,
                            ]
                            .map(|kind| {
                                let button =
                                    window.standardWindowButton(kind).expect("window control");
                                let frame = button.convertRect_toView(button.bounds(), None);
                                frame.origin.y + frame.size.height / 2.0
                            });
                            assert!(
                                (positions[0] - positions[1]).abs() < ALIGNMENT_TOLERANCE
                                    && (positions[0] - positions[2]).abs() < ALIGNMENT_TOLERANCE,
                                "traffic lights misaligned after cycle {}: {positions:?}",
                                step / 2
                            );
                            let top = window.frame().size.height - positions[0];
                            if let Some(expected) = baseline.get() {
                                assert!(
                                    (top - expected).abs() < ALIGNMENT_TOLERANCE,
                                    "traffic lights moved from {expected} to {top}"
                                );
                            } else {
                                baseline.set(Some(top));
                            }
                        }
                        if step == RESTORE_CYCLES * 2 {
                            nsh_platform_macos::quit_app();
                        } else {
                            context.emit("step", json!(step + 1));
                        }
                    }
                });
                let timer = unsafe {
                    NSTimer::timerWithTimeInterval_repeats_block(TICK_SECONDS, true, &block)
                };
                unsafe {
                    NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
                }
                Box::new(Probe { view, timer })
            }),
        );
    }

    pub fn run() -> impl std::process::Termination {
        nsh_platform_macos::set_components(components);
        let data_dir =
            std::env::temp_dir().join(format!("hongguo-traffic-lights-{}", std::process::id()));
        std::fs::create_dir(&data_dir).expect("create owned probe data");
        let result = nsh_platform_macos::run_app(
            nsh_platform_macos::AppSource::Rust(nsh_platform_macos::Root::of::<ChromeProbe>()),
            nsh_platform_macos::AppOptions {
                title: "Hongguo owned traffic lights probe".into(),
                size: (1280, 840),
                background: true,
                data_dir: Some(data_dir.clone()),
                ..Default::default()
            },
        );
        std::fs::remove_dir_all(data_dir).expect("remove owned probe data");
        result
    }
}

#[cfg(target_os = "macos")]
fn main() -> impl std::process::Termination {
    probe::run()
}
#[cfg(not(target_os = "macos"))]
fn main() {}
