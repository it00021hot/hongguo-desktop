use crate::i18n::tr;
use nsh_core::{
    keymap::{Key, Keystroke, Modifiers, When},
    platform::Os,
    prelude::*,
};
use std::{collections::HashSet, rc::Rc};

const STORAGE_KEY: &str = "hongguo.nsh.shortcuts";
const ACTION_COUNT: usize = 5;
const FIRST_VIRTUAL_KEY: u16 = 0x08;
const LAST_VIRTUAL_KEY: u16 = 0xE2;
const MODIFIER_COMBINATIONS: u8 = 16;
type BossReceiver = Rc<dyn Fn(String)>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Action {
    PlayPause,
    SeekBack,
    SeekForward,
    StepPrev,
    StepNext,
}

impl Action {
    pub(crate) const ALL: [Self; ACTION_COUNT] = [
        Self::PlayPause,
        Self::SeekBack,
        Self::SeekForward,
        Self::StepPrev,
        Self::StepNext,
    ];
    pub(crate) fn label_key(self) -> &'static str {
        match self {
            Self::PlayPause => "settings.keyPlayPause",
            Self::SeekBack => "settings.keySeekBack",
            Self::SeekForward => "settings.keySeekForward",
            Self::StepPrev => "settings.keyStepPrev",
            Self::StepNext => "settings.keyStepNext",
        }
    }
    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|action| *action == self)
            .expect("shortcut action")
    }
}

#[derive(Clone, PartialEq, Debug)]
struct Keys([String; ACTION_COUNT]);

impl Default for Keys {
    fn default() -> Self {
        Self(["space", "left", "right", "up", "down"].map(str::to_owned))
    }
}

impl Keys {
    fn decode(json: &str, platform: Os) -> Result<Self, String> {
        let values = serde_json::from_str::<[String; ACTION_COUNT]>(json)
            .map_err(|error| error.to_string())?;
        let mut parsed = HashSet::new();
        let mut normalized = Vec::with_capacity(ACTION_COUNT);
        for value in values {
            let stroke = parse(&value, platform)?;
            if !parsed.insert(stroke) {
                return Err(format!("duplicate shortcut: {stroke}"));
            }
            normalized.push(stroke.to_string());
        }
        Ok(Self(
            normalized.try_into().expect("five validated shortcuts"),
        ))
    }

    fn assign(&self, action: Action, stroke: Keystroke, platform: Os) -> Result<Self, String> {
        let stroke = parse(&stroke.to_string(), platform)?;
        for other in Action::ALL {
            if other != action
                && Keystroke::parse(&self.0[other.index()], platform).expect("validated shortcut")
                    == stroke
            {
                return Err(format!("duplicate shortcut: {stroke}"));
            }
        }
        let mut next = self.clone();
        next.0[action.index()] = stroke.to_string();
        Ok(next)
    }
}

fn parse(value: &str, platform: Os) -> Result<Keystroke, String> {
    let stroke = Keystroke::parse(value, platform).map_err(|error| error.to_string())?;
    if stroke.key == Key::Escape {
        return Err("Escape is reserved for dismissing the current panel".into());
    }
    if let Some(reason) = stroke.reserved(platform) {
        return Err(reason.to_owned());
    }
    Ok(stroke)
}

#[derive(Clone, Copy, PartialEq)]
enum CaptureTarget {
    Player(Action),
    Boss,
}

#[derive(Clone, Copy)]
pub(crate) struct Shortcuts {
    keys: Signal<Keys>,
    capture: Signal<Option<CaptureTarget>>,
    boss_sink: Stored<Option<BossReceiver>>,
    notice: Signal<SharedString>,
    persistent: bool,
}

impl Shortcuts {
    pub(crate) fn current() -> Option<Self> {
        use_context::<Self>()
    }
    pub(crate) fn provide() -> Self {
        Self::install(true)
    }
    pub(crate) fn provide_preview() -> Self {
        Self::install(false)
    }

    fn install(persistent: bool) -> Self {
        if let Some(current) = Self::current() {
            return current;
        }
        let loaded = if persistent {
            nsh_core::storage::Storage::get_item(STORAGE_KEY)
                .map(|json| Keys::decode(&json, os()))
                .transpose()
        } else {
            Ok(None)
        };
        let (keys, notice) = match loaded {
            Ok(keys) => (keys.unwrap_or_default(), SharedString::default()),
            Err(error) => {
                log::error!("[NSH shortcuts] invalid stored bindings: {error}");
                (Keys::default(), tr("nsh.shortcutLoadFailed"))
            }
        };
        let state = Self {
            keys: signal(keys),
            capture: signal(None),
            boss_sink: stored(None),
            notice: signal(notice),
            persistent,
        };
        provide_context(state);
        state.capture_keys();
        state
    }

    pub(crate) fn key(self, action: Action) -> String {
        self.keys.with(|keys| keys.0[action.index()].clone())
    }
    pub(crate) fn label(self, action: Action) -> SharedString {
        let key = self.key(action);
        if key == "space" {
            tr("settings.keySpace")
        } else {
            key_label(&key).into()
        }
    }
    pub(crate) fn capturing(self) -> Option<Action> {
        match self.capture.get() {
            Some(CaptureTarget::Player(action)) => Some(action),
            _ => None,
        }
    }
    pub(crate) fn capturing_boss(self) -> bool {
        matches!(self.capture.get(), Some(CaptureTarget::Boss))
    }
    pub(crate) fn is_capturing(self) -> bool {
        self.capture.get().is_some()
    }
    pub(crate) fn capture(self, action: Action) {
        self.cancel_capture();
        self.notice.set(SharedString::default());
        self.capture.set(Some(CaptureTarget::Player(action)));
    }
    pub(crate) fn capture_boss(self, receive: impl Fn(String) + 'static) {
        self.cancel_capture();
        self.boss_sink.with(|sink| *sink = Some(Rc::new(receive)));
        self.capture.set(Some(CaptureTarget::Boss));
    }
    pub(crate) fn cancel_capture(self) {
        self.capture.set(None);
        self.boss_sink.with(|sink| *sink = None);
    }
    pub(crate) fn conflicts(self, boss: &str) -> Vec<Action> {
        let Ok(boss) = boss_keystroke(boss, os()) else {
            return Vec::new();
        };
        Action::ALL
            .into_iter()
            .filter(|action| {
                Keystroke::parse(&self.key(*action), os()).is_ok_and(|key| key.key == boss.key)
            })
            .collect()
    }

    pub(crate) fn receive_registered(self, spec: &str) -> bool {
        if !self.is_capturing() {
            return false;
        }
        match boss_keystroke(spec, os()) {
            Ok(stroke) => self.captured(stroke),
            Err(error) => {
                self.cancel_capture();
                self.notice.set(error.to_string().into());
            }
        }
        true
    }
    pub(crate) fn notice(self) -> SharedString {
        self.notice.get()
    }
    pub(crate) fn reset(self) {
        self.cancel_capture();
        self.save(Keys::default());
    }

    fn save(self, keys: Keys) {
        let persisted = if self.persistent {
            serde_json::to_string(&keys.0)
                .map_err(|error| error.to_string())
                .and_then(|json| {
                    nsh_core::storage::Storage::set_item(STORAGE_KEY, json)
                        .map_err(|error| error.to_string())
                })
        } else {
            Ok(())
        };
        match persisted {
            Ok(()) => {
                self.keys.set(keys);
                self.notice.set(SharedString::default());
            }
            Err(error) => self.notice.set(
                tr("nsh.shortcutSaveFailed")
                    .replace("{error}", &error)
                    .into(),
            ),
        }
    }

    fn captured(self, stroke: Keystroke) {
        let Some(target) = self.capture.get_untracked() else {
            return;
        };
        if stroke.key == Key::Escape {
            self.cancel_capture();
            return;
        }
        if matches!(target, CaptureTarget::Boss) {
            let Some(spec) = boss_spec(stroke) else {
                return;
            };
            let sink = self.boss_sink.with(|sink| sink.clone());
            self.cancel_capture();
            if let Some(sink) = sink {
                sink(spec);
            }
            return;
        }
        let CaptureTarget::Player(action) = target else {
            unreachable!()
        };
        self.cancel_capture();
        match self
            .keys
            .with_untracked(|keys| keys.assign(action, stroke, os()))
        {
            Ok(keys) => self.save(keys),
            Err(_) => self.notice.set(
                tr("settings.keyConflict")
                    .replace("{key}", &stroke.label(os(), apple_keys()))
                    .into(),
            ),
        }
    }

    fn capture_keys(self) {
        effect(move || {
            if !self.is_capturing() {
                return;
            }
            let mut keys = HashSet::new();
            for virtual_key in FIRST_VIRTUAL_KEY..=LAST_VIRTUAL_KEY {
                let Some(key) = Key::from_virtual_key(virtual_key) else {
                    continue;
                };
                if !keys.insert(key) {
                    continue;
                }
                for flags in 0..MODIFIER_COMBINATIONS {
                    let stroke = Keystroke {
                        key,
                        modifiers: Modifiers {
                            ctrl: flags & 1 != 0,
                            alt: flags & 2 != 0,
                            shift: flags & 4 != 0,
                            cmd: flags & 8 != 0,
                        },
                    };
                    if stroke.reserved(os()).is_some() {
                        continue;
                    }
                    on_key_with(
                        &stroke.to_string(),
                        KeyOptions {
                            when: When::Always,
                            repeat: false,
                            ..Default::default()
                        },
                        move |_| self.captured(stroke),
                    );
                }
            }
        });
    }
}

fn boss_keystroke(spec: &str, platform: Os) -> Result<Keystroke, String> {
    let canonical = spec
        .split('+')
        .map(str::trim)
        .map(|part| {
            if part.eq_ignore_ascii_case("CommandOrControl") {
                "mod"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join("+");
    Keystroke::parse(&canonical, platform).map_err(|error| error.to_string())
}

fn boss_spec(stroke: Keystroke) -> Option<String> {
    let mods = stroke.modifiers;
    if stroke.key == Key::Backspace && mods == Modifiers::default() {
        return Some(String::new());
    }
    if !(mods.ctrl || mods.cmd || mods.alt) {
        return None;
    }
    let mut parts = Vec::new();
    if mods.ctrl || mods.cmd {
        parts.push("CommandOrControl".to_owned());
    }
    if mods.alt {
        parts.push("Alt".to_owned());
    }
    if mods.shift {
        parts.push("Shift".to_owned());
    }
    parts.push(
        Keystroke {
            key: stroke.key,
            modifiers: Modifiers::default(),
        }
        .to_string()
        .to_ascii_uppercase(),
    );
    Some(parts.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boss_capture_clears_only_with_unmodified_backspace() {
        let stroke = |value| Keystroke::parse(value, Os::MacOs).unwrap();
        assert_eq!(boss_spec(stroke("backspace")), Some(String::new()));
        assert_eq!(boss_spec(stroke("shift+backspace")), None);
        assert_eq!(boss_spec(stroke("j")), None);
        assert_eq!(
            boss_spec(stroke("mod+backspace")),
            Some("CommandOrControl+BACKSPACE".into())
        );
    }
    #[test]
    fn assigning_a_new_key_changes_only_its_action_and_duplicate_aliases_are_rejected() {
        let defaults = Keys::default();
        let changed = defaults
            .assign(Action::PlayPause, parse("j", Os::MacOs).unwrap(), Os::MacOs)
            .unwrap();
        assert_eq!(changed.0, ["j", "left", "right", "up", "down"]);
        assert_eq!(defaults.0, ["space", "left", "right", "up", "down"]);
        assert!(
            defaults
                .assign(
                    Action::PlayPause,
                    parse("LEFT", Os::MacOs).unwrap(),
                    Os::MacOs
                )
                .is_err()
        );
    }
    #[test]
    fn stored_keys_require_the_complete_unique_and_bindable_set() {
        assert!(Keys::decode("[]", Os::MacOs).is_err());
        assert!(Keys::decode("broken", Os::MacOs).is_err());
        assert!(Keys::decode(r#"["space","left","LEFT","up","down"]"#, Os::MacOs).is_err());
        assert!(Keys::decode(r#"["cmd+q","left","right","up","down"]"#, Os::MacOs).is_err());
        assert!(Keys::decode(r#"["escape","left","right","up","down"]"#, Os::MacOs).is_err());
        assert_eq!(
            Keys::decode(r#"["space","left","right","up","down"]"#, Os::MacOs).unwrap(),
            Keys::default()
        );
    }
}
