use nsh_core::{i18n::Translations, platform::Locale, prelude::*};
use std::{collections::HashMap, sync::LazyLock};

static TRANSLATIONS: LazyLock<Translations> = LazyLock::new(|| {
    let mut catalogs = HashMap::new();
    for (language, source) in [
        ("zh-CN", include_str!("../../../assets/locales/zh-CN.json")),
        ("en-US", include_str!("../../../assets/locales/en-US.json")),
    ] {
        let catalog = serde_json::from_str(source).expect("validated translation catalog");
        let mut messages = HashMap::new();
        flatten("", catalog, &mut messages);
        catalogs.insert(language.to_owned(), messages);
    }
    Translations::from_map(catalogs)
});

const LANGUAGE_KEY: &str = "hongguo.locale";

pub(crate) fn install() {
    global::<Translations>().set(TRANSLATIONS.clone());
    let saved = nsh_core::storage::Storage::get_item(LANGUAGE_KEY);
    let language = match saved.as_deref() {
        Some("zh-CN") => "zh-CN",
        Some("en-US") => "en-US",
        _ if locale().as_str().starts_with("zh") => "zh-CN",
        _ => "en-US",
    };
    set_locale(Some(Locale::parse(language).expect("supported locale")));
}

pub(crate) fn language() -> &'static str {
    if is_chinese() { "zh-CN" } else { "en-US" }
}

pub(crate) fn select_language(value: &'static str) -> Result<(), String> {
    if !matches!(value, "zh-CN" | "en-US") {
        return Err(format!("Unsupported application language: {value}"));
    }
    nsh_core::storage::Storage::set_item(LANGUAGE_KEY, value).map_err(|error| error.to_string())?;
    set_locale(Some(Locale::parse(value).expect("supported locale")));
    Ok(())
}

pub(crate) fn is_chinese() -> bool {
    locale().as_str().starts_with("zh")
}

fn flatten(prefix: &str, value: serde_json::Value, messages: &mut HashMap<String, String>) {
    match value {
        serde_json::Value::String(text) => {
            messages.insert(prefix.to_owned(), text);
        }
        serde_json::Value::Object(children) => {
            for (key, value) in children {
                let key = if prefix.is_empty() {
                    key
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&key, value, messages);
            }
        }
        _ => panic!("translation catalog values must be text"),
    }
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
    nsh_core::t!(key).into()
}
