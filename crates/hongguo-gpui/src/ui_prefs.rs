//! 壳级 UI 偏好（GPUI）：侧栏折叠态 / 追更提醒弹出开关 / 找剧筛选折叠态 /
//! 主题偏好 / 界面语言 / 三种窗态的窗口几何。
//!
//! 语义对齐 Tauri 的 zustand `hongguo-ui` persist（localStorage，见
//! `src/stores/ui.ts` 的 partialize）——纯 UI 偏好不进业务库；用数据目录下
//! 一个键值文件承载，坏档回默认。会话级瞬态（置顶）不在此列：那些与
//! 会话绑定，不持久化（Tauri partialize 同款口径）；窗口几何是例外——
//! 跨启动记忆（issue #11），常态/小屏/竖屏各记各的槽位，Tauri 壳的
//! `window_state.rs` 同语义。

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
    /// 上次窗口几何（逻辑像素；issue #11）。`win_w`/`win_h` 为 0 = 无记忆
    /// （首次启动/记录不可用），启动回中置默认。
    pub(crate) win_x: f32,
    pub(crate) win_y: f32,
    pub(crate) win_w: f32,
    pub(crate) win_h: f32,
    /// 上次小屏几何（逻辑像素；同 win_* 语义）。三种窗态各记各的槽位，
    /// 切形态互不覆盖（进小屏时由 [`crate::mini`] 取用）。
    pub(crate) mini_x: f32,
    pub(crate) mini_y: f32,
    pub(crate) mini_w: f32,
    pub(crate) mini_h: f32,
    /// 上次竖屏几何（逻辑像素；进竖屏时由 [`crate::portrait`] 取用）。
    pub(crate) portrait_x: f32,
    pub(crate) portrait_y: f32,
    pub(crate) portrait_w: f32,
    pub(crate) portrait_h: f32,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            sidebar_collapsed: true,
            aibot_popup_on: true,
            browse_filters_collapsed: false,
            theme: ThemePref::Auto,
            locale: crate::i18n::locale(),
            win_x: 0.0,
            win_y: 0.0,
            win_w: 0.0,
            win_h: 0.0,
            mini_x: 0.0,
            mini_y: 0.0,
            mini_w: 0.0,
            mini_h: 0.0,
            portrait_x: 0.0,
            portrait_y: 0.0,
            portrait_w: 0.0,
            portrait_h: 0.0,
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
            "win_x" => prefs.win_x = value.trim().parse().unwrap_or(0.0),
            "win_y" => prefs.win_y = value.trim().parse().unwrap_or(0.0),
            "win_w" => prefs.win_w = value.trim().parse().unwrap_or(0.0),
            "win_h" => prefs.win_h = value.trim().parse().unwrap_or(0.0),
            "mini_x" => prefs.mini_x = value.trim().parse().unwrap_or(0.0),
            "mini_y" => prefs.mini_y = value.trim().parse().unwrap_or(0.0),
            "mini_w" => prefs.mini_w = value.trim().parse().unwrap_or(0.0),
            "mini_h" => prefs.mini_h = value.trim().parse().unwrap_or(0.0),
            "portrait_x" => prefs.portrait_x = value.trim().parse().unwrap_or(0.0),
            "portrait_y" => prefs.portrait_y = value.trim().parse().unwrap_or(0.0),
            "portrait_w" => prefs.portrait_w = value.trim().parse().unwrap_or(0.0),
            "portrait_h" => prefs.portrait_h = value.trim().parse().unwrap_or(0.0),
            _ => {}
        }
    }
    prefs
}

/// 写偏好：同目录临时文件 + rename，半写状态不会留下坏档。
pub(crate) fn save_to(path: &Path, prefs: &UiPrefs) {
    let tmp = path.with_extension("tmp");
    let text = format!(
        "sidebar_collapsed={}\naibot_popup_on={}\nbrowse_filters_collapsed={}\ntheme={}\nlocale={}\nwin_x={}\nwin_y={}\nwin_w={}\nwin_h={}\n",
        prefs.sidebar_collapsed,
        prefs.aibot_popup_on,
        prefs.browse_filters_collapsed,
        prefs.theme.as_str(),
        prefs.locale.tag(),
        prefs.win_x,
        prefs.win_y,
        prefs.win_w,
        prefs.win_h,
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

/// 同步落盘（退出路径用：`cx.quit()` 之后 tokio 桥不再保证跑，窗口几何
/// 随退出保存必须走这条）。
pub(crate) fn save_sync(prefs: &UiPrefs) {
    save_to(&hongguo_store::paths::data_dir().join(FILE_NAME), prefs);
}

/// 几何槽位（形态退出落盘用，见 [`store_geometry`]）。
pub(crate) enum GeometrySlot {
    Mini,
    Portrait,
}

/// 把一份几何写进指定形态槽位并同步落盘（其余键保持盘上值）。塌缩过该
/// 形态恢复下限的值直接丢弃——过渡态读到的坏值不能毒化槽位。
///
/// 形态退出路径专用：几何采样只进内存（`Shell` 槽位，关闭守卫才落盘），
/// 会话内退出再重进读的是这个文件——不在这里补一笔，拖完立刻退出重进
/// 就会回到拖动前的旧位置。
pub(crate) fn store_geometry(slot: GeometrySlot, x: f32, y: f32, w: f32, h: f32) {
    let (floor_w, floor_h) = match slot {
        GeometrySlot::Mini => (
            crate::window_restore::MINI_RESTORE_W,
            crate::window_restore::MINI_RESTORE_H,
        ),
        GeometrySlot::Portrait => (
            crate::window_restore::PORTRAIT_RESTORE_W,
            crate::window_restore::PORTRAIT_RESTORE_H,
        ),
    };
    if w < floor_w || h < floor_h {
        return;
    }
    let mut prefs = load();
    match slot {
        GeometrySlot::Mini => {
            (prefs.mini_x, prefs.mini_y, prefs.mini_w, prefs.mini_h) = (x, y, w, h)
        }
        GeometrySlot::Portrait => {
            (
                prefs.portrait_x,
                prefs.portrait_y,
                prefs.portrait_w,
                prefs.portrait_h,
            ) = (x, y, w, h);
        }
    }
    save_sync(&prefs);
}
