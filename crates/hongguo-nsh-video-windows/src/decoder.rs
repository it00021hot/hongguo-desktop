//! Application-owned MediaEngine, independent of NSH's private Video and GPUI.
//! Construct/control on the window's COM thread. Native notifications only enqueue;
//! they never re-enter MediaEngine or wait for the UI/render worker.

use crate::VideoError;
use hongguo_nsh_video_api::{ErrorCode, Source, VideoProps};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, SyncSender, TryRecvError, TrySendError},
};
use windows::{
    Win32::{
        Media::MediaFoundation::{
            CLSID_MFMediaEngineClassFactory, IMFAttributes, IMFMediaEngine,
            IMFMediaEngineClassFactory, IMFMediaEngineEx, IMFMediaEngineNotify,
            IMFMediaEngineNotify_Impl, MF_MEDIA_ENGINE_CALLBACK,
            MF_MEDIA_ENGINE_EVENT_BUFFERINGENDED, MF_MEDIA_ENGINE_EVENT_BUFFERINGSTARTED,
            MF_MEDIA_ENGINE_EVENT_CANPLAY, MF_MEDIA_ENGINE_EVENT_ENDED,
            MF_MEDIA_ENGINE_EVENT_ERROR, MF_MEDIA_ENGINE_EVENT_FIRSTFRAMEREADY,
            MF_MEDIA_ENGINE_EVENT_LOADEDMETADATA, MF_MEDIA_ENGINE_EVENT_PLAYING,
            MF_MEDIA_ENGINE_EVENT_SEEKED, MF_MEDIA_ENGINE_EVENT_WAITING, MF_VERSION,
            MFCreateAttributes, MFSTARTUP_FULL, MFStartup,
        },
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
    },
    core::{BSTR, Interface, Result, implement},
};

#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum DecoderEvent {
    Metadata,
    Ready,
    Buffering(bool),
    Ended,
    Seeked,
    Failed { kind: usize, code: u32 },
}

pub struct VideoDecoder {
    engine: IMFMediaEngine,
    extended: IMFMediaEngineEx,
    source: Source,
    events: Receiver<DecoderEvent>,
    overflow: Arc<AtomicBool>,
}

impl VideoDecoder {
    pub fn open(
        source: Source,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> std::result::Result<Self, VideoError> {
        if source.id.is_empty() || source.url.is_empty() {
            return Err(VideoError::new(
                ErrorCode::InvalidProps,
                "video source identity and URL are required",
            ));
        }
        if source.frame_source.is_none() {
            return Err(VideoError::new(
                ErrorCode::InvalidProps,
                "builtin video frame source is required",
            ));
        }
        startup()?;
        let (send, events) = std::sync::mpsc::sync_channel(32);
        let overflow = Arc::new(AtomicBool::new(false));
        let notify: IMFMediaEngineNotify = Notify {
            send,
            overflow: overflow.clone(),
            wake: Arc::new(wake),
        }
        .into();
        // SAFETY: COM has been initialized by the owning native window. All interfaces
        // remain retained for the decoder lifetime and callbacks never re-enter it.
        let (engine, extended) = unsafe {
            let mut attributes: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attributes, 3).map_err(decoder_error)?;
            let attributes = attributes
                .ok_or_else(|| decoder_error("Media Foundation returned no attributes"))?;
            attributes
                .SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &notify)
                .map_err(decoder_error)?;
            let factory: IMFMediaEngineClassFactory =
                CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER)
                    .map_err(decoder_error)?;
            let engine = factory
                .CreateInstance(0, &attributes)
                .map_err(decoder_error)?;
            let configured = (|| {
                let extended = engine.cast().map_err(decoder_error)?;
                engine
                    .SetSource(&BSTR::from(&source.url))
                    .map_err(decoder_error)?;
                engine.Load().map_err(decoder_error)?;
                Ok(extended)
            })();
            let extended = match configured {
                Ok(extended) => extended,
                Err(error) => {
                    let _ = engine.Shutdown();
                    return Err(error);
                }
            };
            (engine, extended)
        };
        Ok(Self {
            engine,
            extended,
            source,
            events,
            overflow,
        })
    }

    pub fn source(&self) -> &Source {
        &self.source
    }

    pub fn next_event(&self) -> std::result::Result<Option<DecoderEvent>, VideoError> {
        if self.overflow.swap(false, Ordering::AcqRel) {
            return Err(decoder_error("native video event queue overflowed"));
        }
        match self.events.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                Err(decoder_error("native video event channel disconnected"))
            }
        }
    }

    pub fn apply(&self, props: &VideoProps) -> std::result::Result<(), VideoError> {
        if !props.rate.is_finite()
            || props.rate <= 0.0
            || !props.volume.is_finite()
            || !(0.0..=1.0).contains(&props.volume)
        {
            return Err(VideoError::new(
                ErrorCode::InvalidProps,
                "invalid native playback rate or volume",
            ));
        }
        // SAFETY: Playback controls on the retained engine, on the owning window thread.
        unsafe {
            if !self.extended.IsPlaybackRateSupported(props.rate).as_bool() {
                return Err(decoder_error(
                    "MediaEngine does not support the requested playback rate",
                ));
            }
            self.engine
                .SetVolume(f64::from(props.volume))
                .map_err(decoder_error)?;
            self.engine.SetMuted(props.muted).map_err(decoder_error)?;
            self.engine
                .SetPlaybackRate(props.rate)
                .map_err(decoder_error)?;
            if props.paused || !props.active {
                self.engine.Pause().map_err(decoder_error)
            } else {
                self.engine.Play().map_err(decoder_error)
            }
        }
    }

    pub fn seek(&self, milliseconds: u64) -> std::result::Result<(), VideoError> {
        // SAFETY: Native SEEKED is separately consumed before reporting the actual GetCurrentTime.
        unsafe { self.engine.SetCurrentTime(milliseconds as f64 / 1_000.0) }
            .map_err(|error| VideoError::new(ErrorCode::SeekFailed, error.to_string()))
    }

    pub fn pause(&self) -> std::result::Result<(), VideoError> {
        // SAFETY: Pause is valid while metadata is loading; no rate capability query is needed.
        unsafe { self.engine.Pause() }.map_err(decoder_error)
    }

    pub fn position_ms(&self) -> Option<u64> {
        // SAFETY: Read-only query on this live MediaEngine.
        milliseconds(unsafe { self.engine.GetCurrentTime() })
    }

    pub fn seeking(&self) -> bool {
        // SAFETY: Read-only state of the window-thread-owned audio engine.
        unsafe { self.engine.IsSeeking().as_bool() }
    }

    pub fn duration_ms(&self) -> Option<u64> {
        // SAFETY: Read-only query on this live MediaEngine.
        milliseconds(unsafe { self.engine.GetDuration() })
    }
}

impl Drop for VideoDecoder {
    fn drop(&mut self) {
        // SAFETY: Shutdown retires this source's workers and callback references. It is never reused after Drop.
        unsafe {
            let _ = self.engine.Pause();
            let _ = self.engine.Shutdown();
        }
    }
}

fn milliseconds(seconds: f64) -> Option<u64> {
    let value = seconds * 1_000.0;
    (value.is_finite() && value >= 0.0 && value < u64::MAX as f64).then_some(value as u64)
}

fn decoder_error(error: impl std::fmt::Display) -> VideoError {
    VideoError::new(ErrorCode::DecoderFailed, error.to_string())
}

fn startup() -> std::result::Result<(), VideoError> {
    static STARTED: OnceLock<std::result::Result<(), VideoError>> = OnceLock::new();
    STARTED
        .get_or_init(|| {
            // SAFETY: Process-wide Media Foundation initialization; the native app owns COM.
            unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }
                .map_err(|error| VideoError::new(ErrorCode::DecoderUnavailable, error.to_string()))
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::milliseconds;

    #[test]
    fn native_timeline_rejects_non_numeric_negative_and_overflow_positions() {
        assert_eq!(milliseconds(f64::NAN), None);
        assert_eq!(milliseconds(f64::INFINITY), None);
        assert_eq!(milliseconds(-0.001), None);
        assert_eq!(milliseconds(f64::MAX), None);
        assert_eq!(milliseconds(0.0), Some(0));
        assert_eq!(milliseconds(1.125), Some(1_125));
    }
}

#[implement(IMFMediaEngineNotify)]
struct Notify {
    send: SyncSender<DecoderEvent>,
    overflow: Arc<AtomicBool>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl IMFMediaEngineNotify_Impl for Notify_Impl {
    fn EventNotify(&self, event: u32, param1: usize, param2: u32) -> Result<()> {
        let event = match event {
            id if id == MF_MEDIA_ENGINE_EVENT_LOADEDMETADATA.0 as u32 => DecoderEvent::Metadata,
            id if id == MF_MEDIA_ENGINE_EVENT_CANPLAY.0 as u32
                || id == MF_MEDIA_ENGINE_EVENT_FIRSTFRAMEREADY.0 as u32 =>
            {
                DecoderEvent::Ready
            }
            id if id == MF_MEDIA_ENGINE_EVENT_BUFFERINGSTARTED.0 as u32
                || id == MF_MEDIA_ENGINE_EVENT_WAITING.0 as u32 =>
            {
                DecoderEvent::Buffering(true)
            }
            id if id == MF_MEDIA_ENGINE_EVENT_BUFFERINGENDED.0 as u32
                || id == MF_MEDIA_ENGINE_EVENT_PLAYING.0 as u32 =>
            {
                DecoderEvent::Buffering(false)
            }
            id if id == MF_MEDIA_ENGINE_EVENT_ENDED.0 as u32 => DecoderEvent::Ended,
            id if id == MF_MEDIA_ENGINE_EVENT_SEEKED.0 as u32 => DecoderEvent::Seeked,
            id if id == MF_MEDIA_ENGINE_EVENT_ERROR.0 as u32 => DecoderEvent::Failed {
                kind: param1,
                code: param2,
            },
            _ => return Ok(()),
        };
        match self.send.try_send(event) {
            Ok(()) => (self.wake)(),
            Err(TrySendError::Full(_)) => {
                self.overflow.store(true, Ordering::Release);
                (self.wake)();
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
        Ok(())
    }
}
