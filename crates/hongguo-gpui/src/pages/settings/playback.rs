//! 播放设置卡：自动下一集 / 看完自动删除 / 最小化暂停，以及卡片内的
//! 转码后端探测入口；另含自动删除的截图取证辅助。
//!
//! 从 `settings.rs` 按域拆出，纯移动零行为变更。

use super::*;

use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable};
use gpui_kit::{App, div};

use super::transcode::ffmpeg_install_block;
use crate::pages::card;
use crate::ui_button::button as Button;

impl super::SettingsView {
    pub(super) fn render_playback_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
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
                            .when(self.redetecting, |b| b.cursor_default())
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
}

impl super::SettingsView {
    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=auto-delete`）：
    /// 走真实保存路径打开「看完自动删除」——验播放到底时真删本地文件。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_set_auto_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.patch(cx, |s| s.auto_delete_after_play = true);
        self.save(window, cx);
        let saved = self.state.settings().auto_delete_after_play;
        log::info!("[AutoDelete] 保存后 库里的看完自动删 = {saved}");
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
