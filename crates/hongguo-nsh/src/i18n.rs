use nsh_core::prelude::*;
use std::sync::LazyLock;

static ZH: LazyLock<serde_json::Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../src/locales/zh-CN.json"))
        .expect("validated zh-CN catalog")
});
static EN: LazyLock<serde_json::Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../src/locales/en-US.json"))
        .expect("validated en-US catalog")
});

const LANGUAGE_KEY: &str = "hongguo.locale";
#[derive(Clone, Copy)]
pub(crate) struct Language(Signal<&'static str>);

pub(crate) fn provide_language() -> Language {
    if let Some(language) = use_context::<Language>() {
        return language;
    }
    let saved = nsh_core::storage::Storage::get_item(LANGUAGE_KEY);
    let value = match saved.as_deref() {
        Some("zh-CN") => "zh-CN",
        Some("en-US") => "en-US",
        _ if locale().as_str().starts_with("zh") => "zh-CN",
        _ => "en-US",
    };
    let language = Language(signal(value));
    provide_context(language);
    language
}

impl Language {
    pub(crate) fn value(self) -> &'static str {
        self.0.get()
    }
    pub(crate) fn select(self, value: &'static str) -> Result<(), String> {
        nsh_core::storage::Storage::set_item(LANGUAGE_KEY, value)
            .map_err(|error| error.to_string())?;
        self.0.set(value);
        Ok(())
    }
}

pub(crate) fn is_chinese() -> bool {
    use_context::<Language>().map_or_else(
        || locale().as_str().starts_with("zh"),
        |language| language.value() == "zh-CN",
    )
}

pub(crate) fn format_play_count(value: i64) -> String {
    if is_chinese() {
        if value >= 100_000_000 {
            return tr("number.yi")
                .replace("{value}", &format!("{:.1}", value as f64 / 100_000_000.0));
        }
        if value >= 10_000 {
            return tr("number.wan").replace("{value}", &format!("{:.0}", value as f64 / 10_000.0));
        }
        return value.to_string();
    }
    let (number, key) = if value >= 1_000_000 {
        (value as f64 / 1_000_000.0, "number.million")
    } else if value >= 10_000 {
        (value as f64 / 1_000.0, "number.thousand")
    } else {
        return value.to_string();
    };
    let number = format!("{number:.1}");
    tr(key).replace("{value}", number.strip_suffix(".0").unwrap_or(&number))
}

pub(crate) fn format_count_precise(value: i64) -> String {
    if !is_chinese() {
        return format_play_count(value);
    }
    let (number, key) = if value >= 100_000_000 {
        (value as f64 / 100_000_000.0, "number.yi")
    } else if value >= 10_000 {
        (value as f64 / 10_000.0, "number.wan")
    } else {
        return value.to_string();
    };
    let number = format!("{number:.1}");
    tr(key).replace("{value}", number.strip_suffix(".0").unwrap_or(&number))
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut unit = 0;
    let mut value = bytes as f64;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

pub(crate) fn format_duration(seconds: f64) -> String {
    const SECONDS_PER_MINUTE: u64 = 60;
    const SECONDS_PER_HOUR: u64 = 3600;
    let seconds = if seconds.is_finite() {
        seconds.max(0.0) as u64
    } else {
        0
    };
    let hours = seconds / SECONDS_PER_HOUR;
    let minutes = (seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE;
    let seconds = seconds % SECONDS_PER_MINUTE;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

pub(crate) fn tr(key: &str) -> SharedString {
    let catalog = if is_chinese() { &*ZH } else { &*EN };
    lookup(catalog, key).unwrap_or(key).to_owned().into()
}

fn lookup<'a>(catalog: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    key.split('.')
        .try_fold(catalog, |value, part| value.get(part))?
        .as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_duration_uses_the_same_hour_and_invalid_value_boundaries_as_tauri() {
        for (seconds, expected) in [
            (0.0, "00:00"),
            (42.9, "00:42"),
            (3599.0, "59:59"),
            (3600.0, "1:00:00"),
            (7384.0, "2:03:04"),
            (-1.0, "00:00"),
            (f64::NAN, "00:00"),
            (f64::INFINITY, "00:00"),
        ] {
            assert_eq!(format_duration(seconds), expected);
        }
    }

    #[test]
    fn both_catalogs_have_shell_navigation() {
        for key in [
            "app.name",
            "nav.home.title",
            "nav.browse.title",
            "nav.settings.title",
            "common.retry",
            "common.loading",
        ] {
            assert!(lookup(&ZH, key).is_some(), "zh-CN: {key}");
            assert!(lookup(&EN, key).is_some(), "en-US: {key}");
        }
    }
}
