#[cfg(target_os = "macos")]
mod probe {
    use nsh_platform_macos::{
        objc2::{MainThreadMarker, msg_send, sel},
        objc2_app_kit::{NSApplication, NSView, NSWindowOcclusionState},
        objc2_foundation::{NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes, NSTimer},
    };
    use std::{
        cell::RefCell,
        fs::OpenOptions,
        io::Write,
        process::{Child, Command},
        rc::Rc,
        time::Instant,
    };

    fn rate(view: &NSView) -> Option<f32> {
        if let Some(layer) = view.layer() {
            unsafe {
                if layer.respondsToSelector(sel!(player)) {
                    let player: *mut nsh_platform_macos::objc2::runtime::AnyObject =
                        msg_send![&*layer, player];
                    if !player.is_null() {
                        return Some(msg_send![player, rate]);
                    }
                }
            }
        }
        view.subviews().iter().find_map(|child| rate(&child))
    }

    pub(super) fn run() -> impl std::process::Termination {
        env_logger::init();
        let directory =
            std::path::PathBuf::from(std::env::args().nth(2).expect("owned probe directory"));
        let data_dir = directory.join("data");
        let mtm = MainThreadMarker::new().expect("probe main thread");
        let application = NSApplication::sharedApplication(mtm);
        let start = Instant::now();
        let child = Rc::new(RefCell::new(None::<Child>));
        let held_child = child.clone();
        let block = block2::RcBlock::new(move |_| {
            for window in application.windows().iter() {
                if window.title().to_string() != "Hongguo owned visibility playback probe" {
                    continue;
                }
                let actual_rate = window.contentView().and_then(|view| rate(&view));
                let exposed = window
                    .occlusionState()
                    .contains(NSWindowOcclusionState::Visible);
                let elapsed = start.elapsed().as_secs_f64();
                let snapshot = serde_json::json!({"elapsed":elapsed,"nativeRate":actual_rate,"nativeExposed":exposed});
                let mut log = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(directory.join("native.jsonl"))
                    .expect("owned native log");
                writeln!(log, "{snapshot}").expect("write native probe sample");
                if elapsed > 0.5 && actual_rate.is_some() && held_child.borrow().is_none() {
                    let frame = window.frame();
                    let screen = nsh_platform_macos::objc2_app_kit::NSScreen::screens(mtm)
                        .firstObject()
                        .expect("display")
                        .frame();
                    let top = screen.origin.y + screen.size.height;
                    let cover = Command::new(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/../../target/nsh-visibility-cover"
                    ))
                    .args([
                        (frame.origin.x - 32.0).to_string(),
                        (top - frame.origin.y - frame.size.height - 32.0).to_string(),
                        (frame.size.width + 64.0).to_string(),
                        (frame.size.height + 64.0).to_string(),
                    ])
                    .args(std::env::args().skip(3))
                    .spawn()
                    .expect("owned cover process");
                    held_child.replace(Some(cover));
                }
                if let Some(child) = held_child.borrow_mut().as_mut()
                    && child.try_wait().expect("owned cover status").is_some()
                {
                    window.orderFrontRegardless();
                }
            }
            if start.elapsed().as_secs() >= 7 {
                nsh_platform_macos::quit_app();
            }
        });
        let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(0.1, true, &block) };
        unsafe {
            NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
        }
        nsh_platform_macos::set_components(hongguo_nsh::__nsh_native_components_macos);
        let result = nsh_platform_macos::run_app(
            nsh_platform_macos::AppSource::Rust(
                nsh_platform_macos::Root::of::<hongguo_nsh::VisibilityPlaybackProbe>()
                    .app_id("com.hongguo.nsh.visibility-playback")
                    .tokens(
                        nsh_base::tokens_of(Some(include_str!("../theme.json")))
                            .expect("probe theme"),
                    ),
            ),
            nsh_platform_macos::AppOptions {
                title: "Hongguo owned visibility playback probe".into(),
                size: (640, 480),
                background: true,
                data_dir: Some(data_dir),
                ..Default::default()
            },
        );
        timer.invalidate();
        if let Some(mut child) = child.borrow_mut().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        result
    }
}
#[cfg(target_os = "macos")]
fn main() -> impl std::process::Termination {
    probe::run()
}
#[cfg(not(target_os = "macos"))]
fn main() {}
