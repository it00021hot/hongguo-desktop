//! 设置页（Tauri `/settings` = `settings-page.tsx` 的 GPUI 对齐）。
//!
//! 卡片：账户（登录态/退出）/ 下载目录（可选目录）/ 命名与并发 / 网络代理
//! （含测试连接）/ 播放（三开关 + 转码后端探测 + ffmpeg 指引）/ 存储
//! （缓存目录 + 占用与缓存清理）/ 更新（版本展示）/ 保存。草稿语义同
//! Tauri：改过才进草稿，保存成功丢草稿回「跟随服务端」。保存与探测走
//! service（`settings_service::save`、`storage_service`、`capability::probe`），
//! 与 Tauri 的 settings/transcode command 同一段。

use std::sync::Arc;

use gpui_kit::component::input::InputState;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::prelude::*;
use gpui_kit::{Entity, RenderImage, SharedString, Window};
use hongguo_common::model::{ProxyTestResult, Settings};
use hongguo_media_platform::capability::probe::DecodeCapability;
use hongguo_service::app_state::AppStateInner;
use hongguo_service::storage_service::cache::CacheUsage;
use hongguo_service::storage_service::usage::StorageUsage;

use general::{
    naming_from_index, naming_index, naming_items, naming_labels, proxy_from_index, proxy_index,
    proxy_items, proxy_labels,
};

mod account;
mod general;
mod load;
mod network;
mod playback;
mod save;
mod shortcuts;
mod storage;
mod transcode;
mod view;

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

pub(crate) struct SettingsView {
    state: Arc<AppStateInner>,
    loaded: bool,
    loading: bool,
    /// 草稿（None = 未修改，跟随服务端值）。
    draft: Option<Settings>,
    dir_input: Entity<InputState>,
    cache_dir_input: Entity<InputState>,
    /// 缓存占用上限输入（MB）。
    cache_limit_input: Entity<InputState>,
    /// 缓存自动清理周期下拉（天；关闭/1/3/7/30）。
    cache_ttl_select: Entity<SelectState<SearchableVec<SharedString>>>,
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
        let cache_limit_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(crate::i18n::t("settings.cacheLimitPlaceholder"))
        });
        let cache_ttl_select =
            cx.new(|cx| SelectState::new(storage::cache_ttl_items(), None, window, cx));
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
            &cache_ttl_select,
            window,
            |this, _, event: &SelectEvent<SearchableVec<SharedString>>, _, cx| {
                if let SelectEvent::Confirm(Some(value)) = event
                    && let Some(ix) = storage::cache_ttl_labels()
                        .iter()
                        .position(|l| l == value.as_str())
                {
                    this.patch(cx, |s| s.cache_ttl_days = storage::cache_ttl_from_index(ix));
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
            cache_limit_input,
            cache_ttl_select,
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

    fn patch(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Settings)) {
        let mut current = self.current();
        f(&mut current);
        self.draft = Some(current);
        cx.notify();
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
}

impl gpui_kit::EventEmitter<SettingsEvent> for SettingsView {}
