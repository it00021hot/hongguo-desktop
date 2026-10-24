//! 存储/缓存/下载设置卡：下载目录与缓存目录选择、缓存占用展示与清理；
//! 缓存目录展示值的换算（空 = 跟随数据目录）也被加载/保存路径复用。
//!
//! 从 `settings.rs` 按域拆出，纯移动零行为变更。

use super::*;

use gpui_kit::component::button::Button;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::label::Label;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::select::Select;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::div;

use crate::bridge;
use crate::home::fmt::format_bytes;
use crate::pages::card;

/// 缓存自动清理周期的档位（天；0 = 关闭）。下拉里的每一档。
pub(super) const CACHE_TTL_DAYS: [u32; 5] = [0, 1, 3, 7, 30];

/// 周期档位的展示文案（与 [`CACHE_TTL_DAYS`] 同序）。
pub(super) fn cache_ttl_labels() -> [String; 5] {
    [
        crate::i18n::t("settings.cacheTtlOff"),
        crate::i18n::t("settings.cacheTtlDays1"),
        crate::i18n::t("settings.cacheTtlDays3"),
        crate::i18n::t("settings.cacheTtlDays7"),
        crate::i18n::t("settings.cacheTtlDays30"),
    ]
}

/// 周期档位 → 存储值（天）。
pub(super) fn cache_ttl_from_index(ix: usize) -> u32 {
    CACHE_TTL_DAYS.get(ix).copied().unwrap_or(0)
}

/// 存储值（天）→ 档位下标（不认识的档位落到「关闭」）。
pub(super) fn cache_ttl_index(days: u32) -> usize {
    CACHE_TTL_DAYS.iter().position(|d| *d == days).unwrap_or(0)
}

/// 下拉的可选项。
pub(super) fn cache_ttl_items() -> SearchableVec<SharedString> {
    SearchableVec::new(
        cache_ttl_labels()
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<SharedString>>(),
    )
}

/// 缓存目录输入框的展示值：未配置时直接填默认位置（存储语义仍是空 = 跟随数据目录）。
pub(super) fn cache_dir_display(default_cache_dir: &str, cache_dir: &str) -> String {
    if cache_dir.trim().is_empty() {
        default_cache_dir.to_string()
    } else {
        cache_dir.to_string()
    }
}

impl super::SettingsView {
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

    pub(super) fn render_dir_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
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

    pub(super) fn render_storage_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
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
            // 自动清理两档：占用上限（MB）+ 保留周期（天）。两档都关 = 不动
            // 用户的数据；超限/过期由后台每小时扫一轮删最旧的。
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(Label::new(crate::i18n::t("settings.cacheAutoClean")))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                h_flex()
                                    .flex_1()
                                    .items_center()
                                    .gap_2()
                                    .child(div().text_sm().child(SharedString::from(
                                        crate::i18n::t("settings.cacheLimit"),
                                    )))
                                    .child(
                                        div()
                                            .w(gpui_kit::px(96.0))
                                            .child(Input::new(&self.cache_limit_input)),
                                    )
                                    .child(div().text_sm().child(SharedString::from(
                                        crate::i18n::t("settings.cacheLimitUnit"),
                                    ))),
                            )
                            .child(
                                h_flex()
                                    .flex_1()
                                    .items_center()
                                    .gap_2()
                                    .child(div().text_sm().child(SharedString::from(
                                        crate::i18n::t("settings.cacheTtl"),
                                    )))
                                    .child(
                                        div().flex_1().child(Select::new(&self.cache_ttl_select)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("settings.cacheAutoCleanDesc")),
                    ),
            )
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
}
