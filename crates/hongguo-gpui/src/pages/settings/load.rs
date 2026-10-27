//! 设置页加载：进页拉占用/缓存/能力探测并回填表单；另含截图预览种子。
//!
//! 从 `settings.rs` 按域拆出，纯移动零行为变更。

use super::*;

use super::general::{naming_index, proxy_index};
use super::storage::cache_dir_display;
use crate::bridge;
use hongguo_common::AppError;
#[cfg(feature = "screenshot")]
use hongguo_common::model::settings::ProxyMode;
#[cfg(feature = "screenshot")]
use hongguo_media_platform::capability::probe::DecodeCapability;
#[cfg(feature = "screenshot")]
use hongguo_service::storage_service::cache::CacheUsage;
#[cfg(feature = "screenshot")]
use hongguo_service::storage_service::usage::StorageUsage;

impl super::SettingsView {
    /// 进入页面时拉占用/缓存/能力探测（探测首次 1–2 秒，走阻塞池）。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        if self.loaded || self.loading {
            return;
        }
        self.loading = true;
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let probed = tokio::task::spawn_blocking(move || {
                let usage = hongguo_service::storage_service::usage::usage(&state);
                let cache_usage = hongguo_service::storage_service::cache::usage();
                let capability = hongguo_media_platform::capability::probe::detect();
                (usage, cache_usage, capability)
            })
            .await
            .map_err(|e| AppError::Internal(format!("占用与能力探测阻塞池任务失败: {e}")))?;
            Ok::<_, AppError>(probed)
        });
        cx.spawn(async move |this, cx| {
            let probed = rx.await.expect("tokio 桥不会丢弃发送端");
            let (usage, cache_usage, capability) = match probed {
                Ok(probed) => probed,
                Err(e) => {
                    log::warn!("[Settings] 占用与能力探测失败: {e}");
                    // 探测拿不到就别让页面一直转圈：标记已加载，字段留空
                    let _ = this.update(cx, |view, cx| {
                        view.loaded = true;
                        view.loading = false;
                        cx.notify();
                    });
                    return;
                }
            };
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
                let limit = settings.cache_limit_mb.to_string();
                view.cache_limit_input
                    .update(cx, |input, cx| input.set_value(limit, window, cx));
                let ttl = gpui_kit::component::IndexPath::new(super::storage::cache_ttl_index(
                    settings.cache_ttl_days,
                ));
                view.cache_ttl_select.update(cx, |select, cx| {
                    select.set_selected_index(Some(ttl), window, cx)
                });
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
}

impl super::SettingsView {
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
        // 缓存自动清理两档：周期下拉必须选中（不选会显示组件自带的英文
        // 占位文案），上限输入留空 = 不限
        self.cache_ttl_select.update(cx, |select, cx| {
            select.set_selected_index(Some(gpui_kit::component::IndexPath::new(0)), window, cx)
        });
        self.cache_limit_input
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
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
