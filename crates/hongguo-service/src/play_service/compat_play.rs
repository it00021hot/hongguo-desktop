//! 播放兼容兜底：解码不出来时转成 H.264 再播（HEVC 能不能解完全看系统装没装「HEVC 视频扩展」）。
//! 转码走 `pipeline::transcode` 的统一分流链（平台硬编 → ffmpeg），产物落兼容缓存、同一集只转一次；
//! 进度按已编码秒数上报到界面。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use crate::app_state::AppState;
use crate::events::ServiceEvent;
use crate::transcode_service::cache;
use hongguo_common::{AppError, AppResult};

/// 兜底转码的结果。
pub struct CompatPlay {
    /// 转码产物的可播放地址（`hongguo-local://`）
    pub url: String,
    /// 命中的已有缓存
    pub cached: bool,
    /// 实际用的转码后端
    pub backend: String,
    /// 转码耗时（毫秒）。提前交出首段时只计到首段落盘，命中缓存时为 0
    pub elapsed_ms: u128,
    /// 地址在整片写完前就可用，后续字节从同一路径继续读
    pub progressive: bool,
}

enum PrefixNote {
    Ready,
    Finished,
    Failed(String),
}

/// 给一集准备兼容格式产物，返回可播放地址。
///
/// `source` 是这一集的**明文 MP4**：已下载的给本地路径，没下载的给整集明文字节。
/// `on_progress` 收到 0–100 的百分比；owned 回调是因为转码段要整体进阻塞池（'static）。
pub async fn ensure_compat(
    series_id: &str,
    vid_index: u32,
    source: CompatSource,
    on_progress: impl Fn(f64) + Send + Sync + 'static,
) -> AppResult<CompatPlay> {
    ensure_inner(series_id, vid_index, source, on_progress, false).await
}

/// 首段可播就返回地址，转码在同一路径上继续。整片结束前缓存不会把半成品当成命中。
pub async fn ensure_compat_progressive(
    series_id: &str,
    vid_index: u32,
    source: CompatSource,
    on_progress: impl Fn(f64) + Send + Sync + 'static,
) -> AppResult<CompatPlay> {
    ensure_inner(series_id, vid_index, source, on_progress, true).await
}

async fn ensure_inner(
    series_id: &str,
    vid_index: u32,
    source: CompatSource,
    on_progress: impl Fn(f64) + Send + Sync + 'static,
    at_prefix: bool,
) -> AppResult<CompatPlay> {
    // 播放档与合并归档分文件（`.play.mp4`），命中/落盘都走 Playback，不能误用归档路径。
    let rate = hongguo_media_platform::RateProfile::Playback;
    if let Some(p) = cache::cached_path_for(series_id, vid_index, rate) {
        on_progress(100.0);
        let url = hongguo_protocol::local::local_play_url(&p.to_string_lossy())
            .ok_or_else(|| AppError::Media("转码产物路径无法转成播放地址".into()))?;
        return Ok(CompatPlay {
            url,
            cached: true,
            backend: backend_label(hongguo_media_platform::capability::selected_backend()),
            elapsed_ms: 0,
            progressive: false,
        });
    }

    let target = cache::cache_file_for(series_id, vid_index, rate);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    let temp = plaintext_temp_path_for(&target);
    let started = std::time::Instant::now();
    let series_owned = series_id.to_string();

    // 写盘 + demux + 编码是分钟级阻塞，占专用阻塞池的线程，不能占 async 工作线程。
    let progress = Arc::new(on_progress);
    let progress_in_task = Arc::clone(&progress);
    let target_for_task = target.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
    let tx_for_task = std::sync::Arc::clone(&tx);
    let handoff = at_prefix;
    let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if at_prefix {
        hongguo_protocol::grow::open(&target).map_err(AppError::Media)?;
        register_cancel(series_id, vid_index, std::sync::Arc::clone(&cancel_flag));
    }
    let cancel_for_task = std::sync::Arc::clone(&cancel_flag);
    let task = crate::runtime::spawn_blocking(move || {
        let (plain, is_temp) = match source {
            CompatSource::LocalFile(p) => (p, false),
            CompatSource::PlainBytes(bytes) => {
                std::fs::write(&temp, &bytes).map_err(|e| AppError::Io(e.to_string()))?;
                (temp.clone(), true)
            }
        };

        // 百分比要拿总时长才折得出来；读不出来报 0——宁可显示不确定，也不给一个编出来的数字。
        let total_secs = video_seconds(&plain);
        let report = move |secs: f64| {
            let pct = match total_secs {
                Some(t) if t > 0.0 => (secs / t * 100.0).clamp(0.0, 100.0),
                _ => 0.0,
            };
            progress_in_task(pct);
        };
        report(0.0);

        // 转码必须走 pipeline 的同一个入口——分流链只有一份，复刻私有分流
        // 会让平台硬编接不进来，兼容播放永远拿不到硬编速度。
        // 播放档：长边封顶 720p + 轻码率——归档档把 1080p 一分钟胀到 ~60MB，软编更是分钟级。
        let scale_to = crate::transcode_service::pipeline::resolution_of(&plain)
            .and_then(crate::transcode_service::pipeline::playback_scale);
        let committed = {
            let path = target_for_task.clone();
            let tx = std::sync::Arc::clone(&tx_for_task);
            move |len: u64| {
                hongguo_protocol::grow::commit(&path, len);
                if let Some(sender) = tx.lock().unwrap_or_else(|p| p.into_inner()).take() {
                    let _ = sender.send(PrefixNote::Ready);
                }
            }
        };
        let stop = {
            let flag = std::sync::Arc::clone(&cancel_for_task);
            move || flag.load(std::sync::atomic::Ordering::SeqCst)
        };
        let hooks = if handoff {
            crate::transcode_service::pipeline::TranscodeHooks {
                on_committed: Some(&committed),
                is_cancelled: Some(&stop),
            }
        } else {
            crate::transcode_service::pipeline::TranscodeHooks::default()
        };
        let result = crate::transcode_service::pipeline::transcode(
            &series_owned,
            vid_index,
            &plain,
            scale_to,
            hongguo_media_platform::RateProfile::Playback,
            Some(&report),
            hooks,
        );
        if handoff {
            match &result {
                Ok(_) => hongguo_protocol::grow::finish(&target_for_task),
                Err(AppError::Cancelled) => hongguo_protocol::grow::cancel(&target_for_task),
                Err(err) => hongguo_protocol::grow::fail(&target_for_task, err.to_string()),
            }
            if let Some(sender) = tx_for_task.lock().unwrap_or_else(|p| p.into_inner()).take() {
                let note = match &result {
                    Ok(_) => PrefixNote::Finished,
                    Err(AppError::Cancelled) => PrefixNote::Failed("已取消".into()),
                    Err(err) => PrefixNote::Failed(err.to_string()),
                };
                let _ = sender.send(note);
            }
        }

        // 源明文只是中间产物，转码成功或失败都要清理，避免残留敏感媒体数据。
        if is_temp {
            let _ = std::fs::remove_file(&plain);
        }
        // 新产物已进缓存：按「最多保留 3 集」的滚动窗口收敛一轮。合并路径不经这里——
        // 中途逐出会让拼接拿到断路径，它消费完就地删除。
        if result.is_ok() {
            cache::trim();
        }
        clear_cancel(&series_owned, vid_index);
        result
    });
    if at_prefix {
        let note = rx
            .await
            .map_err(|_| AppError::Media("转码任务在交出地址前结束".into()))?;
        let elapsed = started.elapsed();
        let url = hongguo_protocol::local::local_play_url(&target.to_string_lossy())
            .ok_or_else(|| AppError::Media("转码产物路径无法转成播放地址".into()))?;
        return match note {
            PrefixNote::Ready => {
                log::info!("[Compat] 第 {vid_index} 集首段可播，后续继续写入同一路径");
                Ok(CompatPlay {
                    url,
                    cached: false,
                    backend: backend_label(hongguo_media_platform::capability::selected_backend()),
                    elapsed_ms: elapsed.as_millis(),
                    progressive: true,
                })
            }
            PrefixNote::Finished => {
                progress(100.0);
                Ok(CompatPlay {
                    url,
                    cached: false,
                    backend: backend_label(hongguo_media_platform::capability::selected_backend()),
                    elapsed_ms: elapsed.as_millis(),
                    progressive: false,
                })
            }
            PrefixNote::Failed(message) => Err(AppError::Media(message)),
        };
    }
    let result = task
        .await
        .map_err(|e| AppError::Media(format!("兼容转码任务异常结束: {e}")))??;

    progress(100.0);
    let elapsed = started.elapsed();
    let url = hongguo_protocol::local::local_play_url(&result.output_path)
        .ok_or_else(|| AppError::Media("转码产物路径无法转成播放地址".into()))?;
    let backend = backend_label(result.backend);
    log::info!("[Compat] 第 {vid_index} 集就绪：{backend}，耗时 {elapsed:?}");
    Ok(CompatPlay {
        url,
        cached: false,
        backend,
        elapsed_ms: elapsed.as_millis(),
        progressive: false,
    })
}

/// 后端标识，给界面提示用。没有可用后端时报「none」，不编一个后端名出来
///（此时产物只可能是此前转好的缓存）。
fn cancel_slots() -> std::sync::MutexGuard<
    'static,
    std::collections::HashMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>,
> {
    static SLOTS: std::sync::OnceLock<
        std::sync::Mutex<
            std::collections::HashMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>,
        >,
    > = std::sync::OnceLock::new();
    SLOTS
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn cancel_key(series_id: &str, vid_index: u32) -> String {
    format!("{series_id}:{vid_index}")
}

fn register_cancel(
    series_id: &str,
    vid_index: u32,
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    cancel_slots().insert(cancel_key(series_id, vid_index), flag);
}

fn clear_cancel(series_id: &str, vid_index: u32) {
    cancel_slots().remove(&cancel_key(series_id, vid_index));
}

/// 停止这一集还在写的兼容转码，并叫醒等在首段之后的读取。
pub fn cancel_compat(series_id: &str, vid_index: u32) {
    if let Some(flag) = cancel_slots()
        .get(&cancel_key(series_id, vid_index))
        .cloned()
    {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    let path = cache::cache_file_for(
        series_id,
        vid_index,
        hongguo_media_platform::RateProfile::Playback,
    );
    hongguo_protocol::grow::cancel(&path);
}

async fn play_compat(
    at_prefix: bool,
    series_id: &str,
    vid_index: u32,
    source: CompatSource,
    on_progress: impl Fn(f64) + Send + Sync + 'static,
) -> AppResult<CompatPlay> {
    if at_prefix {
        ensure_compat_progressive(series_id, vid_index, source, on_progress).await
    } else {
        ensure_compat(series_id, vid_index, source, on_progress).await
    }
}

fn backend_label(backend: Option<hongguo_media::Backend>) -> String {
    match backend {
        Some(b) => b.decoder_label().to_string(),
        None => "none".to_string(),
    }
}

/// 兼容播放的明文输入临时文件，必须与转码流水线的输出临时文件区分。
fn plaintext_temp_path_for(target: &Path) -> PathBuf {
    let stem = target
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("compat");
    target.with_file_name(format!("{stem}.plain.tmp"))
}

/// 视频轨时长（秒）。读不出来返回 `None`。
fn video_seconds(path: &Path) -> Option<f64> {
    let tracks = hongguo_media::demux::demux_file(path).ok()?;
    let v = tracks.video_track()?;
    (v.info.media_timescale > 0)
        .then(|| v.info.media_duration as f64 / v.info.media_timescale as f64)
}

/// 这一集的明文从哪来。
pub enum CompatSource {
    /// 已下载的本地文件
    LocalFile(PathBuf),
    /// 未下载：整集取回 + 解密后的明文字节
    PlainBytes(Vec<u8>),
}

/// 兼容转码进度事件的阶段（前端 `compatProgressSchema` 镜像）。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CompatPhase {
    Downloading,
    Transcoding,
    Ready,
}

/// 兼容转码进度事件载荷（`compat-play-progress`），与前端 `compatProgressSchema` 两端镜像，改字段同一 commit 改另一端。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatProgressEvent {
    /// 缓存键 `{series_id}:{vid_index}`
    pub key: String,
    pub percent: f64,
    pub phase: CompatPhase,
}

/// 兜底转码的结果（前端直接消费）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatPlayResult {
    pub url: String,
    pub cached: bool,
    pub backend: String,
    pub elapsed_ms: u128,
    /// 首段已经可播，整片还在同一路径上继续写
    pub progressive: bool,
}

/// 把一集转成 H.264 供本机解不了的机器播放。
/// 播放器确认「有声音没画面」后调它；已下载的走本地文件，没下载的现取现解；
/// 进度经事件总线（`compat-play-progress`）送达壳。
pub async fn compat_for_playback(
    state: &AppState,
    series_id: &str,
    vid_index: u32,
    vid: Option<String>,
) -> AppResult<CompatPlayResult> {
    compat_for_playback_with(state, series_id, vid_index, vid, false).await
}

/// Tauri 播放链：首段可播即返回。GPUI 仍走 [`compat_for_playback`]，等整片落完。
pub async fn compat_for_playback_prefix(
    state: &AppState,
    series_id: &str,
    vid_index: u32,
    vid: Option<String>,
) -> AppResult<CompatPlayResult> {
    compat_for_playback_with(state, series_id, vid_index, vid, true).await
}

async fn compat_for_playback_with(
    state: &AppState,
    series_id: &str,
    vid_index: u32,
    vid: Option<String>,
    at_prefix: bool,
) -> AppResult<CompatPlayResult> {
    crate::cold_start::wait().await;
    let key = format!("{series_id}:{vid_index}");
    let events = state.events().clone();
    // 进度回调要随转码任务进阻塞池（'static）：emit 自持克隆，各分支再克隆出独立回调
    let emit = {
        let key = key.clone();
        let events = events.clone();
        move |percent: f64, phase: CompatPhase| {
            events.publish(ServiceEvent::CompatProgress(CompatProgressEvent {
                key: key.clone(),
                percent,
                phase,
            }));
        }
    };

    // 优先用已下载的本地文件：不用再走网络，也不用等取流
    let local: Option<PathBuf> = state
        .queue()
        .completed_path(series_id, vid_index)
        .map(PathBuf::from);

    let result = if let Some(p) = local {
        let emit_tc = emit.clone();
        play_compat(
            at_prefix,
            series_id,
            vid_index,
            CompatSource::LocalFile(p),
            move |pct| emit_tc(pct, CompatPhase::Transcoding),
        )
        .await
    } else {
        let Some(vid) = vid else {
            return Err(AppError::InvalidArgs("既没有本地文件，也没有 vid".into()));
        };
        let settings = state.settings();
        let env = state.api_env();
        // 转码源保持全量候选：HEVC 走平台硬编；bytevc2 走内置 Rust 解帧。
        let cap = hongguo_api::stream_pick::DecoderCapability::full()
            .with_bytevc2(hongguo_media_platform::capability::bytevc2_decode_available());
        let play = hongguo_api::play_url::fetch_play_url(&vid, None, cap, &env).await?;
        // 取流占前 20%，转码占后 80%：两段都动，但用户看到的是一条连续的进度
        emit(0.0, CompatPhase::Downloading);
        let plain = crate::play_service::online::fetch_plain(&play, &settings).await?;
        let emit_tc = emit.clone();
        let decoded = play_compat(
            at_prefix,
            series_id,
            vid_index,
            CompatSource::PlainBytes(plain),
            move |pct| emit_tc(20.0 + pct * 0.8, CompatPhase::Transcoding),
        )
        .await;
        if let Err(err) = &decoded
            && hongguo_bytevc2::is_bytevc2(&play.codec)
        {
            log::warn!("[Compat] ByteVC2 内置解码失败，改取不含 bytevc2 的档: {err}");
            let fallback = hongguo_api::play_url::fetch_play_url(
                &vid,
                None,
                hongguo_api::stream_pick::DecoderCapability::full(),
                &env,
            )
            .await?;
            if !hongguo_bytevc2::is_bytevc2(&fallback.codec) {
                let plain = crate::play_service::online::fetch_plain(&fallback, &settings).await?;
                let emit_tc = emit.clone();
                play_compat(
                    at_prefix,
                    series_id,
                    vid_index,
                    CompatSource::PlainBytes(plain),
                    move |pct| emit_tc(20.0 + pct * 0.8, CompatPhase::Transcoding),
                )
                .await
            } else {
                decoded
            }
        } else {
            decoded
        }
    };

    emit(100.0, CompatPhase::Ready);
    let r = result?;
    Ok(CompatPlayResult {
        url: r.url,
        cached: r.cached,
        backend: r.backend,
        elapsed_ms: r.elapsed_ms,
        progressive: r.progressive,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真机冒烟：明文 bvc2 → 内置 Rust 解帧 → 产物须为 H.264。
    #[tokio::test]
    #[ignore = "依赖 /tmp/hg-bytevc2-plain.mp4"]
    async fn live_bytevc2_plain_transcodes_to_h264() {
        let plain = std::path::PathBuf::from("/tmp/hg-bytevc2-plain.mp4");
        assert!(
            plain.is_file(),
            "先跑 hongguo-api::probe_bytevc2_sample_live 落明文样本"
        );
        let demuxed = hongguo_media::demux::demux_file(&plain).expect("明文应能解复用");
        let codec = &demuxed.video_track().expect("应有视频轨").info.codec;
        assert!(
            hongguo_bytevc2::is_bytevc2(codec),
            "明文样本应是 bvc2，实际={codec}"
        );

        let sid = format!("bvc2-smoke-{}", std::process::id());
        let got = ensure_compat(&sid, 1, CompatSource::LocalFile(plain), |_| {})
            .await
            .expect("bytevc2 兼容转码应成功");
        assert!(!got.cached);
        let out = cache::cached_path_for(&sid, 1, hongguo_media_platform::RateProfile::Playback)
            .expect("转码产物应在缓存");
        let out_demux = hongguo_media::demux::demux_file(&out).expect("产物应能解复用");
        let out_codec = &out_demux.video_track().expect("产物应有视频轨").info.codec;
        assert!(
            hongguo_media::h264::is_h264(out_codec),
            "兼容产物须为 h264，实际={out_codec} backend={}",
            got.backend
        );
        let _ = std::fs::remove_file(&out);
        println!(
            "[bvc2-smoke] OK backend={} elapsed_ms={} out={}",
            got.backend,
            got.elapsed_ms,
            out.display()
        );
    }

    #[tokio::test]
    async fn a_cached_compat_file_short_circuits() {
        let dir = std::env::temp_dir().join(format!("hg-compat-hit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);
        let p = cache::cache_file_for("s", 1, hongguo_media_platform::RateProfile::Playback);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, vec![7u8; 200 * 1024]).unwrap();

        let got = ensure_compat("s", 1, CompatSource::PlainBytes(Vec::new()), |_| {})
            .await
            .expect("命中缓存不该失败");
        assert!(got.cached, "应当命中缓存");
        assert_eq!(got.elapsed_ms, 0, "命中缓存不耗时间");
        assert!(
            got.url.starts_with(&format!(
                "{}/f/",
                hongguo_protocol::scheme_base(hongguo_protocol::LOCAL_SCHEME)
            )),
            "实际: {}",
            got.url
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_missing_source_is_an_error_not_an_empty_artifact() {
        let dir = std::env::temp_dir().join(format!("hg-compat-miss-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);
        let missing = dir.join("nope.mp4");
        let r = ensure_compat("s2", 1, CompatSource::LocalFile(missing), |_| {}).await;
        assert!(r.is_err(), "源不存在时必须报错，不能产出空文件");
        assert!(
            !cache::cache_file_for("s2", 1, hongguo_media_platform::RateProfile::Playback).exists(),
            "失败时不应留下产物"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plaintext_source_temp_is_distinct_from_ffmpeg_output_temp() {
        let target = cache::cache_file_for("s", 22, hongguo_media_platform::RateProfile::Playback);
        let source = plaintext_temp_path_for(&target);
        let ffmpeg_output = hongguo_common::utils::path::temp_path_for(&target);
        assert_ne!(source, ffmpeg_output, "ffmpeg 会把自己的临时输出误认成输入");
    }

    #[tokio::test]
    async fn plaintext_source_temp_is_removed_when_transcode_fails() {
        let dir =
            std::env::temp_dir().join(format!("hg-compat-temp-cleanup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);
        let target =
            cache::cache_file_for("cleanup", 1, hongguo_media_platform::RateProfile::Playback);
        let source_temp = plaintext_temp_path_for(&target);

        let result = ensure_compat(
            "cleanup",
            1,
            CompatSource::PlainBytes(vec![1, 2, 3]),
            |_| {},
        )
        .await;

        assert!(result.is_err(), "无效媒体数据应当转码失败");
        assert!(!source_temp.exists(), "失败后不能残留明文临时文件");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
