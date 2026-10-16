//! 设置页（Tauri `/settings` = `settings-page.tsx` 的 GPUI 对齐）。
//!
//! 卡片：账户（登录态/退出）/ 下载目录（可选目录）/ 命名与并发 / 网络代理
//! （含测试连接）/ 播放（三开关 + 转码后端探测 + ffmpeg 指引）/ 存储
//! （缓存目录 + 占用与缓存清理）/ 更新（版本展示）/ 保存。草稿语义同
//! Tauri：改过才进草稿，保存成功丢草稿回「跟随服务端」。保存与探测走
//! service（`settings_service::save`、`storage_service`、`capability::probe`），
//! 与 Tauri 的 settings/transcode command 同一段。

use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::label::Label;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{App, ClipboardItem, Entity, RenderImage, SharedString, Window, div};
use hongguo_common::model::settings::{NamingTemplate, ProxyMode};
use hongguo_common::model::{AccountState, ProxyTestResult, Settings};
use hongguo_media_platform::capability::probe::DecodeCapability;
use hongguo_service::app_state::AppStateInner;
use hongguo_service::storage_service::cache::CacheUsage;
use hongguo_service::storage_service::usage::StorageUsage;

use crate::bridge;
use crate::home::fmt::format_bytes;
use crate::pages::card;

/// 设置页发往壳的事件（登录入口切回首页开登录面板）。
#[derive(Debug, Clone)]
pub(crate) enum SettingsEvent {
    Login,
    /// 快捷键映射变更：壳转给首页播放器（值同时已落盘）。
    ShortcutKeys(crate::playback_prefs::ShortcutKeys),
    /// 主题偏好变更（值同时已落盘；壳据此更新 auto 观察者的判据）。
    ThemeChanged(crate::ui_prefs::ThemePref),
}

/// 可自定义的播放器动作（与 Tauri ShortcutAction 同集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShortcutAction {
    PlayPause,
    SeekBack,
    SeekForward,
    StepPrev,
    StepNext,
}

impl ShortcutAction {
    const ALL: [ShortcutAction; 5] = [
        ShortcutAction::PlayPause,
        ShortcutAction::SeekBack,
        ShortcutAction::SeekForward,
        ShortcutAction::StepPrev,
        ShortcutAction::StepNext,
    ];

    fn label(self) -> &'static str {
        match self {
            ShortcutAction::PlayPause => "播放 / 暂停",
            ShortcutAction::SeekBack => "快退 5 秒",
            ShortcutAction::SeekForward => "快进 5 秒",
            ShortcutAction::StepPrev => "上一集 / 上一部",
            ShortcutAction::StepNext => "下一集 / 下一部",
        }
    }

    fn get(self, keys: &crate::playback_prefs::ShortcutKeys) -> &str {
        match self {
            ShortcutAction::PlayPause => &keys.play_pause,
            ShortcutAction::SeekBack => &keys.seek_back,
            ShortcutAction::SeekForward => &keys.seek_forward,
            ShortcutAction::StepPrev => &keys.step_prev,
            ShortcutAction::StepNext => &keys.step_next,
        }
    }

    fn set(self, keys: &mut crate::playback_prefs::ShortcutKeys, value: String) {
        match self {
            ShortcutAction::PlayPause => keys.play_pause = value,
            ShortcutAction::SeekBack => keys.seek_back = value,
            ShortcutAction::SeekForward => keys.seek_forward = value,
            ShortcutAction::StepPrev => keys.step_prev = value,
            ShortcutAction::StepNext => keys.step_next = value,
        }
    }
}

/// GPUI keystroke 键名 → 可读标签（Tauri shortcutKeyLabel 同款）。
fn shortcut_key_label(key: &str) -> String {
    match key {
        "space" => "空格".to_string(),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "escape" => "Esc".to_string(),
        other if other.chars().count() == 1 => other.to_uppercase(),
        other => other.to_string(),
    }
}

const NAMING_LABELS: [&str; 3] = ["剧名 集数", "剧名 集数 标题", "仅剧名"];
const PROXY_LABELS: [&str; 3] = ["跟随系统", "手动指定", "强制直连"];

pub(crate) struct SettingsView {
    state: Arc<AppStateInner>,
    loaded: bool,
    loading: bool,
    /// 草稿（None = 未修改，跟随服务端值）。
    draft: Option<Settings>,
    dir_input: Entity<InputState>,
    cache_dir_input: Entity<InputState>,
    proxy_input: Entity<InputState>,
    naming_select: Entity<SelectState<SearchableVec<SharedString>>>,
    proxy_select: Entity<SelectState<SearchableVec<SharedString>>>,
    concurrency: Entity<SliderState>,
    prefetch: Entity<SliderState>,
    capability: Option<DecodeCapability>,
    redetecting: bool,
    redetect_done: bool,
    usage: Option<StorageUsage>,
    cache_usage: Option<CacheUsage>,
    clearing_cache: bool,
    testing_proxy: bool,
    test_result: Option<ProxyTestResult>,
    saving: bool,
    account_avatar: Option<Arc<RenderImage>>,
    /// 快捷键映射（设置页与播放器共用一份；改动即存即生效）。
    shortcut_keys: crate::playback_prefs::ShortcutKeys,
    /// 正在捕获按键的动作（Esc 取消；纯修饰键忽略）。
    capturing: Option<ShortcutAction>,
    /// 捕获焦点锚：进入捕获态后由根元素接管键盘。
    keys_focus: gpui_kit::FocusHandle,
    /// 主题偏好（Tauri 的主题切换在顶栏；GPUI 顶栏是原生拖拽区，入口落在
    /// 设置页——三档语义与 Tauri `hongguo-theme` store 相同）。
    theme: crate::ui_prefs::ThemePref,
}

fn naming_index(naming: NamingTemplate) -> usize {
    match naming {
        NamingTemplate::TitleIndex => 0,
        NamingTemplate::TitleIndexEpisode => 1,
        NamingTemplate::OnlyTitle => 2,
    }
}

fn naming_from_index(ix: usize) -> NamingTemplate {
    match ix {
        1 => NamingTemplate::TitleIndexEpisode,
        2 => NamingTemplate::OnlyTitle,
        _ => NamingTemplate::TitleIndex,
    }
}

fn proxy_index(mode: ProxyMode) -> usize {
    match mode {
        ProxyMode::System => 0,
        ProxyMode::Manual => 1,
        ProxyMode::Direct => 2,
    }
}

fn proxy_from_index(ix: usize) -> ProxyMode {
    match ix {
        1 => ProxyMode::Manual,
        2 => ProxyMode::Direct,
        _ => ProxyMode::System,
    }
}

impl SettingsView {
    pub(crate) fn new(
        state: Arc<AppStateInner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let dir_input = cx.new(|cx| InputState::new(window, cx));
        let cache_dir_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("留空跟随数据目录"));
        let proxy_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("http://127.0.0.1:7890"));
        let naming_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(
                    NAMING_LABELS
                        .iter()
                        .map(|s| SharedString::from(*s))
                        .collect::<Vec<SharedString>>(),
                ),
                None,
                window,
                cx,
            )
        });
        let proxy_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(
                    PROXY_LABELS
                        .iter()
                        .map(|s| SharedString::from(*s))
                        .collect::<Vec<SharedString>>(),
                ),
                None,
                window,
                cx,
            )
        });
        let concurrency = cx.new(|_| {
            SliderState::new()
                .min(1.)
                .max(10.)
                .step(1.)
                .default_value(3.)
        });
        let prefetch = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(3.)
                .step(1.)
                .default_value(1.)
        });
        cx.subscribe_in(
            &naming_select,
            window,
            |this, _, event: &SelectEvent<SearchableVec<SharedString>>, _, cx| {
                if let SelectEvent::Confirm(Some(value)) = event
                    && let Some(ix) = NAMING_LABELS.iter().position(|l| *l == value.as_str())
                {
                    this.patch(cx, |s| s.naming = naming_from_index(ix));
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &proxy_select,
            window,
            |this, _, event: &SelectEvent<SearchableVec<SharedString>>, _, cx| {
                if let SelectEvent::Confirm(Some(value)) = event
                    && let Some(ix) = PROXY_LABELS.iter().position(|l| *l == value.as_str())
                {
                    this.patch(cx, |s| s.proxy.mode = proxy_from_index(ix));
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &concurrency,
            window,
            |this, _, event: &SliderEvent, _, cx| {
                let SliderEvent::Change(value) = event else {
                    return;
                };
                let n = value.start().round().clamp(1.0, 10.0) as usize;
                this.patch(cx, |s| s.max_concurrency = n);
            },
        )
        .detach();
        cx.subscribe_in(&prefetch, window, |this, _, event: &SliderEvent, _, cx| {
            let SliderEvent::Change(value) = event else {
                return;
            };
            let n = value.start().round().clamp(0.0, 3.0) as u32;
            this.patch(cx, |s| s.prefetch_episodes = n);
        })
        .detach();
        Self {
            state,
            loaded: false,
            loading: false,
            draft: None,
            dir_input,
            cache_dir_input,
            proxy_input,
            naming_select,
            proxy_select,
            concurrency,
            prefetch,
            capability: None,
            redetecting: false,
            redetect_done: false,
            usage: None,
            cache_usage: None,
            clearing_cache: false,
            testing_proxy: false,
            test_result: None,
            saving: false,
            account_avatar: None,
            shortcut_keys: crate::playback_prefs::ShortcutKeys::default(),
            capturing: None,
            keys_focus: cx.focus_handle(),
            theme: crate::ui_prefs::load().theme,
        }
    }

    fn current(&self) -> Settings {
        self.draft.clone().unwrap_or_else(|| self.state.settings())
    }

    /// 切换主题：立即应用 + 落盘 + 通知壳（auto 观察者换判据）。
    fn set_theme(&mut self, pref: crate::ui_prefs::ThemePref, cx: &mut Context<Self>) {
        use crate::ui_prefs::ThemePref;
        self.theme = pref;
        let mut prefs = crate::ui_prefs::load();
        prefs.theme = pref;
        crate::ui_prefs::save(prefs);
        let mode = pref.resolve(cx.window_appearance());
        gpui_kit::component::Theme::change(mode, None, cx);
        // 显式档位把窗口外观钉给平台（macOS 窗口边框跟随）；auto 交还系统
        cx.set_window_appearance(match pref {
            ThemePref::Auto => None,
            ThemePref::Light => Some(gpui_kit::WindowAppearance::Light),
            ThemePref::Dark => Some(gpui_kit::WindowAppearance::Dark),
        });
        cx.emit(SettingsEvent::ThemeChanged(pref));
        cx.notify();
    }

    fn patch(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Settings)) {
        let mut current = self.current();
        f(&mut current);
        self.draft = Some(current);
        cx.notify();
    }

    /// 进入页面时拉占用/缓存/能力探测（探测首次 1–2 秒，走阻塞池）。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        if self.loaded || self.loading {
            return;
        }
        self.loading = true;
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            tokio::task::spawn_blocking(move || {
                let usage = hongguo_service::storage_service::usage::usage(&state);
                let cache_usage = hongguo_service::storage_service::cache::usage();
                let capability = hongguo_media_platform::capability::probe::detect();
                (usage, cache_usage, capability)
            })
            .await
            .expect("阻塞池 join")
        });
        cx.spawn(async move |this, cx| {
            let (usage, cache_usage, capability) = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.loaded = true;
                view.loading = false;
                view.usage = Some(usage);
                view.cache_usage = Some(cache_usage);
                view.capability = Some(capability);
                // 快捷键映射从偏好文件带上（与播放器同一份）
                let prefs_path =
                    hongguo_store::paths::data_dir().join(crate::playback_prefs::FILE_NAME);
                view.shortcut_keys = crate::playback_prefs::load_from(&prefs_path).keys;
                // 表单初值跟随服务端
                let settings = view.state.settings();
                let dir = settings.download_dir.clone();
                view.dir_input
                    .update(cx, |input, cx| input.set_value(dir, window, cx));
                let cache_dir = settings.cache_dir.clone();
                view.cache_dir_input
                    .update(cx, |input, cx| input.set_value(cache_dir, window, cx));
                let url = settings.proxy.url.clone();
                view.proxy_input
                    .update(cx, |input, cx| input.set_value(url, window, cx));
                let naming = gpui_kit::component::IndexPath::new(naming_index(settings.naming));
                view.naming_select.update(cx, |select, cx| {
                    select.set_selected_index(Some(naming), window, cx)
                });
                let proxy = gpui_kit::component::IndexPath::new(proxy_index(settings.proxy.mode));
                view.proxy_select.update(cx, |select, cx| {
                    select.set_selected_index(Some(proxy), window, cx)
                });
                let concurrency = settings.max_concurrency as f32;
                view.concurrency
                    .update(cx, |state, cx| state.set_value(concurrency, window, cx));
                let prefetch = settings.prefetch_episodes as f32;
                view.prefetch
                    .update(cx, |state, cx| state.set_value(prefetch, window, cx));
                if let Some(account) = &settings.account
                    && !account.avatar_url.is_empty()
                {
                    view.load_avatar(account.avatar_url.clone(), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn load_avatar(&mut self, url: String, cx: &mut Context<Self>) {
        let rx = crate::cover_img::fetch(url);
        cx.spawn(async move |this, cx| {
            let bytes = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                if let Ok(bytes) = bytes
                    && let Ok(decoded) = image::load_from_memory(&bytes)
                {
                    view.account_avatar = Some(Arc::new(crate::image_upload::render_image(
                        decoded.to_rgba8(),
                    )));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn pick_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择下载目录".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = rx.await;
            let _ = this.update_in(cx, |view, window, cx| {
                if let Ok(Ok(Some(paths))) = result
                    && let Some(path) = paths.first()
                {
                    let text = path.to_string_lossy().to_string();
                    view.dir_input
                        .update(cx, |input, cx| input.set_value(text, window, cx));
                    let text = view.dir_input.read(cx).value().to_string();
                    view.patch(cx, |s| s.download_dir = text);
                }
            });
        })
        .detach();
    }

    /// 选缓存目录：与下载目录同流程，写进 cache_dir_input（空 = 跟随数据目录）。
    fn pick_cache_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择缓存目录".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = rx.await;
            let _ = this.update_in(cx, |view, window, cx| {
                if let Ok(Ok(Some(paths))) = result
                    && let Some(path) = paths.first()
                {
                    let text = path.to_string_lossy().to_string();
                    view.cache_dir_input
                        .update(cx, |input, cx| input.set_value(text, window, cx));
                    let text = view.cache_dir_input.read(cx).value().to_string();
                    view.patch(cx, |s| s.cache_dir = text);
                }
            });
        })
        .detach();
    }

    fn test_proxy(&mut self, cx: &mut Context<Self>) {
        if self.testing_proxy {
            return;
        }
        self.testing_proxy = true;
        self.test_result = None;
        cx.notify();
        let mut cfg = self.current().proxy;
        if cfg.mode == ProxyMode::Manual {
            cfg.url = self.proxy_input.read(cx).value().trim().to_string();
        }
        let rx = bridge::spawn(async move {
            hongguo_service::settings_service::proxy::test_proxy(&cfg).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.testing_proxy = false;
                match result {
                    Ok(r) => {
                        window.push_notification(
                            if r.ok {
                                Notification::success(format!(
                                    "连接正常，耗时 {} 毫秒",
                                    r.elapsed_ms
                                ))
                            } else {
                                Notification::error(format!("连接失败：{}", r.message))
                            },
                            cx,
                        );
                        view.test_result = Some(r);
                    }
                    Err(e) => {
                        log::warn!("[Settings] 代理测试失败: {e}");
                        window.push_notification(Notification::error(format!("连接失败：{e}")), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn redetect_capability(&mut self, cx: &mut Context<Self>) {
        if self.redetecting {
            return;
        }
        self.redetecting = true;
        cx.notify();
        let rx = bridge::spawn(async move {
            tokio::task::spawn_blocking(hongguo_media_platform::capability::probe::redetect)
                .await
                .expect("阻塞池 join")
        });
        cx.spawn(async move |this, cx| {
            let capability = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.redetecting = false;
                view.redetect_done = true;
                view.capability = Some(capability);
                cx.notify();
            });
        })
        .detach();
    }

    fn clear_cache(&mut self, cx: &mut Context<Self>) {
        if self.clearing_cache {
            return;
        }
        self.clearing_cache = true;
        cx.notify();
        let rx = bridge::spawn(async move {
            tokio::task::spawn_blocking(hongguo_service::storage_service::cache::clear)
                .await
                .expect("阻塞池 join")
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.clearing_cache = false;
                window.push_notification(
                    Notification::success(format!(
                        "已清理 {} 项，释放 {}",
                        result.items,
                        format_bytes(result.bytes)
                    )),
                    cx,
                );
                view.cache_usage = Some(hongguo_service::storage_service::cache::usage());
                cx.notify();
            });
        })
        .detach();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let mut next = self.current();
        next.download_dir = self.dir_input.read(cx).value().trim().to_string();
        next.cache_dir = self.cache_dir_input.read(cx).value().trim().to_string();
        if next.proxy.mode == ProxyMode::Manual {
            next.proxy.url = self.proxy_input.read(cx).value().trim().to_string();
        }
        self.saving = true;
        cx.notify();
        match hongguo_service::settings_service::save(&self.state, next) {
            Ok(saved) => {
                window.push_notification(Notification::success("设置已保存"), cx);
                self.draft = None;
                // 归一化后的值回写表单
                let dir = saved.download_dir.clone();
                self.dir_input
                    .update(cx, |input, cx| input.set_value(dir, window, cx));
                let cache_dir = saved.cache_dir.clone();
                self.cache_dir_input
                    .update(cx, |input, cx| input.set_value(cache_dir, window, cx));
                let url = saved.proxy.url.clone();
                self.proxy_input
                    .update(cx, |input, cx| input.set_value(url, window, cx));
                let concurrency = saved.max_concurrency as f32;
                self.concurrency
                    .update(cx, |state, cx| state.set_value(concurrency, window, cx));
                let prefetch = saved.prefetch_episodes as f32;
                self.prefetch
                    .update(cx, |state, cx| state.set_value(prefetch, window, cx));
            }
            Err(e) => {
                log::warn!("[Settings] 保存失败: {e}");
                window.push_notification(Notification::error(format!("保存失败：{e}")), cx);
            }
        }
        self.saving = false;
        cx.notify();
    }

    fn logout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = hongguo_service::login_service::flow::logout(&self.state) {
            log::warn!("[Settings] 退出失败: {e}");
            window.push_notification(Notification::error(format!("退出失败：{e}")), cx);
            return;
        }
        window.push_notification(Notification::success("已退出登录"), cx);
        self.draft = None;
        cx.notify();
    }

    fn confirm_logout(&self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            let view = view.clone();
            dialog
                .title("退出登录？")
                .description("退出后将无法预约新剧与同步观看记录，可随时重新登录。")
                .ok_text("退出登录")
                .cancel_text("取消")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |view, cx| view.logout(window, cx));
                    true
                })
        });
    }

    fn backend_label(&self) -> &'static str {
        match &self.capability {
            None => "加载中…",
            Some(cap) => {
                if cap.platform_hw_encoder || cap.h264_hw_encoder {
                    "硬件加速"
                } else if cap.platform_encoder || cap.has_ffmpeg {
                    "软件加速"
                } else {
                    "标准速度"
                }
            }
        }
    }

    /// 捕获态按键处理（仅捕获中生效；Esc 取消，纯修饰键忽略，
    /// 重复键当场拦截——Tauri ShortcutCard 同口径）。
    fn on_capture_key(
        &mut self,
        ev: &gpui_kit::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(action) = self.capturing else {
            return;
        };
        let key = ev.keystroke.key.to_lowercase();
        if key == "escape" {
            self.capturing = None;
            cx.notify();
            return;
        }
        if matches!(
            key.as_str(),
            "shift" | "control" | "alt" | "meta" | "platform" | "fn" | "capslock"
        ) {
            return;
        }
        if let Some(owner) = ShortcutAction::ALL
            .iter()
            .find(|a| **a != action && a.get(&self.shortcut_keys) == key)
        {
            window.push_notification(
                Notification::error(format!("「{}」已被其他动作占用", shortcut_key_label(&key))),
                cx,
            );
            // 冲突时保留捕获态（换一个键再试），与 Tauri 的「当场拦截」同效
            let _ = owner;
            cx.notify();
            return;
        }
        action.set(&mut self.shortcut_keys, key);
        self.capturing = None;
        self.persist_shortcut_keys(cx);
        cx.emit(SettingsEvent::ShortcutKeys(self.shortcut_keys.clone()));
        cx.notify();
    }

    /// 快捷键落盘（读-改-写：保留播放偏好其余字段不被覆盖）。
    fn persist_shortcut_keys(&self, cx: &Context<Self>) {
        let keys = self.shortcut_keys.clone();
        crate::bridge::spawn(async move {
            tokio::task::spawn_blocking(move || {
                let path = hongguo_store::paths::data_dir().join(crate::playback_prefs::FILE_NAME);
                let mut prefs = crate::playback_prefs::load_from(&path);
                prefs.keys = keys;
                crate::playback_prefs::save_to(&path, &prefs);
            })
            .await
            .expect("阻塞池 join")
        });
        let _ = cx;
    }

    fn render_shortcut_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        card(cx)
            .child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_base()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("播放器快捷键"),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("settings-keys-reset")
                            .label("恢复默认")
                            .ghost()
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.shortcut_keys = crate::playback_prefs::ShortcutKeys::default();
                                this.capturing = None;
                                this.persist_shortcut_keys(cx);
                                cx.emit(SettingsEvent::ShortcutKeys(this.shortcut_keys.clone()));
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("点击按键后按下新键完成设置，Esc 取消。播放中的 Esc 固定用于退出选中。"),
            )
            .children(ShortcutAction::ALL.map(|action| {
                let capturing = self.capturing == Some(action);
                let label = if capturing {
                    "按下新按键…".to_string()
                } else {
                    shortcut_key_label(action.get(&self.shortcut_keys))
                };
                let button = Button::new(format!("settings-key-{}", action.label()))
                    .label(label)
                    .outline()
                    .small();
                let button = if capturing {
                    button.text_color(theme.primary)
                } else {
                    button
                };
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .py_1()
                    .child(div().text_sm().child(action.label()))
                    .child(button.on_click(cx.listener(move |this, _, window, cx| {
                        this.capturing = Some(action);
                        window.focus(&this.keys_focus, cx);
                        cx.notify();
                    })))
            }))
            .into_any_element()
    }

    fn redetected_line(&self) -> Option<String> {
        self.redetect_done
            .then(|| format!("检测完成，当前为「{}」。", self.backend_label()))
    }

    // —— 渲染 ——

    fn render_account_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let account = self.current().account.clone();
        card(cx)
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        gpui_kit::component::Icon::new(gpui_kit::assets::IconName::UserRound)
                            .small()
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .text_base()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("账户"),
                    ),
            )
            .child(match account {
                Some(account) => {
                    let masked = mask_mobile(&account.mobile);
                    let hongguo_id = hongguo_id_of(&account);
                    let display = if account.user_name.is_empty() {
                        account.user_id.clone()
                    } else {
                        account.user_name.clone()
                    };
                    h_flex()
                        .flex_wrap()
                        .items_center()
                        .gap_3()
                        .child(match &self.account_avatar {
                            Some(bitmap) => {
                                gpui_kit::img(gpui_kit::ImageSource::Render(Arc::clone(bitmap)))
                                    .size_8()
                                    .rounded_full()
                                    .into_any_element()
                            }
                            None => div()
                                .size_8()
                                .rounded_full()
                                .bg(theme.secondary)
                                .into_any_element(),
                        })
                        .child(Tag::secondary().child(SharedString::from(display)))
                        .children((!hongguo_id.is_empty()).then(|| {
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(SharedString::from(format!("红果号: {hongguo_id}")))
                        }))
                        .children((!masked.is_empty()).then(|| {
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(SharedString::from(masked))
                        }))
                        .child(div().flex_1())
                        .child(
                            Button::new("settings-logout")
                                .icon(gpui_kit::assets::IconName::LogOut)
                                .label("退出登录")
                                .outline()
                                .small()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.confirm_logout(window, cx);
                                })),
                        )
                        .into_any_element()
                }
                None => h_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("登录后可预约新剧、同步观看记录"),
                    )
                    .child(
                        Button::new("settings-login")
                            .icon(gpui_kit::assets::IconName::LogIn)
                            .label("登录")
                            .small()
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(SettingsEvent::Login);
                            })),
                    )
                    .into_any_element(),
            })
            .into_any_element()
    }

    fn render_dir_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        card(cx)
            .child(
                div()
                    .text_base()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("下载目录"),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.dir_input)))
                    .child(
                        Button::new("settings-pick-dir")
                            .icon(gpui_kit::assets::IconName::FolderOpen)
                            .label("选择目录")
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.pick_dir(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_naming_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let current = self.current();
        card(cx)
            .child(
                div()
                    .text_base()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("文件命名模板"),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(Label::new("文件命名模板"))
                    .child(Select::new(&self.naming_select).w_full()),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(Label::new(format!(
                        "最大并发数 — {}",
                        current.max_concurrency
                    )))
                    .child(Slider::new(&self.concurrency))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("1–10，保存后立即生效"),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(Label::new(format!(
                        "连播预取集数 — {}（0=关闭）",
                        current.prefetch_episodes
                    )))
                    .child(Slider::new(&self.prefetch))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("起播后自动向后预载接下来几集；换剧时同样按此数预载下一部剧"),
                    ),
            )
            .into_any_element()
    }

    fn render_proxy_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let current = self.current();
        card(cx)
            .child(
                div()
                    .text_base()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("网络代理"),
            )
            .child(Select::new(&self.proxy_select).w_full())
            .children(
                (current.proxy.mode == ProxyMode::Manual).then(|| Input::new(&self.proxy_input)),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("settings-test-proxy")
                            .icon(gpui_kit::assets::IconName::Zap)
                            .label(if self.testing_proxy {
                                "测试中…"
                            } else {
                                "测试连接"
                            })
                            .secondary()
                            .small()
                            .disabled(self.testing_proxy)
                            .on_click(cx.listener(|this, _, _, cx| this.test_proxy(cx))),
                    )
                    .children(self.test_result.as_ref().map(|r| {
                        let text = if r.ok {
                            format!("{} ms", r.elapsed_ms)
                        } else {
                            "失败".to_string()
                        };
                        if r.ok {
                            Tag::success().child(SharedString::from(text))
                        } else {
                            Tag::danger()
                                .child(SharedString::from(text))
                                .text_color(theme.danger)
                        }
                    })),
            )
            .into_any_element()
    }

    /// 外观卡：主题三档（跟随系统 / 亮色 / 暗色）。Tauri 把入口放在顶栏，
    /// GPUI 顶栏是平台拖拽区（自绘控件会被命中测试判成拖拽吞掉），入口落
    /// 在设置页；档位语义与 `hongguo-theme` store 相同。
    fn render_appearance_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        use crate::ui_prefs::ThemePref;
        let theme = cx.theme();
        let option = |id: &'static str, label: &'static str, value: ThemePref| {
            let selected = self.theme == value;
            let button = Button::new(id).label(label).small();
            let button = if selected {
                button.secondary()
            } else {
                button.ghost()
            };
            button.on_click(cx.listener(move |this, _, _, cx| this.set_theme(value, cx)))
        };
        card(cx)
            .child(
                div()
                    .text_base()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("外观"),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(option("settings-theme-auto", "跟随系统", ThemePref::Auto))
                    .child(option("settings-theme-light", "亮色", ThemePref::Light))
                    .child(option("settings-theme-dark", "暗色", ThemePref::Dark)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("跟随系统时随 Windows / macOS 的外观设置自动切换。"),
            )
            .into_any_element()
    }

    fn render_playback_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let current = self.current();
        // 安装指引只在「ffmpeg 是兜底决定者且确实没装」时出现：平台编码层
        // 可用的机器分流不会走 ffmpeg，装了也不改道（与 Tauri 设置页同款
        // 条件；探测本身严格按优先级，见 media-platform capability/probe.rs）
        let show_ffmpeg_hint = matches!(
            self.capability.as_ref(),
            Some(cap) if !cap.platform_encoder && !cap.has_ffmpeg
        );
        let capability_tag = match &self.capability {
            Some(cap) if cap.platform_hw_encoder || cap.h264_hw_encoder => {
                Tag::success().child(self.backend_label())
            }
            Some(cap) if cap.platform_encoder || cap.has_ffmpeg => {
                Tag::warning().child(self.backend_label())
            }
            _ => Tag::secondary().child(self.backend_label()),
        };
        card(cx)
            .child(
                div()
                    .text_base()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("播放"),
            )
            .child(toggle_row(
                "settings-auto-next",
                "播完自动下一集",
                current.auto_next_episode,
                cx.listener(|this, checked: &bool, _, cx| {
                    let next = *checked;
                    this.patch(cx, |s| s.auto_next_episode = next);
                }),
            ))
            .child(toggle_row(
                "settings-auto-delete",
                "看完自动删除",
                current.auto_delete_after_play,
                cx.listener(|this, checked: &bool, _, cx| {
                    let next = *checked;
                    this.patch(cx, |s| s.auto_delete_after_play = next);
                }),
            ))
            .child(toggle_row(
                "settings-pause-minimize",
                "最小化时自动暂停",
                current.pause_on_minimize,
                cx.listener(|this, checked: &bool, _, cx| {
                    let next = *checked;
                    this.patch(cx, |s| s.pause_on_minimize = next);
                }),
            ))
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .rounded(gpui_kit::px(8.0))
                    .bg(theme.secondary)
                    .p_3()
                    .text_sm()
                    .child(
                        gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Cpu)
                            .small()
                            .text_color(theme.muted_foreground),
                    )
                    .child("转换方式")
                    .child(div().flex_1())
                    .child(capability_tag)
                    .child(
                        Button::new("settings-redetect")
                            .label(if self.redetecting {
                                "加载中…"
                            } else {
                                "重新检测"
                            })
                            .ghost()
                            .small()
                            .disabled(self.redetecting)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.redetect_capability(cx);
                            })),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("兼容合并会把视频转成通用格式。优先走系统自带编码：Mac 上 VideoToolbox 有硬编用硬编，没硬编用系统软编；Windows 用 Media Foundation 硬编或 ffmpeg。都没有才落到纯软解（慢但够用）。徽章按实测分档：真能走显卡才显示「硬件加速」。装完 ffmpeg 若没变化，点「重新检测」。"),
            )
            .children(self.redetected_line().map(|line| {
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(line))
            }))
            .children(show_ffmpeg_hint.then(|| ffmpeg_install_block(cx)))
            .into_any_element()
    }

    fn render_storage_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let usage_line = match &self.usage {
            Some(usage) => format!(
                "下载视频 {} · {} 个文件",
                format_bytes(usage.bytes),
                usage.files
            ),
            None => "加载中…".to_string(),
        };
        let cache_line = match &self.cache_usage {
            Some(cache_usage) => format!(
                "缓存占用 {}（转码 {} · 封面 {} · 内存流 {} 项）",
                format_bytes(cache_usage.compat.bytes + cache_usage.cover.bytes),
                format_bytes(cache_usage.compat.bytes),
                format_bytes(cache_usage.cover.bytes),
                cache_usage.online.files
            ),
            None => "加载中…".to_string(),
        };
        card(cx)
            .child(
                div()
                    .text_base()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("磁盘占用"),
            )
            // 缓存目录：空 = 跟随数据目录。保存即时生效于新写入，旧目录残留
            // 不迁移——与下载目录同语义（Tauri 设置页存储卡同款位置同款文案）
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(Label::new("缓存目录"))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&self.cache_dir_input)))
                            .child(
                                Button::new("settings-pick-cache-dir")
                                    .icon(gpui_kit::assets::IconName::FolderOpen)
                                    .label("选择目录")
                                    .outline()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.pick_cache_dir(window, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("转码与封面缓存的存放位置，留空跟随数据目录。保存后新缓存写入新目录；已生成的旧缓存不迁移，可在原目录手动清理。"),
                    ),
            )
            .child(div().text_sm().child(SharedString::from(usage_line)))
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().child(SharedString::from(cache_line)))
                    .child(div().flex_1())
                    .child(
                        Button::new("settings-clear-cache")
                            .label(if self.clearing_cache {
                                "加载中…"
                            } else {
                                "清理缓存"
                            })
                            .outline()
                            .small()
                            .disabled(self.clearing_cache)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clear_cache(cx);
                            })),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("磁盘清理（按剧删除本地文件）在左侧「磁盘清理」页。"),
            )
            .into_any_element()
    }

    fn render_update_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        card(cx)
            .child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_base()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("红果播放器"),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("settings-check-update")
                            .label("检查更新")
                            .outline()
                            .small()
                            .disabled(true),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(format!(
                        "版本 {} · GPUI 壳暂未接入更新通道（Tauri 壳内可用）",
                        env!("CARGO_PKG_VERSION")
                    ))),
            )
            .into_any_element()
    }
}

impl gpui_kit::EventEmitter<SettingsEvent> for SettingsView {}

impl SettingsView {
    /// 预览种子（screenshot feature）：灌假占用/缓存/能力与表单值。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(
        &mut self,
        scene: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use hongguo_service::storage_service::cache::CachePart;
        self.loaded = true;
        self.loading = false;
        self.usage = Some(StorageUsage {
            bytes: 2_442_000_000,
            files: 7,
        });
        self.cache_usage = Some(CacheUsage {
            online: CachePart { files: 3, bytes: 0 },
            compat: CachePart {
                files: 2,
                bytes: 736_000_000,
            },
            cover: CachePart {
                files: 46,
                bytes: 12_400_000,
            },
        });
        let mut capability = DecodeCapability::default();
        // 半硬件一档（macOS Apple 软编会话）：徽标显示「软件加速」
        capability.platform_encoder = true;
        capability.has_ffmpeg = scene != "settings-no-ffmpeg";
        self.capability = Some(capability);
        let settings = self.state.settings();
        let dir = settings.download_dir.clone();
        self.dir_input
            .update(cx, |input, cx| input.set_value(dir, window, cx));
        let cache_dir = settings.cache_dir.clone();
        self.cache_dir_input
            .update(cx, |input, cx| input.set_value(cache_dir, window, cx));
        let url = settings.proxy.url.clone();
        self.proxy_input
            .update(cx, |input, cx| input.set_value(url, window, cx));
        self.naming_select.update(cx, |select, cx| {
            select.set_selected_index(Some(gpui_kit::component::IndexPath::new(0)), window, cx)
        });
        self.proxy_select.update(cx, |select, cx| {
            select.set_selected_index(Some(gpui_kit::component::IndexPath::new(1)), window, cx)
        });
        // 与「手动指定」选中态一致地进草稿：URL 输入框随模式展示
        let mut draft = settings.clone();
        draft.proxy.mode = ProxyMode::Manual;
        self.draft = Some(draft);
        self.concurrency.update(cx, |state, cx| {
            state.set_value(settings.max_concurrency as f32, window, cx)
        });
        cx.notify();
    }
}

impl gpui_kit::Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if !self.loaded {
            h_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(Spinner::new().small())
                .into_any_element()
        } else {
            v_flex()
                .max_w(gpui_kit::px(768.0))
                .mx_auto()
                .w_full()
                .p_6()
                .gap_4()
                .child(self.render_account_card(cx))
                .child(self.render_appearance_card(cx))
                .child(self.render_dir_card(cx))
                .child(self.render_naming_card(cx))
                .child(self.render_proxy_card(cx))
                .child(separator(cx))
                .child(self.render_playback_card(cx))
                .child(self.render_shortcut_card(cx))
                .child(self.render_storage_card(cx))
                .child(self.render_update_card(cx))
                .child(
                    h_flex().w_full().justify_end().child(
                        Button::new("settings-save")
                            .label("保存")
                            .primary()
                            .loading(self.saving)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save(window, cx);
                            })),
                    ),
                )
                .into_any_element()
        };
        let theme = cx.theme();
        div()
            .id("settings-page")
            .size_full()
            .overflow_y_scroll()
            .bg(theme.background)
            .text_color(theme.foreground)
            // 捕获态键盘归属：焦点锚在根元素上，捕获中接管按键
            .track_focus(&self.keys_focus)
            .key_context("SettingsKeys")
            .on_key_down(
                cx.listener(|this, ev: &gpui_kit::KeyDownEvent, window, cx| {
                    if this.capturing.is_some() {
                        this.on_capture_key(ev, window, cx);
                    }
                }),
            )
            .child(body)
    }
}

/// 开关行：Switch 与文字同排（Tauri ToggleRow 同款）。
fn toggle_row(
    id: &'static str,
    label: &'static str,
    checked: bool,
    on_click: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> gpui_kit::AnyElement {
    h_flex()
        .items_center()
        .gap_3()
        .child(Switch::new(id).checked(checked).on_click(on_click))
        .child(div().text_sm().child(label))
        .into_any_element()
}

fn separator(cx: &App) -> gpui_kit::AnyElement {
    div()
        .w_full()
        .h(gpui_kit::px(1.0))
        .bg(cx.theme().border)
        .into_any_element()
}

/// ffmpeg 安装指引块（未装 ffmpeg 时出现；命令按平台）。
fn ffmpeg_install_block(cx: &mut Context<SettingsView>) -> gpui_kit::AnyElement {
    let theme = cx.theme();
    #[cfg(target_os = "macos")]
    let (cmd, page): (&str, &str) = ("brew install ffmpeg", "https://ffmpeg.org/download.html");
    #[cfg(not(target_os = "macos"))]
    let (cmd, page): (&str, &str) = (
        "winget install Gyan.FFmpeg",
        "https://www.gyan.dev/ffmpeg/builds/",
    );
    v_flex()
        .w_full()
        .gap_2()
        .rounded(gpui_kit::px(8.0))
        .bg(theme.secondary)
        .p_3()
        .text_xs()
        .child(div().child("想让兼容合并提速可以装 ffmpeg（免费开源）：装上后转码自动改走它，有 N/A/Intel 显卡时还能用硬件编码。不装也完全能用，只是转码慢一些。"))
        .child(
            h_flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .rounded(gpui_kit::px(4.0))
                        .bg(theme.background)
                        .px_2()
                        .py_1()
                        .font_family(theme.mono_font_family.clone())
                        .child(cmd),
                )
                .child(
                    Button::new("settings-copy-ffmpeg")
                        .icon(gpui_kit::assets::IconName::Copy)
                        .label("复制命令")
                        .ghost()
                        .small()
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(cmd.to_string()));
                        }),
                )
                .child(
                    Button::new("settings-open-ffmpeg")
                        .icon(gpui_kit::assets::IconName::ExternalLink)
                        .label("打开下载页")
                        .ghost()
                        .small()
                        .on_click(move |_, _, _| open_external(page)),
                ),
        )
        .child(div().child("装完不用重启应用，回到上面点「重新检测」即可生效。"))
        .into_any_element()
}

/// 用系统默认方式打开外部页面（Tauri openExternalPage 的 GPUI 对应；
/// 调用点固定为设置页的白名单地址）。
fn open_external(url: &str) {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = {
        use std::os::windows::process::CommandExt;
        // cmd.exe 是控制台程序：GUI 进程里不压 CREATE_NO_WINDOW 会闪黑窗
        std::process::Command::new("cmd")
            .raw_arg(format!("/c start \"\" \"{url}\""))
            .creation_flags(0x0800_0000)
            .spawn()
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result: std::io::Result<std::process::Child> = {
        log::warn!("[Settings] 打开外部页面在 {url} 所在平台未实现");
        return;
    };
    if let Err(e) = result {
        log::warn!("[Settings] 打开外部页面失败: {e}");
    }
}

/// 手机号打码（138****1234；长度不足原样返回）。
fn mask_mobile(mobile: &str) -> String {
    if mobile.chars().count() >= 11 {
        let head: String = mobile.chars().take(3).collect();
        let tail: String = mobile.chars().skip(7).collect();
        format!("{head}****{tail}")
    } else {
        mobile.to_string()
    }
}

/// 从 user_info 原文提取红果号（biz_user_id；缺失返回空串）。
fn hongguo_id_of(account: &AccountState) -> String {
    let raw = account.raw_profile.as_str();
    if raw.is_empty() {
        return String::new();
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return String::new();
    };
    value
        .get("data")
        .and_then(|d| d.get("biz_user_id"))
        .map(|id| match id {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}
