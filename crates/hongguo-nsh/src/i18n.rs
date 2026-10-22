//! 壳文案：直接复用 Tauri 壳的 `src/locales/*.json`（两壳共用同一份词条，
//! 避免同一句文案在两处资源里漂移）。编译期嵌入，运行时扁平化成
//! `a.b.c -> value`，切换语言只改进程内状态。
//!
//! 默认语言跟随系统（nsh 的 `locale()` 给的是 BCP 47 tag）；`HONGGUO_LOCALE`
//! 可显式覆盖，与 GPUI 壳同一约定。

use std::collections::HashMap;
use std::sync::OnceLock;

const ZH_CN: &str = include_str!("../../../src/locales/zh-CN.json");
const EN_US: &str = include_str!("../../../src/locales/en-US.json");

/// 壳支持的语言（与 Tauri `src/locales/index.ts` 的资源键一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    ZhCn,
    EnUs,
}

impl Locale {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().split(['-', '_']).next()? {
            "zh" => Some(Self::ZhCn),
            "en" => Some(Self::EnUs),
            _ => None,
        }
    }

    /// 首次启动的默认语言：显式环境变量优先，其次系统 UI 语言。
    fn detect() -> Self {
        if let Ok(value) = std::env::var("HONGGUO_LOCALE")
            && let Some(locale) = Self::parse(&value)
        {
            return locale;
        }
        Locale::parse(nsh_core::ui::prelude::locale().as_str()).unwrap_or(Self::ZhCn)
    }
}

/// 当前语言（进程内唯一事实源；`main` 在开窗前定一次）。
pub fn locale() -> Locale {
    static CURRENT: OnceLock<Locale> = OnceLock::new();
    *CURRENT.get_or_init(Locale::detect)
}

/// 查词条：`t("nav.home.title")`。缺词条时回退成 key 本身——宁可露出 key
/// 也不静默给空串（空串在 UI 上是「丢了一块」，比 key 更难排查）。
pub fn t(key: &str) -> String {
    catalog(locale())
        .get(key)
        .cloned()
        .unwrap_or_else(|| key.to_owned())
}

fn catalog(locale: Locale) -> &'static HashMap<String, String> {
    static ZH: OnceLock<HashMap<String, String>> = OnceLock::new();
    static EN: OnceLock<HashMap<String, String>> = OnceLock::new();
    match locale {
        Locale::ZhCn => ZH.get_or_init(|| load(ZH_CN, "zh-CN")),
        Locale::EnUs => EN.get_or_init(|| load(EN_US, "en-US")),
    }
}

fn load(source: &str, label: &str) -> HashMap<String, String> {
    let root: serde_json::Value = serde_json::from_str(source)
        .unwrap_or_else(|e| panic!("内嵌语言资源 {label} 不是合法 JSON：{e}"));
    let mut out = HashMap::new();
    flatten("", &root, &mut out);
    out
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
