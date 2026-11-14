#[path = "record.rs"]
mod record;
#[path = "source.rs"]
mod source;

use hongguo_nsh_video_api::frames::register_frames;
use hongguo_nsh_video_api::{
    COMPONENT_NAME, Enhancement, SEEK_COMMAND, STATE_PROP, Seek, Source as VideoSource, VideoEvent,
    VideoProps,
};
use hongguo_playback::{PlaybackSession, SessionConfig};
use nsh_core::prelude::*;
use record::{Recorder, host_time_seconds};
use serde_json::json;
use source::{FileSource, Source};
use std::{
    fs::File,
    path::PathBuf,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const APP_ID: &str = "com.hongguo.nsh.playback-probe";
const TOKEN: &str = "playback-probe";
const PROGRESS_MS: u32 = 100;
const PAUSE_AFTER_MEDIA_SECONDS: f64 = 3.0;
const PAUSE_DURATION: Duration = Duration::from_millis(750);
const PAUSE_TOLERANCE_SECONDS: f64 = 0.1;
const SEEK_TOLERANCE_MS: u64 = 150;
const RESUMED_PROGRESS_MS: u64 = 300;
pub const USAGE: &str = "Usage: playback_probe <plaintext.mp4> <new-output.jsonl> [rate=1] [seconds=30] [off|sharpen|anime4k|both] [continuous|controls]\nUses the production NSH PixelView, PlaybackSession, AudioUnit and Metal path. Audio stays muted. Creates an isolated app and a new JSONL file; never opens a user instance.";

struct Config {
    source: PathBuf,
    output: PathBuf,
    rate: f64,
    seconds: f64,
    enhancement: Enhancement,
    controls: bool,
}

impl Config {
    fn parse() -> Result<Self, Box<dyn std::error::Error>> {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        if args.len() < 2 || args.len() > 6 {
            return Err(USAGE.into());
        }
        let rate = args
            .get(2)
            .map(|v| v.parse::<f64>())
            .transpose()?
            .unwrap_or(1.0);
        let seconds = args
            .get(3)
            .map(|v| v.parse::<f64>())
            .transpose()?
            .unwrap_or(30.0);
        if !rate.is_finite()
            || !(0.75..=3.0).contains(&rate)
            || !seconds.is_finite()
            || !(1.0..=600.0).contains(&seconds)
        {
            return Err("probe rate must be 0.75–3 and duration 1–600 seconds".into());
        }
        let mut enhancement = Enhancement::default();
        match args.get(4).map(String::as_str).unwrap_or("off") {
            "off" => {}
            "sharpen" => enhancement.sharpen_on = true,
            "anime4k" => enhancement.anime4k_on = true,
            "both" => {
                enhancement.sharpen_on = true;
                enhancement.anime4k_on = true;
            }
            _ => return Err("invalid enhancement mode".into()),
        }
        let controls = match args.get(5).map(String::as_str).unwrap_or("continuous") {
            "continuous" => false,
            "controls" => true,
            _ => return Err("scenario must be continuous or controls".into()),
        };
        Ok(Self {
            source: PathBuf::from(&args[0]),
            output: PathBuf::from(&args[1]),
            rate,
            seconds,
            enhancement,
            controls,
        })
    }
}

struct Probe {
    config: Config,
    source: Arc<Source>,
    failed: AtomicBool,
    finished: AtomicBool,
    controls_complete: AtomicBool,
}
static PROBE: OnceLock<Arc<Probe>> = OnceLock::new();

fn probe() -> Arc<Probe> {
    PROBE
        .get()
        .expect("probe configured before mounting")
        .clone()
}

fn register(components: &nsh_platform_macos::Components) {
    let state = probe();
    hongguo_nsh_video_macos::register_with_observer(components, move |observation| {
        let query_started = host_time_seconds();
        let position = state.source.session.position().ok();
        let query_finished = host_time_seconds();
        state.source.recorder.event(json!({"event":"nativePresentation",
            "sourceId":observation.source_id,"revision":observation.revision,
            "generation":observation.generation,"serial":observation.serial,
            "ptsSeconds":observation.pts_secs,"presentedTimeSeconds":observation.presented_time_secs,
            "callbackTimeSeconds":observation.callback_time_secs,
            "gpuElapsedMs":observation.gpu_elapsed.as_secs_f64()*1000.0,
            "pcmPositionSeconds":position.map(|p|p.position_secs),
            "pcmQueryStartedHostSeconds":query_started,"pcmQueryFinishedHostSeconds":query_finished,
            "buffering":position.map(|p|p.buffering),"holding":state.source.session.holding()}));
    });
}

#[component]
fn PlaybackProbe() -> impl IntoElement {
    let state = probe();
    let mut source = VideoSource::default();
    source.id = TOKEN.into();
    source.url = format!("hongguo-playback://{TOKEN}");
    source.frame_source = Some(format!("hongguo-frames://{TOKEN}"));
    let mut initial = VideoProps::default();
    initial.source = Some(source);
    initial.active = true;
    initial.paused = false;
    initial.muted = true;
    initial.rate = state.config.rate;
    initial.enhancement = state.config.enhancement;
    initial.progress_interval_ms = PROGRESS_MS;
    let props = signal(initial);
    let reference = element_ref();
    let phase = signal(0_u8);
    let seek_target = signal(0_u64);
    let finished = state.clone();
    set_timeout(Duration::from_secs_f64(state.config.seconds), move || {
        props.update(|p| p.paused = true);
        finished.finished.store(true, Ordering::Release);
        finished
            .source
            .recorder
            .event(json!({"event":"durationReached"}));
        nsh_platform_macos::quit_app();
    });
    view().flex_1().child(
        native(COMPONENT_NAME).flex_1().element_ref(&reference)
            .prop(STATE_PROP, move || serde_json::to_value(props.get()).unwrap())
            .on_native_event(move |event: &nsh_core::element::NativeEvent| {
                let event: VideoEvent = serde_json::from_value(event.data.clone()).expect("typed native video event");
                state.source.recorder.event(json!({"event":"videoEvent","value":event}));
                match event {
                    VideoEvent::Error {..} => {
                        state.failed.store(true, Ordering::Release);
                        nsh_platform_macos::quit_app();
                    }
                    VideoEvent::Progress {position_ms,duration_ms,..}
                        if state.config.controls && phase.get_untracked()==0
                            && position_ms as f64 / 1000.0 >= PAUSE_AFTER_MEDIA_SECONDS => {
                        phase.set(1);
                        props.update(|p|p.paused=true);
                        let before = state.source.session.position().unwrap().position_secs;
                        let next = state.clone();
                        let target = duration_ms.map(|duration| duration / 3).unwrap_or(10_000);
                        seek_target.set(target);
                        state.source.recorder.event(json!({"event":"pauseRequested","positionSeconds":before}));
                        set_timeout(PAUSE_DURATION, move || {
                            let after = next.source.session.position().unwrap().position_secs;
                            let passed = (after-before).abs()<=PAUSE_TOLERANCE_SECONDS;
                            next.source.recorder.event(json!({"event":"pauseChecked","beforeSeconds":before,"afterSeconds":after,"passed":passed}));
                            if !passed { next.failed.store(true,Ordering::Release); }
                            let mut seek = Seek::default();
                            seek.source_id=TOKEN.into();seek.request_id="probe-paused-seek".into();seek.position_ms=target;
                            reference.command(SEEK_COMMAND,serde_json::to_value(seek).unwrap());
                        });
                    }
                    VideoEvent::Seeked {request_id,position_ms,..} if request_id=="probe-paused-seek" => {
                        let passed=position_ms.abs_diff(seek_target.get_untracked())<=SEEK_TOLERANCE_MS;
                        state.source.recorder.event(json!({"event":"seekChecked","positionMs":position_ms,"targetMs":seek_target.get_untracked(),"passed":passed}));
                        if !passed {state.failed.store(true,Ordering::Release);}
                        phase.set(2);
                        let next=state.clone();
                        set_timeout(PAUSE_DURATION,move || {
                            props.update(|p|p.paused=false);
                            phase.set(3);
                            next.source.recorder.event(json!({"event":"resumedAfterPausedSeek"}));
                        });
                    }
                    VideoEvent::Progress {position_ms,..} if phase.get_untracked()==3
                        && position_ms>=seek_target.get_untracked().saturating_add(RESUMED_PROGRESS_MS) => {
                        phase.set(4);
                        state.controls_complete.store(true,Ordering::Release);
                        state.source.recorder.event(json!({"event":"controlsComplete","positionMs":position_ms}));
                    }
                    VideoEvent::End {..} => {
                        state.finished.store(true,Ordering::Release);
                        nsh_platform_macos::quit_app();
                    }
                    _ => {}
                }
            })
    )
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let config = Config::parse()?;
    let file = File::open(&config.source)?;
    if !file.metadata()?.is_file() {
        return Err("probe source must be a regular plaintext MP4 file".into());
    }
    let recorder = Arc::new(Recorder::open(&config.output)?);
    let session = Arc::new(PlaybackSession::open(
        Arc::new(FileSource(file)),
        SessionConfig {
            defer_decoder_binding: true,
            ..Default::default()
        },
    )?);
    let source = Arc::new(Source::new(session, recorder.clone()));
    let state = Arc::new(Probe {
        config,
        source: source.clone(),
        failed: AtomicBool::new(false),
        finished: AtomicBool::new(false),
        controls_complete: AtomicBool::new(false),
    });
    PROBE
        .set(state.clone())
        .map_err(|_| "probe already mounted")?;
    let registration = register_frames(TOKEN, source.clone())?;
    recorder.event(json!({"event":"started","rate":state.config.rate,"seconds":state.config.seconds,
        "enhancement":state.config.enhancement,"controls":state.config.controls,"muted":true,
        "measurementBoundary":"native presentedTime and callback/PCM observations; no physical scanout or acoustic latency claim"}));
    nsh_platform_macos::set_components(register);
    let result = nsh_platform_macos::run_app(
        nsh_platform_macos::AppSource::Rust(
            nsh_platform_macos::Root::of::<PlaybackProbe>().app_id(APP_ID),
        ),
        nsh_platform_macos::AppOptions {
            title: "NSH playback probe".into(),
            size: (960, 600),
            app_id: APP_ID.into(),
            data_dir: Some(state.config.output.with_extension("data")),
            ..Default::default()
        },
    );
    let (selected, presented) = source.counts();
    let smoke_completed = selected > 0
        && presented > 0
        && state.finished.load(Ordering::Acquire)
        && !state.failed.load(Ordering::Acquire)
        && recorder.dropped() == 0
        && (!state.config.controls || state.controls_complete.load(Ordering::Acquire));
    recorder.event(json!({"event":"summary","smokeCompleted":smoke_completed,"selectedUniqueFrames":selected,
        "presentedAckUniqueFrames":presented,"logEventsDropped":recorder.dropped(),"appResult":format!("{result:?}"),
        "continuousPlaybackVerified":null,"frameDropRate":null,"physicalAvSyncErrorMs":null,
        "summaryBoundary":"smoke completion requires actual presentation and requested controls; inspect raw time series for continuous playback, freeze and drift"}));
    drop(registration);
    source.session.close();
    recorder.finish()?;
    if !smoke_completed {
        return Err(
            "playback probe did not complete with actual presentation and controls; inspect JSONL"
                .into(),
        );
    }
    Ok(())
}
