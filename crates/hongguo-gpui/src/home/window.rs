//! 窗口与来源切换：tab 换源、无边框播放、置顶、沉浸全屏（壳与控制条的
//! 入口，状态都在 [`HomeView`] 上）。

use super::*;

use gpui_kit::Window;

impl HomeView {
    /// 切沉浸流 tab（顶栏 tab 入口）：重复点当前 tab = 换一批（重开会话、
    /// 游标归零、在播剧钉住不动）；切到别的 tab 关掉在播钉——新源首批到位
    /// 或缓存游标处的剧立即接管播放，切 tab 就换片（Tauri pinHold 同口径）。
    pub(crate) fn switch_source(&mut self, s: Source, cx: &mut Context<Self>) {
        log::info!(
            "[Home] 切换 tab 源 → {}（当前 {}）",
            self.tab_label(&s),
            self.tab_label(&self.active)
        );
        let same = self.active == s;
        let empty = self.feeds.get(&s).is_none_or(|f| f.items.is_empty());
        if same {
            // 换一批：重开会话、游标归零，在播剧钉住不动（开闸）
            self.pin_hold = true;
            self.load_feed(s, true, cx);
            return;
        }
        // 跨 tab 关闸：钉子不关的话，游标 0 会被在播剧占住，首批到位后
        // follow_cursor/play_target 判「目标就是在播」而不切——点了没反应
        self.pin_hold = false;
        self.active = s.clone();
        if empty {
            self.load_feed(s, false, cx);
        } else {
            let cursor_item = self
                .display_items()
                .get(self.cursor())
                .map(|i| i.series_id.clone());
            let target_same = self
                .now
                .as_ref()
                .is_some_and(|n| cursor_item.as_deref() == Some(&n.series_id));
            if !target_same && let Some(item) = self.display_items().get(self.cursor()).cloned() {
                self.play_target(item, None, None, cx);
            }
        }
        cx.notify();
    }

    /// 无边框播放（控制栏按钮 / Esc 退出）：chrome 全藏但**不进 OS 全屏**，
    /// 窗口保持原几何（播放页 bare 布局同款）。与沉浸全屏互斥——
    /// 开无边框先退 OS 全屏。macOS 红绿灯一并藏（Tauri 同款只 setHidden
    /// 三颗圆点不动 styleMask，见 [`crate::macos`]）。
    pub(super) fn toggle_frameless(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = !self.frameless;
        if next && self.immersive {
            self.toggle_immersive_fullscreen(window, cx);
        }
        self.frameless = next;
        crate::platform::set_traffic_lights_hidden(window, next);
        // 壳必须重绘才收得起/放得回头顶这层 chrome（见 HomeEvent::ChromeChanged）
        cx.emit(HomeEvent::ChromeChanged);
        self.wake_controls(cx);
        cx.notify();
    }

    /// 置顶窗口（顶栏入口与小屏紧凑条共用；FFI 成功才翻状态，两边不各说
    /// 各话——Tauri usePinWindow 同一份语义）。
    pub(crate) fn toggle_pinned(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = !self.pinned;
        if crate::mini::set_topmost(window, cx, next) {
            self.pinned = next;
            log::info!("[Home] 窗口钉住 → {next}");
            cx.notify();
        }
    }

    /// 播放器全屏（控制条按钮 / F / Esc）：OS 全屏 + chrome（顶栏/侧栏）
    /// 全藏 = 真沉浸。与「应用全屏」（顶栏最大化钮，chrome 保留）分层。
    /// 与无边框播放互斥：进全屏先退无边框（红绿灯显回，全屏由系统接管）。
    pub(crate) fn toggle_immersive_fullscreen(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.frameless && !self.immersive {
            self.frameless = false;
            crate::platform::set_traffic_lights_hidden(window, false);
        }
        self.immersive = !self.immersive;
        window.toggle_fullscreen();
        // 同 frameless：chrome 显隐翻面要推壳重绘（OS 全屏事件不一定及时）
        cx.emit(HomeEvent::ChromeChanged);
        self.wake_controls(cx);
        cx.notify();
    }
}
