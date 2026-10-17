//! 壳级 UI 偏好（GPUI）：侧栏折叠态 / 追更提醒弹出开关 / 找剧筛选折叠态 /
//! 主题偏好 / 界面语言。
//!
//! 语义对齐 Tauri 的 zustand `hongguo-ui` persist（localStorage，见
//! `src/stores/ui.ts` 的 partialize）——纯 UI 偏好不进业务库；用数据目录下
//! 一个键值文件承载，坏档回默认。窗口级瞬态（小屏/置顶/全屏）不在此列：
//! 那些与会话绑定，不持久化（Tauri partialize 同款口径）。

use std::path::Path;

/// 数据目录下的文件名。
pub(crate) const FILE_NAME: &str = "gpui-ui-prefs";

/// 主题偏好（Tauri `hongguo-theme` store 同款三档）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThemePref {
    /// 跟随系统（默认，Tauri `theme: 'auto'` 同款）。
    Auto,
    Light,
    Dark,
}

impl ThemePref {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ThemePref::Auto => "auto",
            ThemePref::Light => "light",
            ThemePref::Dark => "dark",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "light" => ThemePref::Light,
            "dark" => ThemePref::Dark,
            _ => ThemePref::Auto,
        }
    }

    /// 偏好 → 实际主题档（`auto` 由系统外观决定）。
    pub(crate) fn resolve(
        self,
        appearance: gpui_kit::WindowAppearance,
    ) -> gpui_kit::component::ThemeMode {
        use gpui_kit::WindowAppearance as A;
        match self {
            ThemePref::Light => gpui_kit::component::ThemeMode::Light,
            ThemePref::Dark => gpui_kit::component::ThemeMode::Dark,
            ThemePref::Auto => match appearance {
                A::Light | A::VibrantLight => gpui_kit::component::ThemeMode::Light,
                _ => gpui_kit::component::ThemeMode::Dark,
            },
        }
    }
}

/// UI 偏好。默认值与 Tauri 一致：侧栏默认折叠（用户展开后持久化记住）、
/// 追更提醒默认弹出、主题跟随系统。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UiPrefs {
    pub(crate) sidebar_collapsed: bool,
    pub(crate) aibot_popup_on: bool,
    /// 找剧筛选面板折叠态（Tauri `browseFiltersCollapsed` 同款）。
    pub(crate) browse_filters_collapsed: bool,
    pub(crate) theme: ThemePref,
    /// 界面语言（Tauri `hongguo-locale` store 同义；独立壳各存各的）。
    pub(crate) locale: crate::i18n::Locale,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            sidebar_collapsed: true,
            aibot_popup_on: true,
            browse_filters_collapsed: false,
            theme: ThemePref::Auto,
            locale: crate::i18n::locale(),
        }
    }
}

/// 读偏好；文件缺失/坏行一律回落默认，不因半写或手改坏档影响壳启动。
pub(crate) fn load_from(path: &Path) -> UiPrefs {
    let Ok(text) = std::fs::read_to_string(path) else {
        return UiPrefs::default();
    };
    let mut prefs = UiPrefs::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "sidebar_collapsed" => prefs.sidebar_collapsed = value.trim() == "true",
            "aibot_popup_on" => prefs.aibot_popup_on = value.trim() == "true",
            "browse_filters_collapsed" => prefs.browse_filters_collapsed = value.trim() == "true",
            "theme" => prefs.theme = ThemePref::parse(value.trim()),
            "locale" => {
                if let Some(locale) = crate::i18n::Locale::parse(value.trim()) {
                    prefs.locale = locale;
                }
            }
            _ => {}
        }
    }
    prefs
}

/// 写偏好：同目录临时文件 + rename，半写状态不会留下坏档。
pub(crate) fn save_to(path: &Path, prefs: &UiPrefs) {
    let tmp = path.with_extension("tmp");
    let text = format!(
        "sidebar_collapsed={}\naibot_popup_on={}\nbrowse_filters_collapsed={}\ntheme={}\nlocale={}\n",
        prefs.sidebar_collapsed,
        prefs.aibot_popup_on,
        prefs.browse_filters_collapsed,
        prefs.theme.as_str(),
        prefs.locale.tag()
    );
    if std::fs::write(&tmp, text)
        .and_then(|_| std::fs::rename(&tmp, path))
        .is_err()
    {
        log::warn!("[Shell] UI 偏好写盘失败（下次启动回默认）");
    }
}

/// 读取当前盘上的偏好（壳与页面共用同一入口，避免各自拼路径）。
pub(crate) fn load() -> UiPrefs {
    load_from(&hongguo_store::paths::data_dir().join(FILE_NAME))
}

/// 落盘当前偏好（读-改-写由调用方按字段维护，见 [`crate::shell`] 与追更页）。
pub(crate) fn save(prefs: UiPrefs) {
    crate::bridge::spawn(async move {
        let path = hongguo_store::paths::data_dir().join(FILE_NAME);
        let _ = tokio::task::spawn_blocking(move || save_to(&path, &prefs)).await;
    });
}
