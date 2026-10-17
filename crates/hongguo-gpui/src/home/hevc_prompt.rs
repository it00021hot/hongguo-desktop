//! 缺 HEVC 扩展时的安装引导（Tauri `HevcInstallCard` 的 proactive 形态）。
//!
//! Tauri 有两档：解码失败后的**居中主卡**（要么装扩展、要么转兼容格式）与
//! 提前告知的**底部悬浮卡**。GPUI 侧选流时就会按 `hevc_playable()` 让接口
//! 下发 H.264（`DecoderCapability::from_hevc`，见 `home/playback.rs`），不会
//! 走到「有声音没画面」那条路——所以这里只做 proactive 那一档：告诉用户当前
//! 是降档画质，装好扩展后续剧集自动切回 H.265。
//!
//! 会话记忆对齐 Tauri `hevc-session`：点过「暂不安装」本进程不再打扰
//!（切剧/小窗重挂都不重弹），重启应用重置。

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Disableable;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::prelude::*;

impl super::HomeView {
    /// 底部悬浮安装引导（Tauri HevcInstallCard floating 形态：不盖画面、
    /// 不打断正在播的内容）。
    pub(super) fn render_hevc_prompt(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        h_flex()
            .id("hevc-prompt")
            .absolute()
            .bottom(gpui_kit::px(56.0))
            .left_0()
            .right_0()
            .justify_center()
            .px_6()
            // 悬浮卡自己吃掉点击：别让它点到舞台（点画面 = 暂停）
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .child(
                v_flex()
                    .max_w(gpui_kit::px(384.0))
                    .w_full()
                    .gap_2()
                    .rounded(gpui_kit::px(8.0))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .p_4()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .child(crate::i18n::t("player.hevcCardTitle")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("player.hevcCardDesc")),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                Button::new("hevc-install")
                                    .label(if self.hevc_installing {
                                        crate::i18n::t("player.hevcInstalling")
                                    } else {
                                        crate::i18n::t("player.hevcInstall")
                                    })
                                    .small()
                                    .disabled(self.hevc_installing)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.install_hevc_extension(cx);
                                    })),
                            )
                            .child(
                                Button::new("hevc-later")
                                    .label(crate::i18n::t("player.hevcLater"))
                                    .ghost()
                                    .small()
                                    .disabled(self.hevc_installing)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.hevc_prompt_dismissed = true;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// 一键安装（Tauri `useInstallHevcExtension` 同源：service 下载微软签名
    /// 扩展包 → 校验 SHA-256 → `Add-AppxPackage` 用户级安装 → 重探能力）。
    fn install_hevc_extension(&mut self, cx: &mut Context<Self>) {
        if self.hevc_installing {
            return;
        }
        self.hevc_installing = true;
        cx.notify();
        let proxy = self.state.api_env().proxy;
        let rx = crate::bridge::spawn(async move {
            hongguo_service::capability_service::install_hevc_extension(proxy).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.hevc_installing = false;
                match result {
                    // GPUI 不会「正在重试播放」：当前这集早已按 H.264 起播且播放
                    // 正常，装完的收益从下一集/下一次起播的选流开始（能力已在
                    // service 里重探并缓存）。
                    Ok(true) => view.show_toast(crate::i18n::t("player.hevcInstallSuccess"), cx),
                    Ok(false) => view.show_toast(crate::i18n::t("player.hevcStillNoPicture"), cx),
                    Err(e) => {
                        log::warn!("[Home] HEVC 扩展安装失败: {e}");
                        view.show_toast(
                            crate::i18n::tf(
                                "player.hevcInstallFailed",
                                &[("reason", &crate::i18n::app_error_text(&e))],
                            ),
                            cx,
                        );
                    }
                }
            });
        })
        .detach();
    }
}
