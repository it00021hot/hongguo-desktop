//! 视图状态：渲染门槛/壳协作面的 getter，以及语言刷新、轻提示、快捷键
//! 映射换入这类小状态写入。

use super::*;

use gpui_kit::Window;

impl HomeView {
    /// 引擎是否就位：控制条、选集/下载面板渲染的共同门槛。真实壳=引擎
    /// 句柄在；预览进程由种子置位虚拟引擎（screenshot feature）。
    pub(super) fn engine_ready(&self) -> bool {
        self.player.is_some() || self.preview_engine()
    }

    #[cfg(feature = "screenshot")]
    fn preview_engine(&self) -> bool {
        self.preview_engine
    }

    #[cfg(not(feature = "screenshot"))]
    fn preview_engine(&self) -> bool {
        false
    }

    // —— 壳协作面（Shell 顶栏/侧栏读写的最小开口） ——

    /// 当前 tab 源（预览交互探针读，断言切源生效）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn active_source(&self) -> Source {
        self.active.clone()
    }

    /// 窗口是否已置顶（顶栏钮的图标态；与小屏紧凑条共用 `pinned`）。
    pub(crate) fn pinned(&self) -> bool {
        self.pinned
    }

    /// chrome（顶栏/侧栏）是否该藏：沉浸全屏或无边框播放任一开着就算——
    /// Shell 据此整窗只剩画面（Tauri playerFullscreen || playerFrameless
    /// 同一条件，名字保留沉浸语义）。
    pub(crate) fn immersive_fullscreen(&self) -> bool {
        self.immersive || self.frameless
    }

    /// 设置页改了快捷键映射：换进运行中的视图（新值已由设置页落盘）。
    pub(crate) fn reload_shortcut_keys(
        &mut self,
        keys: crate::playback_prefs::ShortcutKeys,
        cx: &mut Context<Self>,
    ) {
        self.shortcut_keys = keys;
        cx.notify();
    }

    /// 弹一条轻量提示（Tauri toast 同位）：单槽覆盖，定时收尾（不依赖
    /// 重渲染节拍——暂停中没有 Frame 事件也能按时消失）。
    pub(crate) fn show_toast(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.toast = Some(crate::toast::Toast::new(message));
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(crate::toast::TOAST_TTL)
                .await;
            let _ = this.update(cx, |view, cx| {
                if view
                    .toast
                    .as_ref()
                    .is_some_and(crate::toast::Toast::expired)
                {
                    view.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 语言切换：刷新输入框 placeholder（输入内容保持不动）。
    pub(crate) fn refresh_locale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = &self.danmaku_input {
            input.update(cx, |input, cx| {
                input.set_placeholder(
                    crate::i18n::t("player.interact.danmakuPlaceholder"),
                    window,
                    cx,
                );
            });
        }
        if let Some(input) = &self.comment_input {
            input.update(cx, |input, cx| {
                input.set_placeholder(crate::i18n::t("player.comments.placeholder"), window, cx);
            });
        }
        self.download_range.update(cx, |input, cx| {
            input.set_placeholder(crate::i18n::t("download.rangePlaceholder"), window, cx);
        });
        cx.notify();
    }
}
