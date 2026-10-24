//! 首页渲染入口：整页元素树（Render 实现）与壳顶栏的 tab 槽。

use super::*;

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Sizable;
use gpui_kit::component::h_flex;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::{Window, div};

use crate::ui_button::button as Button;

impl gpui_kit::Render for HomeView {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let build_started = Instant::now();
        // 出帧节奏取证（每 64 帧一行，debug 级）：本行是**上屏链路唯一的
        // 量化哨兵**。间隔接近刷新周期（16.7ms@60Hz）说明合成跟得上；
        // 几十上百毫秒说明主线程在等 GPU——`sample <pid>` 会显示栈停在
        // `CAMetalLayer nextDrawable` 的信号量上（呈现池耗尽）。
        // 排障实录：本机开系统动态视频壁纸时 GPU 占用 95%，本行跌到 5fps；
        // 关掉壁纸后 GPU 7%、回升到 22–28fps。
        static RENDERS: AtomicU32 = AtomicU32::new(0);
        static LAST_RENDER_MS: AtomicU64 = AtomicU64::new(0);
        let n = RENDERS.fetch_add(1, Ordering::Relaxed);
        if n.is_multiple_of(64) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let last = LAST_RENDER_MS.swap(now, Ordering::Relaxed);
            if last != 0 {
                let interval = (now - last) as f64 / 64.0;
                log::debug!(
                    "[Home] render #{n}（均 {interval:.1}ms/帧 → {:.1}fps）",
                    1000.0 / interval.max(1.0)
                );
            }
        }
        // 竖屏重适配：换集/换清晰度后档位尺寸变了就按新比例重落窗（比例
        // 变化 ≤1% 不动）。挂在这里是因为它是「起播完成后」唯一一定有
        // Window 的钩子——起播的异步收尾处拿不到 Window。
        self.refit_portrait(window, cx);
        // render_* 要 &mut cx，先取（theme 借的是 cx 的不可变引用）。
        // 分类 tab 已移入壳顶栏（Shell render_title_bar），舞台从 tab 行
        // 下方起排改为吃满内容区（Tauri 沉浸流同款）。
        // 播放快捷键的焦点种子：键盘事件只派发给焦点子树，舞台的
        // FocusHandle 得在首次渲染时拿到焦点（只播种一次——之后焦点可能
        // 在弹层菜单里，每帧强抢会打架；点舞台会带回焦点）。
        // 放在 theme 借用之前：focus 要可变借 cx。
        if !self.focus_seeded && !self.focus.is_focused(window) {
            self.focus_seeded = true;
            window.focus(&self.focus, cx);
        }
        // 弹幕发送成功后的输入框清空（set_value 要 window，这里补执行）
        if self.danmaku_clear_input
            && let Some(input) = &self.danmaku_input
        {
            self.danmaku_clear_input = false;
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        // 评论输入框的延迟写入（set_value 要 window）：
        // 1) 发送成功清空——comment_clear_input 此前只置位没人消费（发完
        //    不清，与 Tauri 不一致），一并修掉；
        // 2) 提及插入（候选点击 → 吞掉 `@词` 触发词后接 `@名字\u{FFFC} `）
        if let Some(input) = &self.comment_input {
            if self.comment_clear_input {
                self.comment_clear_input = false;
                self.mention_user = None;
                input.update(cx, |state, cx| state.set_value("", window, cx));
            } else if let Some(segment) = self.mention_insert.take() {
                let current = input.read(cx).value().to_string();
                let base = comment_view::strip_trailing_mention_trigger(&current);
                input.update(cx, |state, cx| {
                    // 带 token 的 set_value：重置后表情码与提及仍是原子内联
                    // 单元（提及插入是全量重置点之一，顺带把存量代码重新附着）
                    state.set_value(
                        crate::danmaku_emoji::rich_content(&format!("{base}{segment}")),
                        window,
                        cx,
                    )
                });
            }
        }
        // 下载任务快照刷新（render_stage 是 &self，写不进去；借这里的 &mut）
        if self.downloads_open {
            let sid = self
                .now
                .as_ref()
                .map(|n| n.series_id.clone())
                .unwrap_or_default();
            self.downloads = Arc::new(self.state.queue().of_series(&sid));
            self.done_vids = Arc::new(self.state.queue().completed_vids(&sid));
        }
        // 全屏态图标跟随（Render::render 有 window 可查）
        self.fullscreen = window.is_fullscreen();
        let theme = cx.theme().clone();
        let feed = self.feeds.get(&self.active).expect("源已初始化");
        // (提示文案, 是否给手动重试)：加载态由自动重试链接管，不能把
        // 「重试」按钮和「加载中…」画在一起——启动首屏正常在拉请求时也
        // 会冒出重试钮，用户会以为启动即失败（用户启动实测截图）。
        // 只有失败与空页才需要用户动作；空页保留重试（Tauri 空态同款）。
        let notice: Option<(String, bool)> = if let Some(err) = &feed.failed {
            Some((err.clone(), true))
        } else if feed.loading && self.now.is_none() {
            Some((crate::i18n::t("common.loading"), false))
        } else if !feed.loading && feed.items.is_empty() && self.now.is_none() {
            Some((crate::i18n::t("common.empty"), true))
        } else {
            None
        };
        let body = match notice {
            Some((message, retry)) => {
                let mut panel = v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    // 壳根是透明的（给视频洞让路）；加载/空/失败态不是舞台，
                    // 必须自己铺黑，不能把桌面透出来。
                    .bg(gpui_kit::rgb(0x000000));
                if !retry {
                    panel = panel.child(Spinner::new().small());
                }
                panel = panel.child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(message),
                );
                if retry {
                    panel = panel.child(
                        Button::new("home-retry")
                            .label(crate::i18n::t("common.retry"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.load_feed(this.active.clone(), false, cx);
                            })),
                    );
                }
                panel.into_any_element()
            }
            None => self
                .render_stage(window, cx)
                .id("home-stage")
                .on_mouse_move(cx.listener(|this, _: &gpui_kit::MouseMoveEvent, _, cx| {
                    this.wake_controls(cx);
                }))
                // 指针离开画面（含鼠标直接甩出窗口）：立即收——控制栏
                // hover 位一并清掉，不然「指针最后停在控制栏上时甩出窗」
                // 会永久常显，得点一下别处才收
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    this.pointer_on_stage = *hovered;
                    if !hovered {
                        this.controls_hovered = false;
                        this.volume_hovered = false;
                        this.volume_popup_hovered = false;
                        this.volume_popup_open = false;
                        if this.controls_until.is_some() {
                            this.controls_until = None;
                        }
                        cx.notify();
                    }
                }))
                .on_scroll_wheel(cx.listener(
                    |this, ev: &gpui_kit::ScrollWheelEvent, window, cx| {
                        // 小屏里滚轮不切剧（纯播放器语义，同 ↑↓ 守卫）
                        if this.mini.is_some() {
                            return;
                        }
                        // 滚轮也是「用户在场」（真实输入）：切剧/切集时唤醒
                        // 悬浮层（Tauri wakeChrome 同款，面板守卫之前就亮；
                        // 静默期滚轮同样解除——它不是指针晃动）
                        this.wake_on_input(cx);
                        // 面板开着不抢滚动：选集/下载是 Tauri
                        // seriesPanelOpen/downloading 的显式守卫；评论面板
                        // 在 Tauri 靠 data-wheel-block 拦——面板类统一在
                        // 舞台层守卫（条状区域见各自根节点的滚轮拦截）
                        if this.picker_open || this.comments_panel_open || this.downloads_open {
                            return;
                        }
                        // Tauri 同参：|Δy|<15 忽略；方向同 Tauri
                        //（deltaY>0=下滚=下一部，y<0 即下一部）
                        let y = ev.delta.pixel_delta(window.line_height()).y.to_f64();
                        if y.abs() < 15.0 {
                            return;
                        }
                        // 400ms 冷却吃掉一次惯性滚动的连发。裁决在舞台层
                        // 而不是 step() 里：连播切集路径同样要冷却，而键盘
                        // ↑↓（Tauri 同款）不吃这把滚轮锁
                        if !this.wheel_cooldown_ready() {
                            return;
                        }
                        this.step(if y < 0.0 { 1 } else { -1 }, cx);
                    },
                ))
                .into_any_element(),
        };
        // 系统播放器画面已就绪时，Home 根也是透明洞的一部分——它盖在 Shell
        // 根和舞台之间，铺主题底色会把下层 AVPlayerLayer 整块遮黑。
        let video_hole = self.system_video_ready && !crate::preview::is_active();
        // 底色恒黑（不是主题底色）：视频位与 GPUI 内容是两张画布，
        // 拖动改窗口尺寸时舞台/黑边会落后一帧，任何**未被覆盖**的像素都会露
        // 出来——露主题浅色就是用户看到的那条「白线」。黑色与舞台/黑边同色，
        // 缝就彻底不可见（macOS 靠窗口黑底天然如此）。
        let root = v_flex()
            .size_full()
            .bg(gpui_kit::rgb(0x000000))
            .when(video_hole, |el| el.bg(gpui_kit::rgba(0x00000000)))
            .text_color(theme.foreground)
            // 沉浸流锁死在视口内：任何 1px 超高都会冒出页面滚动条并吃掉滚轮
            .overflow_hidden()
            .child(body);
        // 元素树构建耗时（不含 GPUI 的 layout/paint/GPU 提交）：用于把
        // 「主线程慢」拆成「我们建树慢」还是「渲染器慢」。
        static BUILD_US: AtomicU64 = AtomicU64::new(0);
        static BUILD_MAX_US: AtomicU64 = AtomicU64::new(0);
        static BUILD_N: AtomicU32 = AtomicU32::new(0);
        let us = build_started.elapsed().as_micros() as u64;
        BUILD_US.fetch_add(us, Ordering::Relaxed);
        BUILD_MAX_US.fetch_max(us, Ordering::Relaxed);
        let n = BUILD_N.fetch_add(1, Ordering::Relaxed) + 1;
        if n.is_multiple_of(64) {
            let avg = BUILD_US.swap(0, Ordering::Relaxed) / u64::from(n.min(64));
            let max = BUILD_MAX_US.swap(0, Ordering::Relaxed);
            BUILD_N.store(0, Ordering::Relaxed);
            log::debug!("[Home][render] 建树均 {avg}µs/最大 {max}µs（{n} 帧累计）");
        }
        root
    }
}

impl HomeView {
    /// tab 的显示名：服务端下发标题优先，缺省回落本地名。
    pub(crate) fn tab_label(&self, s: &Source) -> String {
        self.tab_labels
            .get(s.as_str())
            .filter(|t| !t.is_empty())
            .cloned()
            .unwrap_or_else(|| s.fallback_label().to_string())
    }

    /// 顶栏中部 tab 槽（Tauri `TopBarTabs` portal 同位）：壳在顶栏渲染它，
    /// 点 tab 换源。列表由服务端下发（回落三件套见 `feed::Source`）。
    /// 舞台里的悬浮 tab 行已随顶栏化退场（Tauri 只有顶栏一处）。
    pub(crate) fn render_top_tabs(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        if self.tabs.is_empty() {
            return None;
        }
        Some(
            h_flex()
                .id("top-feed-tabs")
                .gap_0p5()
                .children(self.tabs.iter().cloned().map(|s| {
                    let active = s == self.active;
                    let button = crate::pill_tab::pill_tab(
                        format!("stage-tab-{}", s.as_str()),
                        self.tab_label(&s),
                        active,
                        cx,
                    );
                    button.on_click(cx.listener(move |this, _, _, cx| {
                        this.switch_source(s.clone(), cx);
                        cx.stop_propagation();
                    }))
                }))
                .into_any_element(),
        )
    }
}
