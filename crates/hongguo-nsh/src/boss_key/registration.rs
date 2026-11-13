use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex, Once},
};

type Sink = Arc<dyn Fn() + Send + Sync>;
static SINKS: LazyLock<Mutex<HashMap<u32, Sink>>> = LazyLock::new(Default::default);
static INITIALIZE: Once = Once::new();

pub(super) struct Registration {
    manager: Result<GlobalHotKeyManager, String>,
    current: Option<HotKey>,
    sink: Sink,
}

impl Registration {
    pub(super) fn new(sink: impl Fn() + Send + Sync + 'static) -> Self {
        INITIALIZE.call_once(|| {
            GlobalHotKeyEvent::set_event_handler(Some(|event: GlobalHotKeyEvent| {
                if event.state == HotKeyState::Pressed {
                    let sink = SINKS.lock().expect("hotkey sinks").get(&event.id).cloned();
                    if let Some(sink) = sink {
                        sink();
                    }
                }
            }))
        });
        Self {
            manager: GlobalHotKeyManager::new().map_err(|error| error.to_string()),
            current: None,
            sink: Arc::new(sink),
        }
    }

    pub(super) fn apply(&mut self, spec: &str) -> Result<(), String> {
        let next = if spec.trim().is_empty() {
            None
        } else {
            Some(spec.parse::<HotKey>().map_err(|error| error.to_string())?)
        };
        if self.current == next {
            return Ok(());
        }
        let manager = self.manager.as_ref().map_err(Clone::clone)?;
        if let Some(next) = next {
            manager.register(next).map_err(|error| error.to_string())?;
        }
        if let Some(previous) = self.current {
            if let Err(error) = manager.unregister(previous) {
                if let Some(next) = next
                    && let Err(rollback) = manager.unregister(next)
                {
                    log::error!("[NSH boss key] candidate rollback failed: {rollback}");
                    return Err(format!("{error}; rollback: {rollback}"));
                }
                return Err(error.to_string());
            }
            SINKS.lock().expect("hotkey sinks").remove(&previous.id());
        }
        self.current = next;
        if let Some(next) = next {
            SINKS
                .lock()
                .expect("hotkey sinks")
                .insert(next.id(), self.sink.clone());
        }
        Ok(())
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(current) = self.current {
            SINKS.lock().expect("hotkey sinks").remove(&current.id());
            if let Ok(manager) = &self.manager
                && let Err(error) = manager.unregister(current)
            {
                log::warn!("[NSH boss key] unregister failed: {error}");
            }
        }
    }
}
