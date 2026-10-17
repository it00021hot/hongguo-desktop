//! GPUI 壳文案资源：直接复用 Tauri 壳的 `src/locales/*.json`。
//!
//! 两壳共用同一份词条，避免同一句文案在两个资源文件里漂移。GPUI 是
//! bin-only crate，资源在编译期嵌入；运行时只解析一次并扁平化成
//! `a.b.c -> value`，切换语言只改进程内的 `Locale`。

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

const ZH_CN: &str = include_str!("../../../src/locales/zh-CN.json");
const EN_US: &str = include_str!("../../../src/locales/en-US.json");
const UNSET: u8 = u8::MAX;

static CURRENT: AtomicU8 = AtomicU8::new(UNSET);

/// GPUI 壳支持的语言（与 Tauri `src/locales/index.ts` 的资源键一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Locale {
    ZhCn,
    EnUs,
}

impl Locale {
    pub(crate) const ALL: [Self; 2] = [Self::ZhCn, Self::EnUs];

    /// 持久化值 / i18n 资源键。
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::EnUs => "en-US",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "zh-CN" | "zh" => Some(Self::ZhCn),
            "en-US" | "en" => Some(Self::EnUs),
            _ => None,
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::ZhCn => 0,
            Self::EnUs => 1,
        }
    }

    fn from_code(code: u8) -> Self {
        match code {
            1 => Self::EnUs,
            _ => Self::ZhCn,
        }
    }

    /// 菜单项词条（与 Tauri ThemeSwitch 的语言菜单同一组 key）。
    pub(crate) fn label_key(self) -> &'static str {
        match self {
            Self::ZhCn => "search.langZh",
            Self::EnUs => "search.langEn",
        }
    }

    /// 首次启动的默认语言：显式环境变量优先，其次系统 UI 语言。
    fn detect() -> Self {
        if let Ok(value) = std::env::var("HONGGUO_LOCALE")
            && let Some(locale) = Self::parse(&value)
        {
            return locale;
        }
        #[cfg(target_os = "windows")]
        {
            // SAFETY：无参数、无副作用，返回当前用户 UI 语言的 LANGID。
            let langid = unsafe { GetUserDefaultUILanguage() };
            if langid & 0x03ff == LANG_ENGLISH {
                return Self::EnUs;
            }
        }
        #[cfg(not(target_os = "windows"))]
        if let Ok(value) = std::env::var("LANG").or_else(|_| std::env::var("LC_ALL"))
            && value.to_ascii_lowercase().starts_with("en")
        {
            return Self::EnUs;
        }
        Self::ZhCn
    }
}

#[cfg(target_os = "windows")]
const LANG_ENGLISH: u16 = 0x09;

#[cfg(target_os = "windows")]
unsafe extern "system" {
    fn GetUserDefaultUILanguage() -> u16;
}

fn flatten(prefix: &str, value: &serde_json::Value, out: &mut HashMap<String, String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let key = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&key, value, out);
            }
        }
        serde_json::Value::String(text) => {
            out.insert(prefix.to_owned(), text.clone());
        }
        _ => {}
    }
}

fn load(source: &str, label: &str) -> HashMap<String, String> {
    let root: serde_json::Value = serde_json::from_str(source)
        .unwrap_or_else(|e| panic!("内嵌语言资源 {label} 不是合法 JSON：{e}"));
    let mut out = HashMap::new();
    flatten("", &root, &mut out);
    out
}

pub(crate) fn catalog(locale: Locale) -> &'static HashMap<String, String> {
    static ZH: OnceLock<HashMap<String, String>> = OnceLock::new();
    static EN: OnceLock<HashMap<String, String>> = OnceLock::new();
    match locale {
        Locale::ZhCn => ZH.get_or_init(|| load(ZH_CN, "zh-CN")),
        Locale::EnUs => EN.get_or_init(|| load(EN_US, "en-US")),
    }
}

/// 当前语言。
pub(crate) fn locale() -> Locale {
    let code = CURRENT.load(Ordering::Relaxed);
    if code != UNSET {
        return Locale::from_code(code);
    }
    let detected = Locale::detect();
    let _ = CURRENT.compare_exchange(UNSET, detected.code(), Ordering::Relaxed, Ordering::Relaxed);
    Locale::from_code(CURRENT.load(Ordering::Relaxed))
}

/// 切换当前语言。
pub(crate) fn set_locale(locale: Locale) {
    CURRENT.store(locale.code(), Ordering::Relaxed);
}

/// 取词条；缺失时回显 key，便于定位。
pub(crate) fn t(key: &str) -> String {
    catalog(locale())
        .get(key)
        .cloned()
        .unwrap_or_else(|| key.to_owned())
}

/// 取词条并替换 `{name}` 占位符。
pub(crate) fn tf(key: &str, vars: &[(&str, &str)]) -> String {
    let mut text = t(key);
    for (name, value) in vars {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

/// AppError → 当前语言文案（与 Tauri `localizedMessage` 同口径：
/// `Auth` 保持后端原文，其余已知 kind 用词条，未知 key 回落原文）。
pub(crate) fn app_error_text(error: &hongguo_common::AppError) -> String {
    let key = error.i18n_key();
    if key == "error.auth" {
        return error.to_string();
    }
    let text = t(key);
    if text == key { error.to_string() } else { text }
}

#[cfg(test)]
mod tests {
    use super::{Locale, catalog};

    #[test]
    fn locale_catalogs_have_the_same_keys() {
        let mut zh: Vec<_> = catalog(Locale::ZhCn).keys().cloned().collect();
        let mut en: Vec<_> = catalog(Locale::EnUs).keys().cloned().collect();
        zh.sort();
        en.sort();
        assert_eq!(zh, en);
    }

    #[test]
    fn placeholder_formatting_keeps_unknown_placeholders() {
        let count = "12";
        assert!(super::tf("search.resultCount", &[("total", count)]).contains(count));
    }

    #[test]
    fn app_errors_use_the_localized_kind() {
        let error = hongguo_common::AppError::Network("socket closed".into());
        assert!(!super::app_error_text(&error).contains("socket closed"));
    }
}
