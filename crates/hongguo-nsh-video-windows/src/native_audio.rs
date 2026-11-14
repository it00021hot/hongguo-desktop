use hongguo_native_clock::{Clock, Control, NativeClock, NativePlayer, Position};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use windows::{
    Win32::{
        Media::MediaFoundation::*,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
    },
    core::{BSTR, implement},
};

static STARTED: OnceLock<Result<(), String>> = OnceLock::new();

pub(crate) fn open(url: String) -> Result<Arc<dyn Clock>, String> {
    NativeClock::open(move || Player::open(&url)).map(|clock| clock as Arc<dyn Clock>)
}

struct Com;
impl Com {
    fn initialize() -> Result<Self, String> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(error)?;
        Ok(Self)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

#[implement(IMFMediaEngineNotify)]
struct Notify {
    buffering: Arc<AtomicBool>,
}
impl IMFMediaEngineNotify_Impl for Notify_Impl {
    fn EventNotify(&self, event: u32, _param1: usize, _param2: u32) -> windows::core::Result<()> {
        if event == MF_MEDIA_ENGINE_EVENT_BUFFERINGSTARTED.0 as u32
            || event == MF_MEDIA_ENGINE_EVENT_WAITING.0 as u32
        {
            self.buffering.store(true, Ordering::Release);
        } else if event == MF_MEDIA_ENGINE_EVENT_BUFFERINGENDED.0 as u32
            || event == MF_MEDIA_ENGINE_EVENT_CANPLAY.0 as u32
            || event == MF_MEDIA_ENGINE_EVENT_PLAYING.0 as u32
        {
            self.buffering.store(false, Ordering::Release);
        }
        Ok(())
    }
}

struct Player {
    engine: IMFMediaEngine,
    buffering: Arc<AtomicBool>,
    requested: Control,
    applied: bool,
    pending_seek: Option<f64>,
    _com: Com,
}

impl Player {
    fn open(url: &str) -> Result<Self, String> {
        let com = Com::initialize()?;
        STARTED
            .get_or_init(|| unsafe { MFStartup(MF_VERSION, 0) }.map_err(error))
            .clone()?;
        let buffering = Arc::new(AtomicBool::new(false));
        let notify: IMFMediaEngineNotify = Notify {
            buffering: buffering.clone(),
        }
        .into();
        let engine = unsafe {
            let mut attributes = None;
            MFCreateAttributes(&mut attributes, 1).map_err(error)?;
            let attributes = attributes.ok_or("Media Foundation returned no attribute store")?;
            attributes
                .SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &notify)
                .map_err(error)?;
            let factory: IMFMediaEngineClassFactory =
                CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER)
                    .map_err(error)?;
            let engine = factory
                .CreateInstance(MF_MEDIA_ENGINE_AUDIOONLY.0 as u32, &attributes)
                .map_err(error)?;
            engine.SetSource(&BSTR::from(url)).map_err(error)?;
            engine
        };
        Ok(Self {
            engine,
            buffering,
            requested: Control::default(),
            applied: false,
            pending_seek: None,
            _com: com,
        })
    }

    fn apply(&mut self) -> Result<(), String> {
        unsafe {
            self.engine
                .SetVolume(f64::from(self.requested.volume))
                .map_err(error)?;
            self.engine.SetMuted(self.requested.muted).map_err(error)?;
            self.engine
                .SetDefaultPlaybackRate(self.requested.rate)
                .map_err(error)?;
            if self.engine.GetReadyState() < 1 {
                return Ok(());
            }
            self.engine
                .SetPlaybackRate(self.requested.rate)
                .map_err(error)?;
            if self.requested.playing {
                self.engine.Play().map_err(error)?;
            } else {
                self.engine.Pause().map_err(error)?;
            }
        }
        self.applied = true;
        Ok(())
    }
}

impl NativePlayer for Player {
    fn position(&mut self) -> Result<Position, String> {
        if !self.applied {
            self.apply()?;
        }
        unsafe {
            if let Ok(failure) = self.engine.GetError()
                && failure.GetErrorCode() != 0
            {
                return Err(format!(
                    "native MediaEngine audio error: {}",
                    failure.GetErrorCode()
                ));
            }
            let ready = self.engine.GetReadyState();
            if ready >= 1
                && let Some(target) = self.pending_seek.take()
            {
                self.engine.SetCurrentTime(target).map_err(error)?;
            }
            let seeking = self.pending_seek.is_some() || self.engine.IsSeeking().as_bool();
            Ok(Position {
                seconds: self.engine.GetCurrentTime().max(0.0),
                ready: ready >= 2 && !seeking,
                buffering: ready < 3 || self.buffering.load(Ordering::Acquire),
                seeking,
                ended: self.engine.IsEnded().as_bool(),
            })
        }
    }

    fn control(&mut self, control: Control) -> Result<(), String> {
        self.requested = control;
        self.applied = false;
        self.apply()
    }

    fn seek(&mut self, seconds: f64) -> Result<(), String> {
        self.pending_seek = Some(seconds);
        Ok(())
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        unsafe {
            let _ = self.engine.Shutdown();
        }
    }
}

fn error(error: windows::core::Error) -> String {
    error.to_string()
}
