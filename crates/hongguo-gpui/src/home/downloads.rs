//! 「下载到本地」浮层（Tauri `DownloadSheet` + `EpisodeRangePicker` 同款）：
//! 集数网格逐集勾选、快捷区间（全选/反选/清空/前 10/前 30/后 30）、区间语法
//! 输入（`1-50`、`1-10, 25, 30-45`），提交走 `download_service::actions::
//! enqueue_batch`，成功切下载任务页。
//!
//! 浮层由壳在**整窗**层渲染（Tauri Sheet 固定 `inset-x-0 bottom-0`，横贯
//! 侧栏）——`Shell::render` 取 `render_downloads_sheet`，舞台不再自己画。

use std::sync::Arc;

use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, SharedString, Toggled, Window, div};

use super::HomeEvent;
use super::downloads_range::{first_n, format_range, last_n, parse_range};
use crate::bridge;
use crate::ui_button::button as Button;

impl super::HomeView {
    /// 打开/收起「下载到本地」浮层。每次打开重置为「当前这一集」
    /// （Tauri `handleOpen` 同款：不带上次勾选残留）。
    pub(super) fn toggle_downloads(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.downloads_open = !self.downloads_open;
        if self.downloads_open {
            self.reset_download_picker(window, cx);
        }
        self.wake_controls(cx);
        cx.notify();
    }

    /// 关闭浮层（Esc / 取消 / 提交成功后）。
    pub(super) fn close_downloads(&mut self, cx: &mut Context<Self>) {
        self.downloads_open = false;
        cx.notify();
    }

    /// 打开时回到「在播这一集」并清空区间串。
    pub(super) fn reset_download_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.now.as_ref().map(|n| n.vid_index).unwrap_or(1);
        self.download_selected = vec![current];
        self.download_range_error = false;
        let input = self.download_range.clone();
        input.update(cx, |input, cx| {
            // 静默写值（不派 Change，避免反过来触发一次解析）
            input.set_value("", window, cx);
        });
    }

    /// 区间串变化即解析（Tauri `applyRange` 同款）：空串 = 清空选集；
    /// 能解析出至少一集就采用能解析的部分；一集都解析不出才标红。
    pub(super) fn apply_download_range(&mut self, text: &str, cx: &mut Context<Self>) {
        let total = self.episode_total();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            self.download_range_error = false;
            self.download_selected.clear();
            cx.notify();
            return;
        }
        let parsed = parse_range(trimmed, total);
        if parsed.is_empty() {
            self.download_range_error = true;
        } else {
            self.download_range_error = false;
            self.download_selected = parsed;
        }
        cx.notify();
    }

    fn episode_total(&self) -> u32 {
        self.now
            .as_ref()
            .map(|n| n.episode_count.min(u32::MAX as usize) as u32)
            .unwrap_or(0)
    }

    fn set_download_selection(&mut self, next: Vec<u32>, cx: &mut Context<Self>) {
        self.download_selected = next;
        self.download_range_error = false;
        cx.notify();
    }

    /// 提交：入队勾选集 → toast → 关浮层 → 切下载任务页（Tauri 同序）。
    fn submit_downloads(&mut self, cx: &mut Context<Self>) {
        let Some(now) = self.now.clone() else {
            return;
        };
        if self.download_selected.is_empty() {
            return;
        }
        let vids = self.download_selected.clone();
        match hongguo_service::download_service::actions::enqueue_batch(
            &self.state,
            &now.series_id,
            &vids,
        ) {
            Ok(added) => {
                log::info!("[Home] 下载入队 {added} 集（勾选 {} 集）", vids.len());
                self.show_toast(
                    crate::i18n::tf("download.submitting", &[("count", &vids.len().to_string())]),
                    cx,
                );
                self.close_downloads(cx);
                cx.emit(HomeEvent::OpenTasks);
            }
            Err(e) => {
                log::warn!("[Home] 下载入队失败: {e}");
                let message = crate::i18n::app_error_text(&e);
                self.show_toast(
                    crate::i18n::tf("common.failedWith", &[("message", message.as_str())]),
                    cx,
                );
            }
        }
    }

    /// 刷新「已下载集号」快照（选集浮层的完成角标与下载浮层共用）。
    pub(super) fn refresh_downloads(&mut self, cx: &mut Context<Self>) {
        let state = Arc::clone(&self.state);
        let sid = self
            .now
            .as_ref()
            .map(|n| n.series_id.clone())
            .unwrap_or_default();
        let rx = bridge::spawn(async move { state.queue().completed_vids(&sid) });
        cx.spawn(async move |this, cx| {
            let done = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, _| {
                view.done_vids = Arc::new(done);
            });
        })
        .detach();
    }

    /// 「下载到本地」浮层（壳层渲染；None = 没开）。
    pub(crate) fn render_downloads_sheet(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        if !self.downloads_open {
            return None;
        }
        let theme = cx.theme();
        let total = self.episode_total();
        let selected_len = self.download_selected.len();
        let selected_all: Vec<u32> = (1..=total).collect();
        let inverted: Vec<u32> = (1..=total)
            .filter(|n| !self.download_selected.contains(n))
            .collect();

        let outline =
            |id: &'static str, label: String| Button::new(id).label(label).small().outline();
        let ghost = |id: &'static str, label: String| Button::new(id).label(label).small().ghost();
        let quick = h_flex()
            .id("download-quick")
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                outline("download-select-all", crate::i18n::t("download.selectAll")).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.set_download_selection(selected_all.clone(), cx);
                    }),
                ),
            )
            .child(
                outline("download-invert", crate::i18n::t("download.invert")).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.set_download_selection(inverted.clone(), cx);
                    }),
                ),
            )
            .child(
                outline("download-clear", crate::i18n::t("download.clear")).on_click(
                    cx.listener(|this, _, _, cx| this.set_download_selection(Vec::new(), cx)),
                ),
            )
            .child(
                ghost("download-first-10", crate::i18n::t("download.first10")).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.set_download_selection(first_n(total, 10), cx);
                    }),
                ),
            )
            .child(
                ghost("download-first-30", crate::i18n::t("download.first30")).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.set_download_selection(first_n(total, 30), cx);
                    }),
                ),
            )
            .child(
                ghost("download-last-30", crate::i18n::t("download.last30")).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.set_download_selection(last_n(total, 30), cx);
                    },
                )),
            );

        // 区间语法行：输入即解析；勾选非空时给「格式化选集」回填键
        let range_row = h_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .child(gpui_kit::component::input::Input::new(&self.download_range)),
            )
            .children((selected_len > 0).then(|| {
                Button::new("download-format-selection")
                    .icon(gpui_kit::assets::IconName::Check)
                    .ghost()
                    .small()
                    .tooltip(crate::i18n::t("download.formatSelection"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let text = format_range(&this.download_selected);
                        let input = this.download_range.clone();
                        input.update(cx, |input, cx| {
                            input.set_value(text, window, cx);
                        });
                    }))
            }));

        let grid = div()
            .id("download-episodes")
            .max_h(gpui_kit::px(256.0))
            .overflow_y_scroll()
            .rounded(gpui_kit::px(6.0))
            .border_1()
            .border_color(theme.border)
            .p_2()
            .child(
                div()
                    .grid()
                    .grid_cols(8)
                    .gap_1()
                    .children((1..=total).map(|n| {
                        let checked = self.download_selected.contains(&n);
                        // 每格可点 → base::Button：焦点句柄、Tab 可达、
                        // Enter/Space 激活与 role 由库负责；勾选语义走
                        // aria-toggled（Tauri 侧 aria-pressed 同款），无障碍名
                        // 按 Tauri 基线「选择集数 + 集号」。
                        BaseButton::new(ElementId::from(format!("download-ep-{n}")))
                            .accessibility_label(format!(
                                "{} {}",
                                crate::i18n::t("download.selectEpisodes"),
                                n
                            ))
                            .aria_toggled(if checked {
                                Toggled::True
                            } else {
                                Toggled::False
                            })
                            .h(gpui_kit::px(32.0))
                            .flex()
                            .justify_center()
                            .items_center()
                            .rounded(gpui_kit::px(6.0))
                            .border_1()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .cursor_pointer()
                            .when(checked, |s| {
                                // Tauri EpisodeRangePicker 选中态：border-transparent
                                s.bg(theme.primary)
                                    .border_color(gpui_kit::rgba(0x00000000))
                                    .text_color(theme.primary_foreground)
                            })
                            .when(!checked, |s| {
                                s.bg(theme.background)
                                    .border_color(theme.border)
                                    .text_color(theme.foreground)
                                    .hover(|h| h.bg(theme.accent))
                            })
                            // base Button 强制 line_height(relative(1.))，文字行盒比
                            // 默认矮一截；钉回默认行高才与原 div 像素一致。
                            .line_height(gpui_kit::phi())
                            // 下载浮层跟主题走（亮色就是亮面板），焦点环取主题
                            // ring 而不是皮肤区的白色外圈——白圈压在亮面板上看不见。
                            .focus_visible(|s| s.shadow(crate::tokens::focus_ring(theme)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let mut next = this.download_selected.clone();
                                if let Some(pos) = next.iter().position(|&i| i == n) {
                                    next.remove(pos);
                                } else {
                                    next.push(n);
                                    next.sort_unstable();
                                }
                                this.set_download_selection(next, cx);
                            }))
                            .child(SharedString::from(n.to_string()))
                    })),
            );

        Some(
            v_flex()
                .id("downloads-sheet")
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .max_h(gpui_kit::relative(0.8))
                .overflow_hidden()
                .rounded_t(gpui_kit::px(12.0))
                .border_t_1()
                .border_color(theme.border)
                // Tauri DownloadSheet 走 shadcn Sheet（bg-background）：
                // **主题跟随**，亮色就是亮面板。此前写死深底、内部却取主题
                // 令牌，亮色主题下黑字压深板全部穿帮（预览实测）。
                .bg(theme.background)
                .px_6()
                .pt_4()
                .pb_4()
                .gap_3()
                // 浮层吞掉鼠标/滚轮：底下的舞台不接受点击/切剧
                .on_mouse_down(
                    gpui_kit::MouseButton::Left,
                    cx.listener(|_, _, _, cx| cx.stop_propagation()),
                )
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                // 浮层本身不是控件：面板里的控件各自已是 base::Button，键盘走
                // Tab 即可；这里只需要「点不到底下的画面」。拦住 mouse_down 就
                // 够了——gpui 的 click 由「同一元素上按下+抬起」合成，按下被
                // 拦下就合成不出点击（仓库既有纪律同款：评论面板、简介块）。
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_base()
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .child(crate::i18n::t("player.downloadToLocal")),
                        )
                        .child(
                            Button::new("downloads-close")
                                .icon(gpui_kit::assets::IconName::X)
                                .ghost()
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| this.close_downloads(cx))),
                        ),
                )
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .text_color(theme.foreground)
                                .child(crate::i18n::t("download.selectEpisodes")),
                        )
                        .child(gpui_kit::component::tag::Tag::secondary().child(
                            SharedString::from(crate::i18n::tf(
                                "download.selected",
                                &[("count", &selected_len.to_string())],
                            )),
                        )),
                )
                .child(quick)
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.foreground)
                                .child(crate::i18n::t("download.rangeLabel")),
                        )
                        .child(range_row)
                        .children(self.download_range_error.then(|| {
                            div()
                                .text_xs()
                                .text_color(theme.danger)
                                .child(crate::i18n::t("download.rangeInvalid"))
                        })),
                )
                .child(grid)
                .child(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("downloads-cancel")
                                .label(crate::i18n::t("common.cancel"))
                                .outline()
                                .on_click(cx.listener(|this, _, _, cx| this.close_downloads(cx))),
                        )
                        .child(
                            Button::new("downloads-submit")
                                .icon(gpui_kit::assets::IconName::Download)
                                .label(crate::i18n::t("download.submit"))
                                .primary()
                                .disabled(selected_len == 0)
                                .when(selected_len == 0, |b| b.cursor_default())
                                .on_click(cx.listener(|this, _, _, cx| this.submit_downloads(cx))),
                        ),
                )
                .into_any_element(),
        )
    }

    /// 取证辅助（screenshot feature 的 HONGGUO_SHOT_PANEL=downloads）：
    /// 打开「下载到本地」浮层（无人值守抓选集器 UI）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_downloads(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.downloads_open = true;
        self.reset_download_picker(window, cx);
        cx.notify();
    }
}
