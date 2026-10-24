//! 磁盘清理页（Tauri `/storage` = `storage-page.tsx` 的 GPUI 对齐）。
//!
//! 布局：居中窄列（768px）两张卡——「磁盘占用」总览（总量 + 文件数 +
//! 删除全部）与「按剧清理」（剧名过滤 + 逐剧行 + 单行删除）；删除全部带
//! AlertDialog 确认（Tauri 同文案），逐行删除不确认（Tauri 亦然）。
//! 数据与操作走 `hongguo_service::storage_service`（usage / cleanup），
//! 与 Tauri 的 `get_storage_*` / `delete_*` command 同一段服务面。

use std::sync::Arc;

use gpui_kit::component::button::{ButtonVariant, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, Entity, SharedString, Window, div};
use hongguo_service::app_state::AppStateInner;
use hongguo_service::storage_service::usage::{StorageSeriesUsage, StorageUsage};

use crate::bridge;
use crate::home::fmt::format_bytes;
use crate::pages::card;
use crate::ui_button::button as Button;

pub(crate) struct StorageView {
    state: Arc<AppStateInner>,
    /// 总占用（None = 还没拿到数，显示「加载中…」而不是 0 B——
    /// 「还没拿到」和「真的清空了」不是一回事，Tauri 同口径）。
    usage: Option<StorageUsage>,
    /// 按剧聚合（只含磁盘上真有文件的剧）。
    series: Option<Vec<StorageSeriesUsage>>,
    filter: Entity<InputState>,
    loading: bool,
    /// 阻塞池任务失败的短句（Some = 整页换错误态 + 重试）。
    failed: Option<String>,
}

impl StorageView {
    /// 语言切换：刷新搜索框 placeholder，保留输入。
    pub(crate) fn refresh_locale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter.update(cx, |input, cx| {
            input.set_placeholder(crate::i18n::t("storage.searchPlaceholder"), window, cx);
        });
        cx.notify();
    }

    pub(crate) fn new(
        state: Arc<AppStateInner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::i18n::t("storage.searchPlaceholder"))
        });
        // 过滤词变化 → 列表重渲染（可见集在 render 里按当前过滤词派生，
        // 与 Tauri 的 useState 派生 visible 同构）。
        cx.subscribe(&filter, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        Self {
            state,
            usage: None,
            series: None,
            filter,
            loading: false,
            failed: None,
        }
    }

    /// 刷新占用快照（进入页面时调用）。加载中幂等；刷新期间保留旧列表
    /// （与 RankView 同款）。占用统计摸磁盘 metadata，走阻塞池。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        // 重试时先清旧错误，否则失败态会一直挂在页面上。
        self.failed = None;
        cx.notify();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            tokio::task::spawn_blocking(move || {
                use hongguo_service::storage_service::usage;
                (usage::usage(&state), usage::series_usage(&state))
            })
            .await
            .map_err(|e| format!("阻塞池任务失败: {e}"))
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            this.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok((usage, series)) => {
                        view.usage = Some(usage);
                        view.series = Some(series);
                        view.failed = None;
                    }
                    Err(msg) => {
                        view.usage = None;
                        view.series = None;
                        view.failed = Some(msg);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 删除某剧的本地文件（含服务端收尾落盘）；成功后刷新快照 + 通知
    /// （Tauri toast 同文案「{剧名} · 已删除 N 个文件」）。
    fn delete_series_now(
        &mut self,
        series_id: String,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match hongguo_service::storage_service::cleanup::delete_series(&self.state, &series_id) {
            Ok(n) => {
                window.push_notification(
                    Notification::success(crate::i18n::tf(
                        "storage.freedFilesOf",
                        &[("title", title.as_str()), ("count", &n.to_string())],
                    )),
                    cx,
                );
                self.load(cx);
            }
            Err(e) => {
                log::warn!("[Storage] 删除《{title}》文件失败: {e}");
                let message = crate::i18n::app_error_text(&e);
                window.push_notification(
                    Notification::error(crate::i18n::tf(
                        "common.failedWith",
                        &[("message", message.as_str())],
                    )),
                    cx,
                );
            }
        }
    }

    /// 删除全部已下载（Tauri AlertDialog 确认后执行；文案同 zh-CN）。
    fn delete_all_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match hongguo_service::storage_service::cleanup::delete_all(&self.state) {
            Ok(n) => {
                window.push_notification(
                    Notification::success(crate::i18n::tf(
                        "storage.freedFiles",
                        &[("count", &n.to_string())],
                    )),
                    cx,
                );
                self.load(cx);
            }
            Err(e) => {
                log::warn!("[Storage] 删除全部失败: {e}");
                let message = crate::i18n::app_error_text(&e);
                window.push_notification(
                    Notification::error(crate::i18n::tf(
                        "common.failedWith",
                        &[("message", message.as_str())],
                    )),
                    cx,
                );
            }
        }
    }

    fn confirm_delete_all(&self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            // 构造器是 Fn（层变化时会重跑），实体句柄每次克隆一份再交给
            // on_ok，避免把捕获的 view 移出 Fn 闭包。
            let view = view.clone();
            dialog
                .title(crate::i18n::t("storage.deleteAllConfirm"))
                .description(crate::i18n::t("storage.deleteAllDesc"))
                .ok_text(crate::i18n::t("common.confirm"))
                .cancel_text(crate::i18n::t("common.cancel"))
                .show_cancel(true)
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |view, cx| view.delete_all_now(window, cx));
                    true
                })
        });
    }

    fn render_usage_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        card(cx)
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                gpui_kit::component::Icon::new(
                                    gpui_kit::assets::IconName::HardDrive,
                                )
                                .small()
                                .text_color(theme.muted_foreground),
                            )
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(crate::i18n::t("settings.storage")),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("storage.desc")),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(match &self.usage {
                                        Some(u) => SharedString::from(format_bytes(u.bytes)),
                                        None => {
                                            SharedString::from(crate::i18n::t("common.loading"))
                                        }
                                    }),
                            )
                            .children(self.usage.as_ref().map(|u| {
                                div().text_sm().text_color(theme.muted_foreground).child(
                                    crate::i18n::tf(
                                        "storage.fileCount",
                                        &[("count", &u.files.to_string())],
                                    ),
                                )
                            })),
                    )
                    .child(
                        Button::new("storage-delete-all")
                            .icon(gpui_kit::assets::IconName::Trash)
                            .label(crate::i18n::t("settings.deleteAll"))
                            .small()
                            .danger()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_delete_all(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_series_row(
        &self,
        s: &StorageSeriesUsage,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        h_flex()
            .items_center()
            .gap_2()
            .rounded(gpui_kit::px(6.0))
            .border_1()
            .border_color(theme.border)
            .px_3()
            .py_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .child(SharedString::from(s.title.clone())),
            )
            .child(Tag::secondary().child(SharedString::from(crate::i18n::tf(
                "storage.downloadedEpisodes",
                &[("count", &s.files.to_string())],
            ))))
            .child(
                div()
                    .w_16()
                    .flex_shrink_0()
                    .text_right()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(format_bytes(s.bytes))),
            )
            .child(
                Button::new(ElementId::from(format!("storage-del-{}", s.series_id)))
                    .icon(gpui_kit::assets::IconName::Trash)
                    .ghost()
                    .small()
                    .tooltip(crate::i18n::t("storage.deleteFiles"))
                    .on_click({
                        let series_id = s.series_id.clone();
                        let title = s.title.clone();
                        cx.listener(move |this, _, window, cx| {
                            this.delete_series_now(series_id.clone(), title.clone(), window, cx);
                        })
                    }),
            )
            .into_any_element()
    }

    fn render_series_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let filter = self.filter.read(cx).value();
        let kw = filter.trim().to_lowercase();
        let visible: Vec<&StorageSeriesUsage> = self
            .series
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter(|s| kw.is_empty() || s.title.to_lowercase().contains(&kw))
            .collect();
        card(cx)
            .child(
                div()
                    .text_base()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(crate::i18n::t("storage.bySeries")),
            )
            .child(Input::new(&self.filter))
            .child(if self.series.is_none() {
                // 首拉加载：行骨架铺在列表位（数据到位不跳版）。行高按 GPUI
                // 真实行算，不照抄 Tauri 的 h-24：行内容高 25（状态标签
                // 最高：文字 15 + 内边距 8 + 边框 2；标题行 23 / 删除按钮 24
                // 都比它矮）+ 行内上下 p_2 16 + 边框 2 = 43；行间距另按
                // 真实列表的 gap_2 给 8。
                crate::pages::skeleton::rows(cx, 5, 43.0, 8.0)
            } else if visible.is_empty() {
                crate::pages::states::empty_state(cx, None, crate::i18n::t("common.empty"), None)
            } else {
                v_flex()
                    .w_full()
                    .gap_2()
                    .children(visible.into_iter().map(|s| self.render_series_row(s, cx)))
                    .into_any_element()
            })
            .into_any_element()
    }

    /// 页面主体：加载失败时整页换错误态（占用统计与剧列表同源，一起失败）。
    fn render_body(&self, cx: &mut Context<Self>) -> Vec<gpui_kit::AnyElement> {
        if let Some(msg) = self.failed.clone() {
            return vec![crate::pages::states::error_state(
                cx,
                None,
                crate::i18n::t("common.loadFailed"),
                Some(msg),
                Some((
                    "storage-retry",
                    crate::pages::states::retry_on(cx.entity(), |this, cx| this.load(cx)),
                )),
            )];
        }
        vec![self.render_usage_card(cx), self.render_series_card(cx)]
    }

    /// 预览种子（screenshot feature）：灌假占用与假剧列表，跳过磁盘扫描。
    /// `storage-empty` = 清空后的空态分支（0 B + 空列表）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(&mut self, scene: &str, cx: &mut Context<Self>) {
        let empty = scene == "storage-empty";
        self.loading = false;
        self.usage = Some(if empty {
            StorageUsage { bytes: 0, files: 0 }
        } else {
            crate::preview::fixtures::storage_usage()
        });
        self.series = Some(if empty {
            Vec::new()
        } else {
            crate::preview::fixtures::storage_series()
        });
        cx.notify();
    }
}

impl gpui_kit::Render for StorageView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .id("storage-page")
            .overflow_y_scroll()
            .child(
                v_flex()
                    .max_w(gpui_kit::px(768.0))
                    .mx_auto()
                    .w_full()
                    .p_6()
                    .gap_4()
                    .children(self.render_body(cx)),
            )
    }
}
