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
pub(crate) enum ShortcutAction {
    PlayPause,
    SeekBack,
    SeekForward,
    StepPrev,
    StepNext,
}

impl ShortcutAction {
    pub(crate) const ALL: [ShortcutAction; 5] = [
        ShortcutAction::PlayPause,
        ShortcutAction::SeekBack,
        ShortcutAction::SeekForward,
        ShortcutAction::StepPrev,
        ShortcutAction::StepNext,
    ];

    pub(crate) fn label(self) -> String {
        match self {
            ShortcutAction::PlayPause => crate::i18n::t("settings.keyPlayPause"),
            ShortcutAction::SeekBack => crate::i18n::t("settings.keySeekBack"),
            ShortcutAction::SeekForward => crate::i18n::t("settings.keySeekForward"),
            ShortcutAction::StepPrev => crate::i18n::t("settings.keyStepPrev"),
            ShortcutAction::StepNext => crate::i18n::t("settings.keyStepNext"),
        }
    }

    pub(crate) fn get(self, keys: &crate::playback_prefs::ShortcutKeys) -> &str {
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

/// 捕获态目标：播放器动作键 / 老板键组合（两者都 Esc 取消；纯修饰键忽略）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureTarget {
    Shortcut(ShortcutAction),
    BossKey,
}

/// GPUI keystroke 键名 → 可读标签（Tauri shortcutKeyLabel 同款）。
fn shortcut_key_label(key: &str) -> String {
    match key {
        "space" => crate::i18n::t("settings.keySpace"),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "escape" => "Esc".to_string(),
        other if other.chars().count() == 1 => other.to_uppercase(),
        other => other.to_string(),
    }
}

fn naming_labels() -> [String; 3] {
    [
        crate::i18n::t("settings.namingTitleIndex"),
        crate::i18n::t("settings.namingTitleIndexEpisode"),
        crate::i18n::t("settings.namingOnlyTitle"),
    ]
}

fn proxy_labels() -> [String; 3] {
    [
        crate::i18n::t("settings.proxySystem"),
        crate::i18n::t("settings.proxyManual"),
        crate::i18n::t("settings.proxyDirect"),
    ]
}

fn naming_items() -> SearchableVec<SharedString> {
    SearchableVec::new(
        naming_labels()
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<SharedString>>(),
    )
}

fn proxy_items() -> SearchableVec<SharedString> {
    SearchableVec::new(
        proxy_labels()
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<SharedString>>(),
    )
}

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
    /// 缓存目录留空时的默认位置（数据目录；进程内不变，构造时取一次）。
    default_cache_dir: String,
    clearing_cache: bool,
    testing_proxy: bool,
    test_result: Option<ProxyTestResult>,
    saving: bool,
    account_avatar: Option<Arc<RenderImage>>,
    /// 快捷键映射（设置页与播放器共用一份；改动即存即生效）。
    shortcut_keys: crate::playback_prefs::ShortcutKeys,
    /// 正在捕获的按键目标（Esc 取消；纯修饰键忽略）。
    capturing: Option<CaptureTarget>,
    /// 捕获焦点锚：进入捕获态后由根元素接管键盘。
    keys_focus: gpui_kit::FocusHandle,
    /// 主题偏好（Tauri 的主题切换在顶栏；GPUI 顶栏是原生拖拽区，入口落在
    /// 设置页——三档语义与 Tauri `hongguo-theme` store 相同）。
    theme: crate::ui_prefs::ThemePref,
}

/// 缓存目录输入框的展示值：未配置时直接填默认位置（存储语义仍是空 = 跟随数据目录）。
fn cache_dir_display(default_cache_dir: &str, cache_dir: &str) -> String {
    if cache_dir.trim().is_empty() {
        default_cache_dir.to_string()
    } else {
        cache_dir.to_string()
    }
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
        let cache_dir_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::i18n::t("settings.cacheDirPlaceholder"))
        });
        let proxy_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("http://127.0.0.1:7890"));
        let naming_select = cx.new(|cx| SelectState::new(naming_items(), None, window, cx));
        let proxy_select = cx.new(|cx| SelectState::new(proxy_items(), None, window, cx));
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
                .max(10.)
                .step(1.)
                .default_value(3.)
        });
        cx.subscribe_in(
            &naming_select,
            window,
            |this, _, event: &SelectEvent<SearchableVec<SharedString>>, _, cx| {
                if let SelectEvent::Confirm(Some(value)) = event
                    && let Some(ix) = naming_labels().iter().position(|l| l == value.as_str())
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
                    && let Some(ix) = proxy_labels().iter().position(|l| l == value.as_str())
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
            let n = value.start().round().clamp(0.0, 10.0) as u32;
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
            default_cache_dir: hongguo_service::settings_service::default_cache_dir(),
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

    /// 语言切换后刷新有状态的静态标签：两个 Select 的项文本与缓存目录
    /// placeholder。输入框当前值原样保留，不把用户草稿切丢。
    pub(crate) fn refresh_locale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.current();
        let naming_value = naming_labels()[naming_index(current.naming)].clone();
        self.naming_select.update(cx, |select, cx| {
            select.set_items(naming_items(), window, cx);
            select.set_selected_value(&SharedString::from(naming_value), window, cx);
        });

        let proxy_value = proxy_labels()[proxy_index(current.proxy.mode)].clone();
        self.proxy_select.update(cx, |select, cx| {
            select.set_items(proxy_items(), window, cx);
            select.set_selected_value(&SharedString::from(proxy_value), window, cx);
        });

        self.cache_dir_input.update(cx, |input, cx| {
            input.set_placeholder(crate::i18n::t("settings.cacheDirPlaceholder"), window, cx);
        });
        cx.notify();
    }

    /// 切换主题：立即应用 + 落盘 + 通知壳（auto 观察者换判据）。
    pub(crate) fn set_theme(&mut self, pref: crate::ui_prefs::ThemePref, cx: &mut Context<Self>) {
        use crate::ui_prefs::ThemePref;
        self.theme = pref;
        let mut prefs = crate::ui_prefs::load();
        prefs.theme = pref;
        crate::ui_prefs::save(prefs);
        let mode = pref.resolve(cx.window_appearance());
        crate::tokens::apply(mode, cx);
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
                let cache_dir = cache_dir_display(&view.default_cache_dir, &settings.cache_dir);
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
            prompt: Some(crate::i18n::t("settings.chooseDownloadDir").into()),
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
            prompt: Some(crate::i18n::t("settings.chooseCacheDir").into()),
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
                                let elapsed = r.elapsed_ms.to_string();
                                Notification::success(crate::i18n::tf(
                                    "settings.testOk",
                                    &[("ms", elapsed.as_str())],
                                ))
                            } else {
                                Notification::error(crate::i18n::tf(
                                    "settings.testFail",
                                    &[("message", r.message.as_str())],
                                ))
                            },
                            cx,
                        );
                        view.test_result = Some(r);
                    }
                    Err(e) => {
                        log::warn!("[Settings] 代理测试失败: {e}");
                        let message = crate::i18n::app_error_text(&e);
                        window.push_notification(
                            Notification::error(crate::i18n::tf(
                                "settings.testFail",
                                &[("message", message.as_str())],
                            )),
                            cx,
                        );
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
                let items = result.items.to_string();
                let bytes = format_bytes(result.bytes);
                window.push_notification(
                    Notification::success(crate::i18n::tf(
                        "settings.cacheCleared",
                        &[("count", items.as_str()), ("size", bytes.as_str())],
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
        // 老板键冲突拦截（Tauri submit 同款：全局组合在应用内会同时触发
        // 播放器动作，存了就是坏状态）
        let conflicts = crate::boss_key::conflicts(&next.boss_key, &self.shortcut_keys);
        if !conflicts.is_empty() {
            window.push_notification(
                Notification::error(crate::i18n::tf(
                    "settings.bossKeyConflict",
                    &[("actions", conflicts.join("、").as_str())],
                )),
                cx,
            );
            return;
        }
        // 老板键先注册后落库（Tauri save_settings 同序：注册失败整个保存不
        // 提交——先落库的话错误返回了坏键却已入库，重启后照样注册不上）
        if let Err(e) = crate::boss_key::apply(&next.boss_key) {
            log::warn!("[BossKey] {e}");
            let message = e.to_string();
            window.push_notification(
                Notification::error(crate::i18n::tf(
                    "settings.saveFailed",
                    &[("message", message.as_str())],
                )),
                cx,
            );
            return;
        }
        self.saving = true;
        cx.notify();
        match hongguo_service::settings_service::save(&self.state, next) {
            Ok(saved) => {
                window
                    .push_notification(Notification::success(crate::i18n::t("settings.saved")), cx);
                self.draft = None;
                // 归一化后的值回写表单
                let dir = saved.download_dir.clone();
                self.dir_input
                    .update(cx, |input, cx| input.set_value(dir, window, cx));
                let cache_dir = cache_dir_display(&self.default_cache_dir, &saved.cache_dir);
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
                let message = crate::i18n::app_error_text(&e);
                window.push_notification(
                    Notification::error(crate::i18n::tf(
                        "settings.saveFailed",
                        &[("message", message.as_str())],
                    )),
                    cx,
                );
            }
        }
        self.saving = false;
        cx.notify();
    }

    fn logout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = hongguo_service::login_service::flow::logout(&self.state) {
            log::warn!("[Settings] 退出失败: {e}");
            let message = crate::i18n::app_error_text(&e);
            window.push_notification(
                Notification::error(crate::i18n::tf(
                    "settings.logoutFailed",
                    &[("message", message.as_str())],
                )),
                cx,
            );
            return;
        }
        window.push_notification(
            Notification::success(crate::i18n::t("settings.loggedOut")),
            cx,
        );
        self.draft = None;
        cx.notify();
    }

    fn confirm_logout(&self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            let view = view.clone();
            dialog
                .title(crate::i18n::t("nav.logoutConfirmTitle"))
                .description(crate::i18n::t("nav.logoutConfirmBody"))
                .ok_text(crate::i18n::t("settings.logout"))
                .cancel_text(crate::i18n::t("common.cancel"))
                // 组件库取消钮默认不显示：不给 show_cancel，确认框就没有
                // 取消键（Tauri 确认框是「取消 + 确认」两键）
                .show_cancel(true)
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |view, cx| view.logout(window, cx));
                    true
                })
        });
    }

    fn backend_label(&self) -> String {
        match &self.capability {
            None => crate::i18n::t("common.loading"),
            Some(cap) => {
                if cap.platform_hw_encoder || cap.h264_hw_encoder {
                    crate::i18n::t("settings.backendFfmpegHw")
                } else if cap.platform_encoder || cap.has_ffmpeg {
                    crate::i18n::t("settings.backendFfmpegSw")
                } else {
                    crate::i18n::t("settings.backendRust")
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
        let Some(target) = self.capturing else {
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
        // 老板键：全局组合必须带 Ctrl/Cmd 或 Alt（Tauri acceleratorFromEvent
        // 同口径），值写进设置草稿（保存时先注册后落库）。
        if target == CaptureTarget::BossKey {
            let mods = &ev.keystroke.modifiers;
            let Some(spec) = crate::boss_key::accel::accelerator_from_keystroke(
                &key,
                mods.control || mods.platform,
                mods.alt,
                mods.shift,
            ) else {
                window.push_notification(
                    Notification::error(crate::i18n::t("settings.bossKeyUnsupported")),
                    cx,
                );
                return; // 保持捕获态，换一个再试
            };
            self.patch(cx, |s| s.boss_key = spec);
            self.capturing = None;
            cx.notify();
            return;
        }
        let CaptureTarget::Shortcut(action) = target else {
            return;
        };
        if let Some(owner) = ShortcutAction::ALL
            .iter()
            .find(|a| **a != action && a.get(&self.shortcut_keys) == key)
        {
            window.push_notification(
                Notification::error(crate::i18n::tf(
                    "settings.keyConflict",
                    &[("key", shortcut_key_label(&key).as_str())],
                )),
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

    /// 老板键卡（Tauri `BossKeyCard` 同款）：全局快捷键隐藏/恢复整个窗口。
    /// 值进设置草稿，保存时先注册后落库；冲突（末段裸键撞播放器键）当场提示
    /// 并在保存时拦截。Tauri 侧也没有「清除」入口，这里不另造。
    fn render_boss_key_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let boss = self.current().boss_key;
        let capturing = self.capturing == Some(CaptureTarget::BossKey);
        let display = if boss.trim().is_empty() {
            crate::i18n::t("settings.bossKeyOff")
        } else {
            crate::boss_key::accel::display(&boss)
        };
        let conflicts = crate::boss_key::conflicts(&boss, &self.shortcut_keys);
        let button = Button::new("settings-boss-key")
            .label(if capturing {
                crate::i18n::t("settings.bossKeyCapturing")
            } else {
                display.clone()
            })
            .outline()
            .small();
        let button = if capturing {
            button.text_color(theme.primary)
        } else {
            button
        };
        card(cx)
            .child(
                div()
                    .text_base()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(crate::i18n::t("settings.bossKeyCardTitle")),
            )
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .child(div().text_sm().child(crate::i18n::t("settings.bossKey")))
                    .child(button.on_click(cx.listener(|this, _, window, cx| {
                        this.capturing = Some(CaptureTarget::BossKey);
                        window.focus(&this.keys_focus, cx);
                        cx.notify();
                    }))),
            )
            .children((!conflicts.is_empty()).then(|| {
                div()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(SharedString::from(crate::i18n::tf(
                        "settings.bossKeyConflict",
                        &[("actions", conflicts.join("、").as_str())],
                    )))
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(crate::i18n::t("settings.bossKeyDesc")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(crate::i18n::t("settings.bossKeyGlobalHint")),
            )
            .into_any_element()
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
                            .child(crate::i18n::t("settings.shortcuts")),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("settings-keys-reset")
                            .label(crate::i18n::t("settings.resetKeys"))
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
                    .child(crate::i18n::t("settings.shortcutsHint")),
            )
            .children(ShortcutAction::ALL.map(|action| {
                let capturing = self.capturing == Some(CaptureTarget::Shortcut(action));
                let label = if capturing {
                    crate::i18n::t("settings.keyCapturing")
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
                        this.capturing = Some(CaptureTarget::Shortcut(action));
                        window.focus(&this.keys_focus, cx);
                        cx.notify();
                    })))
            }))
            .into_any_element()
    }

    fn redetected_line(&self) -> Option<String> {
        self.redetect_done.then(|| {
            crate::i18n::tf(
                "settings.redetectDone",
                &[("backend", self.backend_label().as_str())],
            )
        })
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
                            .child(crate::i18n::t("settings.account")),
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
                            div().text_xs().text_color(theme.muted_foreground).child(
                                SharedString::from(format!(
                                    "{}: {hongguo_id}",
                                    crate::i18n::t("settings.hongguoId")
                                )),
                            )
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
                                .label(crate::i18n::t("settings.logout"))
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
                            .child(crate::i18n::t("settings.loginPrompt")),
                    )
                    .child(
                        Button::new("settings-login")
                            .icon(gpui_kit::assets::IconName::LogIn)
                            .label(crate::i18n::t("settings.loginAction"))
                            .small()
                            // Tauri 账户卡的登录是默认 primary 实心钮（深色主题下白底）
                            .primary()
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
                    .child(crate::i18n::t("settings.downloadDir")),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.dir_input)))
                    .child(
                        Button::new("settings-pick-dir")
                            .icon(gpui_kit::assets::IconName::FolderOpen)
                            .label(crate::i18n::t("settings.choose"))
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
                    .child(crate::i18n::t("settings.naming")),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(Label::new(crate::i18n::t("settings.naming")))
                    .child(Select::new(&self.naming_select).w_full()),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(Label::new(format!(
                        "{} — {}",
                        crate::i18n::t("settings.concurrency"),
                        current.max_concurrency
                    )))
                    .child(Slider::new(&self.concurrency))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("settings.concurrencyDesc")),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(Label::new(format!(
                        "{} — {}",
                        crate::i18n::t("settings.prefetchEpisodes"),
                        current.prefetch_episodes
                    )))
                    .child(Slider::new(&self.prefetch))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("settings.prefetchEpisodesDesc")),
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
                    .child(crate::i18n::t("settings.proxy")),
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
                                crate::i18n::t("settings.testing")
                            } else {
                                crate::i18n::t("settings.testProxy")
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
                            crate::i18n::t("common.failed")
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
                    .child(crate::i18n::t("settings.playback")),
            )
            .child(toggle_row(
                "settings-auto-next",
                crate::i18n::t("settings.autoNext"),
                current.auto_next_episode,
                cx.listener(|this, checked: &bool, _, cx| {
                    let next = *checked;
                    this.patch(cx, |s| s.auto_next_episode = next);
                }),
            ))
            .child(toggle_row(
                "settings-auto-delete",
                crate::i18n::t("settings.autoDelete"),
                current.auto_delete_after_play,
                cx.listener(|this, checked: &bool, _, cx| {
                    let next = *checked;
                    this.patch(cx, |s| s.auto_delete_after_play = next);
                }),
            ))
            .child(toggle_row(
                "settings-pause-minimize",
                crate::i18n::t("settings.pauseOnMinimize"),
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
                    .child(crate::i18n::t("settings.transcodeBackend"))
                    .child(div().flex_1())
                    .child(capability_tag)
                    .child(
                        Button::new("settings-redetect")
                            .label(if self.redetecting {
                                crate::i18n::t("common.loading")
                            } else {
                                crate::i18n::t("settings.redetectBackend")
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
                    .child(crate::i18n::t("settings.transcodeBackendHint")),
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
            Some(usage) => {
                let size = format_bytes(usage.bytes);
                let files = usage.files.to_string();
                crate::i18n::tf(
                    "settings.storageUsage",
                    &[("size", size.as_str()), ("files", files.as_str())],
                )
            }
            None => crate::i18n::t("common.loading"),
        };
        let cache_line = match &self.cache_usage {
            Some(cache_usage) => {
                let size = format_bytes(cache_usage.compat.bytes + cache_usage.cover.bytes);
                let compat = format_bytes(cache_usage.compat.bytes);
                let cover = format_bytes(cache_usage.cover.bytes);
                let online = cache_usage.online.files.to_string();
                crate::i18n::tf(
                    "settings.cacheUsage",
                    &[
                        ("size", size.as_str()),
                        ("compat", compat.as_str()),
                        ("cover", cover.as_str()),
                        ("online", online.as_str()),
                    ],
                )
            }
            None => crate::i18n::t("common.loading"),
        };
        // 缓存目录：未配置时输入框直接填默认位置（空存储 = 跟随数据目录）。
        // 保存即时生效于新写入，旧目录残留不迁移——与下载目录同语义。
        card(cx)
            .child(
                div()
                    .text_base()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(crate::i18n::t("settings.storage")),
            )
            // 缓存目录：空 = 跟随数据目录。保存即时生效于新写入，旧目录残留
            // 不迁移——与下载目录同语义（Tauri 设置页存储卡同款位置同款文案）
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(Label::new(crate::i18n::t("settings.cacheDir")))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&self.cache_dir_input)))
                            .child(
                                Button::new("settings-pick-cache-dir")
                                    .icon(gpui_kit::assets::IconName::FolderOpen)
                                    .label(crate::i18n::t("settings.choose"))
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
                            .child(crate::i18n::t("settings.cacheDirDesc")),
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
                                crate::i18n::t("common.loading")
                            } else {
                                crate::i18n::t("settings.clearCache")
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
                    .child(crate::i18n::t("settings.diskCleanupHint")),
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
                            .child(crate::i18n::t("update.appTitle")),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("settings-check-update")
                            .label(crate::i18n::t("update.checkUpdate"))
                            .outline()
                            .small()
                            .disabled(true),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(crate::i18n::tf(
                        "settings.gpuiUpdateUnavailable",
                        &[("version", env!("CARGO_PKG_VERSION"))],
                    ))),
            )
            .into_any_element()
    }
}

impl gpui_kit::EventEmitter<SettingsEvent> for SettingsView {}

impl SettingsView {
    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=boss-set`，配
    /// `HONGGUO_BOSS_KEY=<accelerator>`）：走**真实保存路径**写老板键
    /// （先注册后落库 + 冲突拦截都在 save 里），保存结果只落日志。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_set_boss_key(
        &mut self,
        spec: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.patch(cx, |s| s.boss_key = spec.to_string());
        self.save(window, cx);
        let saved = self.state.settings().boss_key;
        log::info!("[BossProbe] 写入 {spec:?} 后，库里的老板键 = {saved:?}");
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=auto-delete`）：
    /// 走真实保存路径打开「看完自动删除」——验播放到底时真删本地文件。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_set_auto_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.patch(cx, |s| s.auto_delete_after_play = true);
        self.save(window, cx);
        let saved = self.state.settings().auto_delete_after_play;
        log::info!("[AutoDelete] 保存后 库里的看完自动删 = {saved}");
    }

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
        let capability = DecodeCapability {
            // 半硬件一档（macOS Apple 软编会话）：徽标显示「软件加速」
            platform_encoder: true,
            has_ffmpeg: scene != "settings-no-ffmpeg",
            ..Default::default()
        };
        self.capability = Some(capability);
        let settings = self.state.settings();
        let dir = settings.download_dir.clone();
        self.dir_input
            .update(cx, |input, cx| input.set_value(dir, window, cx));
        let cache_dir = cache_dir_display(&self.default_cache_dir, &settings.cache_dir);
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
                .child(self.render_dir_card(cx))
                .child(self.render_naming_card(cx))
                .child(self.render_proxy_card(cx))
                .child(separator(cx))
                .child(self.render_playback_card(cx))
                .child(self.render_shortcut_card(cx))
                .child(self.render_boss_key_card(cx))
                .child(self.render_storage_card(cx))
                .child(self.render_update_card(cx))
                .child(
                    h_flex().w_full().justify_end().child(
                        Button::new("settings-save")
                            .label(crate::i18n::t("settings.save"))
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
    label: impl Into<SharedString>,
    checked: bool,
    on_click: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> gpui_kit::AnyElement {
    let label: SharedString = label.into();
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
        .child(div().child(crate::i18n::t("settings.ffmpegInstallHint")))
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
                        .label(crate::i18n::t("settings.copyCommand"))
                        .ghost()
                        .small()
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(cmd.to_string()));
                        }),
                )
                .child(
                    Button::new("settings-open-ffmpeg")
                        .icon(gpui_kit::assets::IconName::ExternalLink)
                        .label(crate::i18n::t("settings.ffmpegDownloadPage"))
                        .ghost()
                        .small()
                        .on_click(move |_, _, _| open_external(page)),
                ),
        )
        .child(div().child(crate::i18n::t("settings.ffmpegInstallNote")))
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
