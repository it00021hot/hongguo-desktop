//! Developer-only local-file candidate audit. No customer setting or policy switch.
mod bridge;
mod copy_timer;
mod measurements;
mod source;
pub(crate) mod stats;

use hongguo_nsh_video_api::frames::{FrameSource, register_frames};
use hongguo_nsh_video_api::{
    COMPONENT_NAME, STATE_PROP, Source as VideoSource, VideoEvent, VideoProps,
};
use hongguo_playback::IndexedSource;
use nsh_core::element::desktop::quit;
use nsh_core::prelude::*;
use serde_json::{Value, json};
use std::{
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

static PROPS: OnceLock<VideoProps> = OnceLock::new();
static RUN_STATS: OnceLock<Arc<stats::Stats>> = OnceLock::new();
static TIMEOUT: OnceLock<Duration> = OnceLock::new();

#[component]
fn AuditCanvas() -> impl IntoElement {
    let started = Instant::now();
    set_interval(Duration::from_secs(1), move || {
        if started.elapsed() > *TIMEOUT.get().expect("audit timeout configured") {
            RUN_STATS
                .get()
                .expect("audit stats")
                .fail("audit exceeded its end-to-end timeout".into());
            quit();
        }
    });
    view().flex_1().child(
        native(COMPONENT_NAME)
            .flex_1()
            .prop(STATE_PROP, || {
                serde_json::to_value(PROPS.get().expect("audit props"))
                    .expect("typed video properties")
            })
            .on_native_event(move |event: &nsh_core::element::NativeEvent| {
                match serde_json::from_value::<VideoEvent>(event.data.clone()) {
                    Ok(VideoEvent::Error { code, detail, .. }) => {
                        RUN_STATS
                            .get()
                            .expect("audit stats")
                            .fail(format!("{code:?}: {detail}"));
                        quit();
                    }
                    Ok(VideoEvent::End { .. }) => {
                        spawn(async {
                            timer(Duration::from_millis(250)).await;
                            quit();
                        });
                    }
                    Ok(VideoEvent::EnhancementStatus {
                        phase: hongguo_nsh_video_api::EnhancementPhase::Error,
                        code,
                        detail,
                        ..
                    }) if PROPS.get().is_some_and(|props| {
                        props.enhancement.anime4k_on || props.enhancement.sharpen_on
                    }) =>
                    {
                        RUN_STATS
                            .get()
                            .expect("audit stats")
                            .fail(format!("requested enhancement failed: {code:?}: {detail}"));
                        quit();
                    }
                    _ => {}
                }
            }),
    )
}

pub fn run_cli() -> Result<(), String> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() < 2 || args.len() > 5 {
        return Err("usage: decoder_audit INPUT.mp4 REPORT.json [d3d11|d3d12|both] [MAX_FRAMES=900] [off|sharpen|anime4k|both]".into());
    }
    let input = PathBuf::from(&args[0]);
    let report = PathBuf::from(&args[1]);
    let backend = args.get(2).and_then(|v| v.to_str()).unwrap_or("both");
    let limit = args
        .get(3)
        .map(|v| {
            v.to_str()
                .ok_or("invalid frame limit")?
                .parse::<u64>()
                .map_err(|_| "invalid frame limit")
        })
        .transpose()?
        .unwrap_or(900);
    if !(60..=10000).contains(&limit) {
        return Err("MAX_FRAMES must be 60..10000 (30 warm-up frames are excluded)".into());
    }
    let enhance = args.get(4).and_then(|v| v.to_str()).unwrap_or("off");
    if !matches!(enhance, "off" | "sharpen" | "anime4k" | "both") {
        return Err("unknown enhancement scenario".into());
    }
    if backend == "both" {
        return paired(&input, &report, limit, enhance);
    }
    if !matches!(backend, "d3d11" | "d3d12") {
        return Err("backend must be d3d11, d3d12 or both".into());
    }
    let start = Instant::now();
    let metadata = std::fs::metadata(&input).map_err(error)?;
    if !metadata.is_file() || metadata.len() > 512 * 1024 * 1024 {
        return Err("audit input must be a local MP4 fixture no larger than 512 MiB".into());
    }
    let data: Arc<[u8]> = std::fs::read(&input).map_err(error)?.into();
    let index = IndexedSource::open(data.len() as u64, |offset, length, out| {
        let end = offset
            .checked_add(length)
            .ok_or_else(|| hongguo_common::AppError::Media("input range overflow".into()))?;
        let bytes = data
            .get(offset as usize..end as usize)
            .ok_or_else(|| hongguo_common::AppError::Media("input range outside fixture".into()))?;
        out.clear();
        out.extend_from_slice(bytes);
        Ok(())
    })
    .map_err(error)?;
    let timeout = Duration::from_secs_f64((index.duration_secs * 2.0 + 30.0).max(45.0));
    let configuration = json!({"codec":format!("{:?}",index.video.codec),"timescale":index.video.info.media_timescale,"coded_width":index.video.info.width,"coded_height":index.video.info.height,"container_rotation":index.video.rotation_degrees,"input_bytes":data.len(),"fixture_fingerprint":fingerprint(&data),"extradata_profile_level":profile(&index.video.extradata,index.video.codec),"input_duration_secs":index.duration_secs});
    let stats = stats::Stats::new(start);
    stats.require_enhancement(enhance != "off");
    let source = source::Source::new(data, index, backend.to_owned(), limit, stats.clone());
    let id = format!("decoder-audit-{backend}");
    stats::register(&id, &stats);
    let frames: Arc<dyn FrameSource> = Arc::new(source::Consumer(source.clone()));
    let registration = register_frames(&id, frames)?;
    let mut video = VideoSource::default();
    video.id = id;
    video.url = registration.url().into();
    video.frame_source = Some(registration.url().into());
    let mut props = VideoProps::default();
    props.source = Some(video);
    props.active = true;
    props.paused = false;
    props.muted = true;
    props.progress_interval_ms = 16;
    props.enhancement.sharpen_on = matches!(enhance, "sharpen" | "both");
    props.enhancement.anime4k_on = matches!(enhance, "anime4k" | "both");
    PROPS
        .set(props)
        .map_err(|_| "one candidate per process is required")?;
    RUN_STATS
        .set(stats.clone())
        .map_err(|_| "one candidate per process is required")?;
    TIMEOUT
        .set(timeout)
        .map_err(|_| "one candidate per process is required")?;
    nsh_platform_windows::set_components(crate::register);
    let _exit = nsh_platform_windows::run_app(
        nsh_platform_windows::AppSource::Rust(
            nsh_platform_windows::Root::of::<AuditCanvas>().app_id("com.hongguo.decoder-audit"),
        ),
        nsh_platform_windows::AppOptions {
            title: format!("Decoder audit: {backend} / {enhance}"),
            app_id: "com.hongguo.decoder-audit".into(),
            size: (960, 640),
            output_color_mode: nsh_platform_windows::OutputColorMode::Auto,
            ..Default::default()
        },
    );
    drop(registration);
    source.finish();
    let metrics = stats.report();
    let result = json!({
        "schema":1,"candidate":backend,"enhancement":enhance,"fixture":configuration,
        "native_sources":serde_json::from_str::<Value>(include_str!("../../../scripts/native-media.sources.json")).map_err(error)?,
        "clock":"fixed 1x monotonic video-only clock, gated on the first GPU-drawn frame; no audio output",
        "measurement_boundary":"actual decode + GPU copies + shared color/enhancement + same NSH canvas; Present/Present1 submission is distinct from GPU completion and physical scanout",
        "unmeasured":"no photon latency or physical power claim; DXGI SyncQPCTime is a vblank reference, not a per-frame photon timestamp",
        "metrics":metrics
    });
    write_report(&report, &result)?;
    if !result["metrics"]["error"].is_null() {
        return Err("candidate failed; inspect the written audit report".into());
    }
    Ok(())
}

fn paired(input: &Path, report: &Path, limit: u64, enhance: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(error)?;
    let mut runs = Vec::new();
    let mut failed = false;
    // ABBA counterbalances warm OS/driver caches. Each child owns a fresh NSH
    // process; no reinitialization of the backend or decoder inside one trial.
    for (ordinal, candidate) in ["d3d11", "d3d12", "d3d12", "d3d11"].into_iter().enumerate() {
        let path = report.with_extension(format!("{ordinal}.{candidate}.json"));
        let status = std::process::Command::new(&exe)
            .arg(input)
            .arg(&path)
            .arg(candidate)
            .arg(limit.to_string())
            .arg(enhance)
            .status()
            .map_err(error)?;
        failed |= !status.success();
        let value = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .unwrap_or_else(|| json!({"candidate":candidate,"error":"child produced no report"}));
        runs.push(value);
    }
    write_report(
        report,
        &json!({"schema":1,"order":"ABBA","production_default":"d3d11","no_winner_claim":true,"runs":runs}),
    )?;
    if failed {
        Err("one or more A/B trials failed; reports retain each candidate's actual failure".into())
    } else {
        Ok(())
    }
}
fn write_report(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(error)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(value).map_err(error)?).map_err(error)
}
fn fingerprint(data: &[u8]) -> String {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hash);
    format!("same-binary-noncryptographic-{:016x}", hash.finish())
}
fn profile(extra: &[u8], codec: hongguo_playback::TrackCodec) -> Value {
    match codec {
        hongguo_playback::TrackCodec::Hevc if extra.len() >= 13 => {
            json!({"profile_idc":extra[1]&31,"level_idc":extra[12]})
        }
        hongguo_playback::TrackCodec::H264 if extra.len() >= 4 => {
            json!({"profile_idc":extra[1],"level_idc":extra[3]})
        }
        _ => Value::Null,
    }
}
fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
