//! 播放偏好（GPUI 壳）：倍速 / 音量 / 静音 / 弹幕显示参数 / 隐身开关。壳层
//! 关注点不进业务库，用数据目录下一个键值文件承载，坏档回默认；换集重建
//! 引擎后由壳把存值重放给新引擎。解析逻辑刻意压到几行平凡的 parse/clamp
//! （本壳 bin-only，正确性由引擎侧的钳制测试兜底）。

use std::path::Path;

/// 数据目录下的文件名。
pub(crate) const FILE_NAME: &str = "gpui-playback-prefs";

/// 播放器快捷键映射（值 = GPUI keystroke 键名：`space` / `left` / `a`…）。
/// 语义对齐 Tauri 的 hongguo-shortcuts（默认空格 / ←→ / ↑↓）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShortcutKeys {
    pub(crate) play_pause: String,
    pub(crate) seek_back: String,
    pub(crate) seek_forward: String,
    pub(crate) step_prev: String,
    pub(crate) step_next: String,
}

impl Default for ShortcutKeys {
    fn default() -> Self {
        Self {
            play_pause: "space".into(),
            seek_back: "left".into(),
            seek_forward: "right".into(),
            step_prev: "up".into(),
            step_next: "down".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlaybackPrefs {
    /// 倍速（0.25–4，与引擎 MIN_RATE/MAX_RATE 一致）。
    pub(crate) rate: f64,
    /// 音量（0–1）。
    pub(crate) volume: f32,
    /// 静音独立于音量（对齐 Tauri：静音后音量值保留）。
    pub(crate) muted: bool,
    /// 弹幕开关（Tauri readDanmakuEnabled 同语义）。
    pub(crate) danmaku_on: bool,
    /// 弹幕文字不透明度 0.1–1（Tauri DanmakuDisplaySettings.opacity）。
    pub(crate) dm_opacity: f32,
    /// 弹幕字号缩放 0.5–2（1 = 基准字号）。
    pub(crate) dm_font_scale: f32,
    /// 弹幕密度 0–1（1 = 全部显示，入场按比例丢弃）。
    pub(crate) dm_density: f32,
    /// 弹幕显示区域 0.25–1（占画面顶部高度比例）。
    pub(crate) dm_area: f32,
    /// 隐身模式开关（Tauri readIncognito 同语义：跨启动还原轮询态）。
    pub(crate) incognito: bool,
    /// 播放器快捷键映射（设置页可改；空值回落默认键）。
    pub(crate) keys: ShortcutKeys,
}

/// 弹幕显示参数默认值（Tauri `DANMAKU_DEFAULTS`：50%/50%/50%/25%）。
pub(crate) const DM_DEFAULTS: DanmakuPrefs = DanmakuPrefs {
    on: true,
    opacity: 0.5,
    font_scale: 0.5,
    density: 0.5,
    area: 0.25,
};

/// 弹幕显示参数（与播放偏好同文件承载）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DanmakuPrefs {
    pub(crate) on: bool,
    pub(crate) opacity: f32,
    pub(crate) font_scale: f32,
    pub(crate) density: f32,
    pub(crate) area: f32,
}

impl Default for PlaybackPrefs {
    fn default() -> Self {
        Self {
            rate: 1.0,
            volume: 1.0,
            muted: false,
            danmaku_on: DM_DEFAULTS.on,
            dm_opacity: DM_DEFAULTS.opacity,
            dm_font_scale: DM_DEFAULTS.font_scale,
            dm_density: DM_DEFAULTS.density,
            dm_area: DM_DEFAULTS.area,
            incognito: false,
            keys: ShortcutKeys::default(),
        }
    }
}

/// 读偏好；文件缺失/坏行/越界值一律回落默认，不因半写或手改坏档影响起播。
pub(crate) fn load_from(path: &Path) -> PlaybackPrefs {
    let Ok(text) = std::fs::read_to_string(path) else {
        return PlaybackPrefs::default();
    };
    let mut prefs = PlaybackPrefs::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "rate" => {
                if let Ok(v) = value.trim().parse::<f64>() {
                    prefs.rate = v.clamp(0.25, 4.0);
                }
            }
            "volume" => {
                if let Ok(v) = value.trim().parse::<f32>() {
                    prefs.volume = v.clamp(0.0, 1.0);
                }
            }
            "muted" => prefs.muted = value.trim() == "true",
            "dm_on" => prefs.danmaku_on = value.trim() == "true",
            "dm_opacity" => {
                if let Ok(v) = value.trim().parse::<f32>() {
                    prefs.dm_opacity = v.clamp(0.1, 1.0);
                }
            }
            "dm_font" => {
                if let Ok(v) = value.trim().parse::<f32>() {
                    prefs.dm_font_scale = v.clamp(0.5, 2.0);
                }
            }
            "dm_density" => {
                if let Ok(v) = value.trim().parse::<f32>() {
                    prefs.dm_density = v.clamp(0.0, 1.0);
                }
            }
            "dm_area" => {
                if let Ok(v) = value.trim().parse::<f32>() {
                    prefs.dm_area = v.clamp(0.25, 1.0);
                }
            }
            "incognito" => prefs.incognito = value.trim() == "true",
            "key_play_pause" | "key_seek_back" | "key_seek_forward" | "key_step_prev"
            | "key_step_next" => {
                let v = value.trim().to_lowercase();
                if v.is_empty() || v.contains('=') {
                    continue; // 坏值回落默认键
                }
                match key.trim() {
                    "key_play_pause" => prefs.keys.play_pause = v,
                    "key_seek_back" => prefs.keys.seek_back = v,
                    "key_seek_forward" => prefs.keys.seek_forward = v,
                    "key_step_prev" => prefs.keys.step_prev = v,
                    _ => prefs.keys.step_next = v,
                }
            }
            _ => {}
        }
    }
    prefs
}

/// 写偏好：同目录临时文件 + rename，半写状态不会留下坏档。
pub(crate) fn save_to(path: &Path, prefs: &PlaybackPrefs) {
    let tmp = path.with_extension("tmp");
    let text = format!(
        "rate={}\nvolume={}\nmuted={}\ndm_on={}\ndm_opacity={}\ndm_font={}\ndm_density={}\ndm_area={}\nincognito={}\nkey_play_pause={}\nkey_seek_back={}\nkey_seek_forward={}\nkey_step_prev={}\nkey_step_next={}\n",
        prefs.rate,
        prefs.volume,
        prefs.muted,
        prefs.danmaku_on,
        prefs.dm_opacity,
        prefs.dm_font_scale,
        prefs.dm_density,
        prefs.dm_area,
        prefs.incognito,
        prefs.keys.play_pause,
        prefs.keys.seek_back,
        prefs.keys.seek_forward,
        prefs.keys.step_prev,
        prefs.keys.step_next
    );
    if std::fs::write(&tmp, text)
        .and_then(|_| std::fs::rename(&tmp, path))
        .is_err()
    {
        log::warn!("[Home] 播放偏好写盘失败（下次启动回默认）");
    }
}
