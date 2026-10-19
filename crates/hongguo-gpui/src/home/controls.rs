//! 控制区（对齐 Tauri 版控制栏核心件）：暂停/倍速/音量/清晰度入口、
//! 进度轨道（拖动 seek 与节流）、控制条显隐、偏好落盘、引擎事件分发，
//! 以及大屏控制条渲染。

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonCustomVariant;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{Window, div};
use hongguo_playback::PlayerEvent;

use super::player_icons;
use crate::bridge;

/// 控制条强制常显时长（秒）：交互后多 linger 一段时间再交回闲置淡出。
/// 只被 screenshot 门控的取证辅助使用，随消费点一起进 feature。
#[cfg(feature = "screenshot")]
pub(super) const CONTROLS_HOLD_SECS: u64 = 600;

/// 播放器浮层钮的 hgplayer 皮肤（Tauri `IconButton` 同款）：透明底、白字、
/// hover 铺 20% 白——**颜色不跟主题走**。
///
/// 为什么非要用 `custom` 变体：`ghost` 的前景与 hover 文字色都取自主题
/// （`secondary_foreground`），亮色主题下就是深色字压在画面上（用户实测
/// 「亮色主题下都看不到按钮」）；而 gpui 的 `Button` 内部已占用 hover 层，
/// 实例 `text_color` 盖不住悬停态，只有 custom 变体能把 normal/hover/active
/// 三态颜色一次钉死，同时保留 Button 的 tooltip / 焦点语义。
pub(super) fn chrome_variant(cx: &gpui_kit::App) -> ButtonCustomVariant {
    let transparent: gpui_kit::Hsla = gpui_kit::rgba(0x00000000).into();
    ButtonCustomVariant::new(cx)
        .color(transparent)
        .foreground(gpui_kit::rgba(0xffffffe6).into())
        .hover(gpui_kit::rgba(0xffffff33).into())
        .active(gpui_kit::rgba(0xffffff4d).into())
}

/// 播放器/评论面板里的输入框：在元素**真正渲染的那一拍**，临时把主题里
/// 光标 / 占位符 / 选区 / **编辑器底色**四个 token 换成 hgplayer 皮肤值，
/// 再把 `Input` 交给组件渲染，然后立刻还原。
///
/// 为什么只能这么做：`Input` 在 `RenderOnce::render` 里从 `cx.theme()` 把
/// 这几个颜色拷进 `InputState` 的 editor style（`set_editor_style` 是
/// gpui-component 的 crate 内部 API），应用侧没有实例级覆盖入口。播放器是
/// 固定深色皮肤，亮色主题的默认值是「近黑光标 + 深灰占位符」，压在深色
/// 胶囊/面板上就是隐形。`Theme::global_mut` 不广播主题变更、不动其它字段
/// （见其注释），我们的改写与还原在同一个同步渲染调用内完成，正文色仍由
/// 调用方的 `.text_color()` 决定（正文取继承色）。
///
/// 编辑器底色必须显式透明：`appearance(false)` 只关掉 `Input` 容器的底色，
/// 而 gpui-base 的 editor 元素自己会**逐行涂 `editor_background`**（未设置
/// 时回落 `input_background()`——亮色下就是白底块，实测两种主题都穿了帮，
/// 见 feed-comments 预览）。底色由外层胶囊/输入框容器画。
#[derive(gpui_kit::IntoElement)]
pub(super) struct SkinnedInput {
    inner: gpui_kit::component::input::Input,
}

impl gpui_kit::RenderOnce for SkinnedInput {
    fn render(
        self,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::App,
    ) -> impl gpui_kit::IntoElement {
        use gpui_kit::component::Theme;
        let (caret, muted, selection, editor_bg) = {
            let theme = Theme::global_mut(cx);
            let saved = (
                theme.caret,
                theme.muted_foreground,
                theme.selection,
                theme.highlight_theme.style.editor_background,
            );
            theme.caret = gpui_kit::rgba(0xffffffff).into();
            theme.muted_foreground = gpui_kit::rgba(0xffffff99).into();
            theme.selection = gpui_kit::rgba(0xffffff40).into();
            // Arc 容器：首次经 make_mut 拆出本壳自有副本，之后就地改写。
            std::sync::Arc::make_mut(&mut theme.highlight_theme)
                .style
                .editor_background = Some(gpui_kit::rgba(0x00000000).into());
            saved
        };
        // Input::render 在这里同步读取主题并把这几个颜色拷进 InputState。
        // `into_any_element()` 把返回的元素摊成自有值：它默认会借住 `cx`
        // （RPIT 捕获入参生命周期），不摊平的话下面还原借不到可变引用。
        let element = self.inner.render(window, cx).into_any_element();
        let theme = Theme::global_mut(cx);
        theme.caret = caret;
        theme.muted_foreground = muted;
        theme.selection = selection;
        std::sync::Arc::make_mut(&mut theme.highlight_theme)
            .style
            .editor_background = editor_bg;
        element
    }
}

/// 给播放器 chrome 的输入框套上皮肤（`Input` 的 builder 照常配）。
pub(super) fn skinned_input(input: gpui_kit::component::input::Input) -> SkinnedInput {
    SkinnedInput { inner: input }
}

/// 进度拖动的拖拽载荷（`on_drag`/`on_drag_move` 按类型配对）。
struct ScrubDrag;

/// 音量竖条拖拽载荷（与进度拖动同套 on_drag 机制）。
struct VolumeDrag;

/// 拖动期间跟指针的幽灵视图：空 div（拖动不需要可视残影，预览走进度条本身）。
struct ScrubGhost;

impl gpui_kit::Render for ScrubGhost {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        div()
    }
}

impl super::HomeView {
    /// 取证辅助（screenshot feature 的 HONGGUO_SHOT_PAUSE）：暂停播放并把
    /// 控制条可见窗拉长到 10 分钟——无人值守抓帧才能拍到控制条/互动栏。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_pause_player(&mut self, cx: &mut Context<Self>) {
        if let Some(player) = &self.player
            && !player.is_paused()
        {
            player.pause();
        }
        self.controls_until = Some(Instant::now() + Duration::from_secs(CONTROLS_HOLD_SECS));
        cx.notify();
    }

    /// 取证辅助（HONGGUO_SHOT_PANEL=dm-settings）：打开弹幕设置面板并拉长
    /// 控制条可见窗（无人值守抓四行滑杆 UI）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_dm_settings(&mut self, cx: &mut Context<Self>) {
        self.dm_panel_open = true;
        self.controls_until = Some(Instant::now() + Duration::from_secs(CONTROLS_HOLD_SECS));
        cx.notify();
    }

    pub(super) fn on_player_event(&mut self, event: PlayerEvent, cx: &mut Context<Self>) {
        match event {
            PlayerEvent::FirstFrame { .. } => {
                // 帧由紧随其后的 Frame 事件带上；这里只做「出画了」的副作用
                self.prefetch_next_stream(cx);
                // 新集首帧出画 = 连播静默期结束（Tauri 首播解除静默同款）
                self.silent_until_input = false;
            }
            PlayerEvent::Frame(frame) => {
                let first = self.frame.is_none();
                if first {
                    log::info!("[Home] 首帧进入 view——video_ready 即为 true");
                }
                // 新集首帧出画：定格帧交棒（Tauri FrameHold 淡出同款）
                self.hold_last_frame = false;
                self.frame = Some(frame);
                self.save_progress_maybe();
                // 弹幕 tick：入场 + 在飞推进（Frame 事件即渲染节拍）
                self.danmaku_tick(cx);
                // 相邻剧分集档案预取（换剧秒回，Tauri 同节奏）
                self.prefetch_neighbor_archives();
            }
            // 引擎缓冲状态机：水位低暂停设备、回填恢复（详见 playback::player）
            PlayerEvent::Buffering => self.buffering = true,
            PlayerEvent::Ready => self.buffering = false,
            PlayerEvent::Ended => {
                self.buffering = false;
                self.advance_after_end(cx);
            }
            PlayerEvent::Failed(msg) => {
                self.buffering = false;
                // 失败落封面：定格帧此时只会盖住错误态（Tauri 真值表 error 档）
                self.hold_last_frame = false;
                // 引擎致命失败即自停（shutdown），封面背后不会再有画面——
                // 失败必须落可见错误态：只写 pending_title 的话，那行小徽标
                // 随 switching 消失，用户看到的就是「无声无息卡在封面」
                self.play_error = Some(crate::i18n::tf(
                    "common.failedWith",
                    &[("message", msg.as_str())],
                ));
            }
        }
        cx.notify();
    }

    /// 倍速档位（与 Tauri 版 `player-controls.tsx` 的 RATES 完全一致）。
    const RATES: [f64; 6] = [0.75, 1.0, 1.25, 1.5, 2.0, 3.0];
    /// 控制条可见时长：鼠标静止/无操作后自动隐藏。
    pub(super) const CONTROLS_VISIBLE: Duration = Duration::from_secs(3);
    /// 拖动中实时 seek 的最小间隔：引擎一次 seek = 冲流 + 解码器重建，
    /// 250ms 限频跟手又不拖垮供给（Tauri 侧浏览器自行合并，这里显式限）。
    const SCRUB_SEEK_MIN_INTERVAL: Duration = Duration::from_millis(250);

    /// 音量滑杆：拖动即生效（引擎在设备回调里乘 PCM，零延迟），松手落盘。
    /// 音量弹层开合裁决（按钮 hover ∥ 弹层 hover 取并集，见挂载处注释）。
    fn update_volume_popup(&mut self, cx: &mut Context<Self>) {
        let open = self.volume_hovered || self.volume_popup_hovered;
        if open {
            self.wake_controls(cx);
        } else {
            // 收起时落一次盘：拖动过程中的音量值别丢
            self.persist_prefs();
        }
        if self.volume_popup_open != open {
            self.volume_popup_open = open;
            cx.notify();
        }
    }

    /// 指针在音量竖条上的取值（竖向：顶部 = 1；轨道矩形来自哨兵）。
    fn volume_ratio_at(&self, y: f64) -> f32 {
        match self.volume_bounds.get() {
            Some(b) => {
                let top = b.origin.y.to_f64();
                let height = b.size.height.to_f64().max(1.0);
                (1.0 - (y - top) / height).clamp(0.0, 1.0) as f32
            }
            None => 0.0,
        }
    }

    /// 设音量（拖/点音量条；静音独立，由喇叭钮切换——Tauri 同语义）。
    fn set_volume_value(&mut self, v: f32, cx: &mut Context<Self>) {
        self.volume = v.clamp(0.0, 1.0);
        if let Some(player) = &self.player {
            player.set_volume(self.volume);
        }
        self.wake_controls(cx);
        cx.notify();
    }

    /// 竖向音量条（Tauri VerticalSlider 同款三段自绘：轨道/已填/滑块）。
    /// 组件库 Slider 的竖向形态与播放器的白色系视觉不一致，这里照抄三段。
    fn render_volume_slider(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let value = (if self.muted { 0.0 } else { self.volume }).clamp(0.0, 1.0);
        let bounds = Rc::clone(&self.volume_bounds);
        div()
            // prepaint 哨兵是 Div 的方法：必须在 .id() 变 Stateful 之前挂
            .on_children_prepainted(move |children, _, _| {
                if let Some(b) = children.first() {
                    bounds.set(Some(*b));
                }
            })
            .id("volume-slider")
            .relative()
            .cursor_pointer()
            .w(gpui_kit::px(24.0))
            .h(gpui_kit::px(96.0))
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, ev: &gpui_kit::MouseDownEvent, _, cx| {
                    let v = this.volume_ratio_at(ev.position.y.to_f64());
                    this.set_volume_value(v, cx);
                }),
            )
            .on_drag(VolumeDrag, |_, _, _, cx| cx.new(|_| ScrubGhost))
            .on_drag_move::<VolumeDrag>(cx.listener(
                |this, ev: &gpui_kit::DragMoveEvent<VolumeDrag>, _, cx| {
                    let v = this.volume_ratio_at(ev.event.position.y.to_f64());
                    this.set_volume_value(v, cx);
                },
            ))
            // 轨道（w-1 居中）
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(gpui_kit::px(10.0))
                    .w(gpui_kit::px(4.0))
                    .rounded_full()
                    .bg(gpui_kit::rgba(0xffffff40)),
            )
            // 已填（自下而上）
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left(gpui_kit::px(10.0))
                    .w(gpui_kit::px(4.0))
                    .h(gpui_kit::relative(value))
                    .rounded_full()
                    .bg(gpui_kit::rgb(0xffffff)),
            )
            // 滑块（底部锚定 value 高度，负 margin 下移半格对中线）
            .child(
                div()
                    .absolute()
                    .bottom(gpui_kit::relative(value))
                    .left(gpui_kit::px(6.0))
                    .mb(gpui_kit::px(-6.0))
                    .size_3()
                    .rounded_full()
                    .bg(gpui_kit::rgb(0xffffff)),
            )
            .into_any_element()
    }

    pub(super) fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        let Some(player) = &self.player else {
            return;
        };
        if player.is_paused() {
            player.resume();
            // 恢复播放重新持有活动断言（play_target 持有、用户暂停释放）
            crate::play_activity::set_active(true);
        } else {
            player.pause();
            // 暂停 = 用户在场，系统可以休眠；主流播放器同语义
            crate::play_activity::set_active(false);
            // 暂停 = force 落盘 + 补一条观看历史（Tauri persist(force) 同款）
            self.save_progress_now();
        }
        self.wake_controls(cx);
        cx.notify();
    }

    pub(super) fn set_rate(&mut self, rate: f64, cx: &mut Context<Self>) {
        self.rate = rate;
        if let Some(player) = &self.player {
            player.set_rate(rate);
        }
        self.persist_prefs();
        self.wake_controls(cx);
        cx.notify();
    }

    pub(super) fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        self.muted = !self.muted;
        if let Some(player) = &self.player {
            player.set_muted(self.muted);
        }
        self.persist_prefs();
        self.wake_controls(cx);
        cx.notify();
    }

    /// 偏好落盘（数据目录键值文件，见 [`crate::playback_prefs`]）。
    pub(super) fn persist_prefs(&self) {
        let prefs = crate::playback_prefs::PlaybackPrefs {
            rate: self.rate,
            volume: self.volume,
            muted: self.muted,
            danmaku_on: self.danmaku_on,
            dm_opacity: self.dm_opacity,
            dm_font_scale: self.dm_font_scale,
            dm_density: self.dm_density,
            dm_area: self.dm_area,
            incognito: self.incognito,
            keys: self.shortcut_keys.clone(),
        };
        bridge::spawn(async move {
            let path = hongguo_store::paths::data_dir().join(crate::playback_prefs::FILE_NAME);
            let _ =
                tokio::task::spawn_blocking(move || crate::playback_prefs::save_to(&path, &prefs))
                    .await;
        });
    }

    /// 鼠标活动/操作刷新控制条可见窗；隐藏→可见的沿上排一次性定时器，
    /// 静止到期后靠它（或播放中的帧事件流）触发重算隐藏。真实输入同时
    /// 解除连播静默（Tauri wakeOnInput 同款）。
    pub(super) fn wake_controls(&mut self, cx: &mut Context<Self>) {
        // 静默期闸门（Tauri wakeOnMove 同款）：连播静默起步后，「用户在场」
        // 只认真实输入（点击/按键/滚轮——它们显式走这里之外的解除路径）与
        // 首帧；指针移动不算，不然静默刚起就被一次晃动解除，悬浮层全亮
        if self.silent_until_input {
            return;
        }
        let was_hidden = !self.controls_visible();
        self.controls_until = Some(Instant::now() + Self::CONTROLS_VISIBLE);
        if was_hidden {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Self::CONTROLS_VISIBLE).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
            cx.notify();
        }
    }

    /// 真实输入的「用户在场」：解除连播静默并亮悬浮层（Tauri
    /// pointerdown/按键 wakeChrome 同款——与 [`Self::wake_controls`] 的差
    /// 别就在静默期是否放行）。点击画面、键盘、滚轮切剧走这里。
    pub(super) fn wake_on_input(&mut self, cx: &mut Context<Self>) {
        self.silent_until_input = false;
        self.wake_controls(cx);
    }

    /// 任一面板打开即常显（Tauri panelOpen 同一集合：选集/评论/弹幕设置/
    /// 音量/下载）。倍速/清晰度下拉菜单与表情网格虽不是「面板」，但开着时
    /// 指针不在控制条本体上、hover 养不了常显窗——不进来就会被帧事件流的
    /// 重算连菜单一起收掉（播放中每帧都触发，暂停态没重渲染所以显得
    /// 「只有暂停能操作」）。
    fn panel_open(&self) -> bool {
        self.picker_open
            || self.comments_panel_open
            || self.dm_panel_open
            || self.volume_popup_open
            || self.downloads_open
            || self.rate_menu_open
            || self.definition_menu_open
            || self.danmaku_emoji_open
    }

    /// 悬浮层总闸（Tauri chromeShown 同款裁决）：连播静默期一票否决（面板
    /// 照常）；否则 悬停控制栏 / 拖进度 / 暂停 / 活动窗未到期 / 面板开 任一
    /// 即亮。指针离开舞台的立即收由舞台 on_hover 清 controls_until 实现。
    pub(super) fn controls_visible(&self) -> bool {
        if self.silent_until_input {
            return self.panel_open();
        }
        self.controls_hovered
            || self.scrubbing.is_some()
            || self.player.as_ref().is_some_and(|p| p.is_paused())
            || self.controls_until.is_some_and(|t| Instant::now() < t)
            || self.panel_open()
    }

    /// 键盘相对 seek（秒，可负）：钳到 [0, 时长]，唤醒控制条。
    pub(super) fn seek_by(&mut self, secs: f64, cx: &mut Context<Self>) {
        let Some(player) = &self.player else {
            return;
        };
        let dur = player.duration_secs();
        let target = (player.position_secs() + secs).clamp(0.0, dur.max(0.0));
        player.seek(target);
        self.wake_controls(cx);
        cx.notify();
    }

    /// 进度轨道的指针位置 → 目标秒。轨道矩形缺失（首帧前）返回 `None`。
    fn scrub_target_at(&self, x: gpui_kit::Pixels) -> Option<f64> {
        let bounds = self.progress_bounds.get()?;
        let player = self.player.as_ref()?;
        let dur = player.duration_secs();
        if dur <= 0.0 {
            return None;
        }
        let ratio = ((x.to_f64() - bounds.origin.x.to_f64()) / bounds.size.width.to_f64().max(1.0))
            .clamp(0.0, 1.0);
        Some(ratio * dur)
    }

    /// 收口拖动：松手（轨道内/外都算）把预览位置交给引擎。松手的这次是
    /// 权威定位，**不受节流**——节流窗口内也要照发。
    fn commit_scrub(&mut self, cx: &mut Context<Self>) {
        if let Some(target) = self.scrubbing.take() {
            self.scrub_seek_now(target, cx);
        }
        self.wake_controls(cx);
        cx.notify();
    }

    /// 立即定位（不受节流）：按下即跳与松手收口共用。
    fn scrub_seek_now(&mut self, target: f64, cx: &mut Context<Self>) {
        self.last_scrub_seek.set(Instant::now());
        if let Some(player) = &self.player {
            player.seek(target);
        }
        self.wake_controls(cx);
        cx.notify();
    }

    /// 拖动中的实时定位（对齐 Tauri ScrubBar 的 pointermove 即 seek）。节流
    /// 窗口内的重复请求直接跳过——预览位置由 `scrubbing` 承载，不受影响；
    /// 松手收口走 [`Self::commit_scrub`]，不受此限。
    fn scrub_seek_throttled(&mut self, target: f64, cx: &mut Context<Self>) {
        if self.last_scrub_seek.get().elapsed() < Self::SCRUB_SEEK_MIN_INTERVAL {
            return;
        }
        self.last_scrub_seek.set(Instant::now());
        if let Some(player) = &self.player {
            player.seek(target);
        }
        self.wake_controls(cx);
        cx.notify();
    }

    /// 隐身模式开关（Tauri incognito 同语义）：开 = 起光标轮询，鼠标脱离
    /// 窗口即暂停 + 藏窗，回到窗口矩形原地重现 + 续播；关 = 停轮询并带回
    /// 窗口。实现见 [`crate::incognito`]。
    pub(super) fn toggle_incognito(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        self.set_incognito(!self.incognito, window, cx);
    }

    /// 播放控制条（底部悬浮）：暂停/继续、进度（拖动 seek）、倍速菜单、
    /// 弹幕发送框与开关、清晰度、选集、音量。
    pub(super) fn render_controls(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let paused = self.player.as_ref().is_some_and(|p| p.is_paused());
        let duration = self
            .player
            .as_ref()
            .map(|p| p.duration_secs())
            .unwrap_or(0.0);
        let position = self.scrubbing.unwrap_or_else(|| {
            self.player
                .as_ref()
                .map(|p| p.position_secs())
                .unwrap_or(0.0)
        });
        let play_icon = if paused {
            IconName::Play
        } else {
            IconName::Pause
        };
        // 倍速菜单的点击回调拿不到 view 的 Context，经实体句柄回写
        let view = cx.entity();
        // 开态回写专用句柄：rate_menu 闭包会把 view 按值收进各 item
        let view_open = view.clone();
        let rate = self.rate;
        let rate_menu = move |menu: gpui_kit::component::menu::PopupMenu,
                              _window: &mut Window,
                              _cx: &mut Context<gpui_kit::component::menu::PopupMenu>|
              -> gpui_kit::component::menu::PopupMenu {
            let mut menu = menu.label(crate::i18n::t("player.playbackRate"));
            for r in Self::RATES {
                let view = view.clone();
                menu = menu.item(
                    PopupMenuItem::new(Self::fmt_rate(r))
                        .checked(r == rate)
                        .on_click(move |_, _window, cx| {
                            view.update(cx, |v, cx| v.set_rate(r, cx));
                        }),
                );
            }
            menu
        };
        v_flex()
            .id("stage-controls")
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            // 纯悬浮（Tauri PlayerControls 同款）：**不要底板/渐变蒙版**，
            // 一组白色控件压在画面上，可读性靠白 + 画面暗部——底板一加就是
            // 一条色块，破坏画面
            // 悬在控制栏本体上不倒计时（Tauri onControlsEnter 同款：拖进度/
            // 悬停音量时静止超时收起 = 抢走控件）；移出照常走台内静止 3s
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.controls_hovered = *hovered;
                if *hovered {
                    this.wake_controls(cx);
                }
                cx.notify();
            }))
            // 控制条悬浮在舞台之上：点击这里的任何控件都**不能**再落到舞台
            //（gpui 对重叠 bounds 的兄弟分支都会派发鼠标事件，不拦的话点
            // 进度条/按钮会同时触发舞台的点击暂停——「点了进度条却暂停了」
            // 的来源；Tauri 里控件层天然挡住指针，这里必须显式拦）。
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|_, _: &gpui_kit::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                }),
            )
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|_, _: &gpui_kit::MouseUpEvent, _, cx| {
                    cx.stop_propagation();
                }),
            )
            // 滚轮同拦（Tauri 控制条 data-wheel-block 同款）：悬在控制栏上
            // 滚不该把剧切了
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            // 容器内边距（Tauri PlayerControls 同款）：px-4 左右 16、pt-10
            // 上方 40 悬停缓冲（热区高过进度条，好上手）、pb-3 底 12；行间
            // gap-2 = 8
            .px_4()
            .pt_10()
            .pb_3()
            .gap_2()
            // 进度条横贯控制栏顶边（Tauri PlayerControls 同款：细条独立一条
            // 在按钮行之上，不夹在按钮中间）
            .child(self.render_progress_track(cx))
            .child(
                h_flex()
                    .id("stage-controls-row")
                    .items_center()
                    // 组内间距 4px（Tauri gap-1 同款；时间读数/发送框另有 ml
                    // 微调，见各自 child）
                    .gap_1()
                    .child(
                        Button::new("stage-toggle-play")
                            .icon(play_icon)
                            .custom(chrome_variant(cx))
                            .cursor_pointer()
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx))),
                    )
                    .child(
                        // 上一集/下一集（Tauri 控制栏同位）：连播语义——越过
                        // 第 1 集原地不动，越过末集跟随换剧
                        Button::new("stage-prev-episode")
                            .icon(gpui_kit::assets::IconName::SkipBack)
                            .custom(chrome_variant(cx))
                            .cursor_pointer()
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| this.step_episode(-1, cx))),
                    )
                    .child(
                        Button::new("stage-next-episode")
                            .icon(gpui_kit::assets::IconName::SkipForward)
                            .custom(chrome_variant(cx))
                            .cursor_pointer()
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| this.step_episode(1, cx))),
                    )
                    .child(
                        // 时间读数（Tauri ml-1 微调；mm:ss 分钟补零）
                        div()
                            .ml_1()
                            .text_xs()
                            .font_family("monospace")
                            .text_color(gpui_kit::rgba(0xffffffcc))
                            .child(format!(
                                "{} / {}",
                                Self::fmt_secs(position),
                                Self::fmt_secs(duration)
                            )),
                    )
                    .child(
                        // 弹幕发送 pill（Tauri 同款）：无框输入 + 内嵌表情 +
                        // 发送；表情弹层从 pill 上方拉起（与评论面板同源
                        // 表情表，光标处插入 [名字]，上屏由弹幕渲染分段画）。
                        // ml-2 与时间读数的间距（Tauri 同款微调）
                        div()
                            .id("danmaku-box")
                            .ml_2()
                            .relative()
                            .child({
                                // 弹幕输入：hgplayer 固定深色皮肤（Tauri 同款，
                                // 不跟主题走）——15% 白底胶囊、白字、白光标、
                                // 白 60% 占位符。光标/占位符/选区靠
                                // [`skinned_input`] 在渲染当期改主题 token。
                                let ink: gpui_kit::Hsla = gpui_kit::rgba(0xffffffff).into();
                                let emoji_active = ink;
                                let emoji_idle: gpui_kit::Hsla = gpui_kit::rgba(0xffffff99).into();
                                h_flex()
                                    .items_center()
                                    .gap_1()
                                    .pl_3()
                                    .pr_1()
                                    .h(gpui_kit::px(32.0))
                                    .rounded_full()
                                    .bg(gpui_kit::rgba(0xffffff26))
                                    .children(self.danmaku_input.as_ref().map(|input| {
                                        // 透明输入（Tauri inputClassName 的
                                        // `bg-transparent`）：`appearance(false)`
                                        // 才不吃主题底色——否则亮色主题下胶囊里
                                        // 会嵌一块白底方块（用户实测截图）。
                                        skinned_input(
                                            Input::new(input)
                                                .appearance(false)
                                                .bordered(false)
                                                // Tauri inputClassName: h-full
                                                // flex-1 text-xs leading-8 ——
                                                // 撑满 32px 胶囊
                                                .h_full()
                                                // Tauri w-44 = 176px
                                                .w(gpui_kit::px(176.0))
                                                .text_xs()
                                                .text_color(ink),
                                        )
                                    }))
                                    .child({
                                        // 表情入口（Tauri EmojiPickerButton 同款）：
                                        // 裸图标钮（16px + px-1），不占按钮底板——
                                        // 原先的 ghost 钮是 28px 方底板，胶囊里
                                        // 顶得比 Tauri 高一倍，输入区被挤窄
                                        div()
                                            .id("stage-danmaku-emoji")
                                            .px_1()
                                            .cursor_pointer()
                                            .role(gpui_kit::Role::Button)
                                            .aria_label(crate::i18n::t("player.danmakuEmoji"))
                                            .text_color(if self.danmaku_emoji_open {
                                                emoji_active
                                            } else {
                                                emoji_idle
                                            })
                                            .hover(|s| s.text_color(emoji_active))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.danmaku_emoji_open = !this.danmaku_emoji_open;
                                                cx.notify();
                                            }))
                                            .child(
                                                // 颜色显式给：`Icon` 是 RenderOnce，
                                                // 靠父级 text_color 继承不可靠（同仓
                                                // 互动栏图标全部显式传色）
                                                Icon::new(gpui_kit::assets::IconName::FaceGrinning)
                                                    .size_4()
                                                    .text_color(if self.danmaku_emoji_open {
                                                        emoji_active
                                                    } else {
                                                        emoji_idle
                                                    }),
                                            )
                                    })
                                    .child({
                                        // 发送钮（Tauri EmojiSendBox 同款）：primary
                                        // 胶囊、h-6(24px) / px-2.5 / text-xs。
                                        // 空文本/发送中**不用 `disabled()`**：组件
                                        // 的禁用前景取主题 muted（亮色主题下深色字
                                        // 压在深色胶囊上等于隐形），改用 Tauri 的
                                        // `disabled:opacity-50` 视觉；点击由
                                        // `submit_danmaku` 内部护栏吞掉。
                                        let empty = self
                                            .danmaku_input
                                            .as_ref()
                                            .is_none_or(|i| i.read(cx).value().trim().is_empty());
                                        Button::new("stage-danmaku-send")
                                            .label(crate::i18n::t("player.interact.send"))
                                            .primary()
                                            .small()
                                            .h(gpui_kit::px(24.0))
                                            .rounded_full()
                                            .cursor_pointer()
                                            .opacity(if empty || self.danmaku_sending {
                                                0.5
                                            } else {
                                                1.0
                                            })
                                            .on_click(
                                                cx.listener(|this, _, _, cx| {
                                                    this.submit_danmaku(cx)
                                                }),
                                            )
                                    })
                            })
                            .children(self.danmaku_emoji_open.then(|| {
                                // 表情网格（评论面板同款：连选不关面板、光标处
                                // 插入不夺焦；点外部 on_mouse_down_out 收起）
                                let danmaku_input = self.danmaku_input.clone();
                                v_flex()
                                    .absolute()
                                    .left_0()
                                    .bottom(gpui_kit::px(38.0))
                                    .w(gpui_kit::px(264.0))
                                    .p_2()
                                    .rounded(gpui_kit::px(12.0))
                                    .bg(gpui_kit::rgba(0x000000d9))
                                    .border_1()
                                    .border_color(gpui_kit::rgba(0xffffff14))
                                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                        this.danmaku_emoji_open = false;
                                        cx.notify();
                                    }))
                                    // 滚轮拦下（Tauri emoji-picker data-wheel-block
                                    // 同款）：表情网格上滚不切剧
                                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                                    .child(
                                        div()
                                            .id("danmaku-emoji-grid")
                                            .max_h(gpui_kit::px(176.0))
                                            .overflow_y_scroll()
                                            .flex()
                                            .flex_wrap()
                                            .gap_0p5()
                                            .children(
                                                crate::danmaku_emoji::EMOJI_FILES.iter().map(
                                                    |(name, _)| {
                                                        let code = name.to_string();
                                                        let emoji =
                                                            crate::danmaku_emoji::emoji_by_name(
                                                                name,
                                                            );
                                                        div()
                                                            .id(gpui_kit::ElementId::from(format!(
                                                                "dm-emoji-{}",
                                                                code
                                                            )))
                                                            .size_8()
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .rounded(gpui_kit::px(6.0))
                                                            .cursor_pointer()
                                                            .hover(|s| {
                                                                s.bg(gpui_kit::rgba(0xffffff26))
                                                            })
                                                            .on_click({
                                                                let input = danmaku_input.clone();
                                                                move |_, window, cx| {
                                                                    if let Some(input) = &input {
                                                                        // 光标处插入 [名字]
                                                                        input.update(
                                                                            cx,
                                                                            |state, cx| {
                                                                                state.insert(
                                                                                    code.clone(),
                                                                                    window,
                                                                                    cx,
                                                                                );
                                                                            },
                                                                        );
                                                                    }
                                                                }
                                                            })
                                                            .children(emoji.map(|bitmap| {
                                                                gpui_kit::img(
                                                                    gpui_kit::ImageSource::Render(
                                                                        bitmap,
                                                                    ),
                                                                )
                                                                .size_6()
                                                            }))
                                                    },
                                                ),
                                            ),
                                    )
                                    .into_any_element()
                            })),
                    )
                    // 左右分组撑杆（Tauri 右组 ml-auto 同款）：倍速起到全屏
                    // 止的右组贴控制栏右缘，不与左组顺排
                    .child(div().flex_1())
                    .child(
                        Button::new("stage-rate")
                            .icon(gpui_kit::assets::IconName::Gauge)
                            .label(Self::fmt_rate(self.rate))
                            .custom(chrome_variant(cx))
                            .cursor_pointer()
                            .small()
                            // 默认 TopLeft 会把弹层盖在按钮上；BottomLeft =
                            // 按钮上方弹出、左缘对齐（用户定稿口径）
                            .dropdown_menu_with_anchor(gpui_kit::Anchor::BottomLeft, rate_menu)
                            // 菜单开态回写视图：开着期间控制条常显（panel_open），
                            // 不然 3s 一到菜单跟着控制条一起被收走
                            .on_open_change({
                                let view = view_open.clone();
                                move |open, _, cx| {
                                    view.update(cx, |v, cx| {
                                        v.rate_menu_open = *open;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child({
                        // 清晰度菜单（Tauri 同款）：自动 + 本集全部档位；本地集
                        // 没有档位表，按钮置灰并保持可见（不渲染会让用户以为
                        // 「这个功能本来就没有」，在线流那集它又出现了）
                        let defs = self
                            .now
                            .as_ref()
                            .map(|n| n.definitions.clone())
                            .unwrap_or_default();
                        let current = self.now.as_ref().map(|n| n.definition).unwrap_or(0);
                        let label = match self.now.as_ref() {
                            Some(n) if n.definition > 0 => format!("{}P", n.definition),
                            _ => crate::i18n::t("player.definitionAuto"),
                        };
                        if defs.is_empty() {
                            // 无档位表（本地集）：禁用钮保留 + 悬停说明（Tauri
                            // title 同语义）——直接不渲染会让用户以为「这个功能
                            // 本来就没有」，在线流那集它又出现了
                            Button::new("stage-definition")
                                .icon(gpui_kit::assets::IconName::MonitorPlay)
                                .label(label)
                                .custom(chrome_variant(cx))
                                .cursor_pointer()
                                .small()
                                .disabled(true)
                                .tooltip(crate::i18n::t("player.definitionLocalOnly"))
                                .into_any_element()
                        } else {
                            let view = cx.entity();
                            let view_open = view.clone();
                            Button::new("stage-definition")
                                .icon(gpui_kit::assets::IconName::MonitorPlay)
                                .label(label)
                                .custom(chrome_variant(cx))
                                .cursor_pointer()
                                .small()
                                // 同倍速菜单：上方弹出、左缘对齐（不盖按钮）
                                .dropdown_menu_with_anchor(
                                    gpui_kit::Anchor::BottomLeft,
                                    move |menu, _window, _cx| {
                                        let mut menu =
                                            menu.label(crate::i18n::t("player.definition"));
                                        menu = menu.item(
                                            PopupMenuItem::new(crate::i18n::t(
                                                "player.definitionAuto",
                                            ))
                                            .checked(current == 0)
                                            .on_click({
                                                let view = view.clone();
                                                move |_, _, cx| {
                                                    view.update(cx, |v, cx| {
                                                        v.switch_definition(None, cx)
                                                    })
                                                }
                                            }),
                                        );
                                        for d in &defs {
                                            let view = view.clone();
                                            let value = d.value;
                                            // 竖屏剧的宽高是反的，只报分辨率会误导
                                            let lbl = if d.height > d.width {
                                                format!("{}P（{}×{}）", d.value, d.width, d.height)
                                            } else {
                                                format!("{}P", d.value)
                                            };
                                            menu = menu.item(
                                                PopupMenuItem::new(lbl)
                                                    .checked(d.value == current)
                                                    .on_click(move |_, _, cx| {
                                                        let view = view.clone();
                                                        view.update(cx, |v, cx| {
                                                            v.switch_definition(Some(value), cx)
                                                        })
                                                    }),
                                            );
                                        }
                                        menu
                                    },
                                )
                                // 菜单开态回写视图（倍速菜单同款）：开着常显。
                                // 挂在 dropdown_menu 返回的 Popover 上，不是 Button
                                .on_open_change({
                                    let view = view_open.clone();
                                    move |open, _, cx| {
                                        view.update(cx, |v, cx| {
                                            v.definition_menu_open = *open;
                                            cx.notify();
                                        });
                                    }
                                })
                                .into_any_element()
                        }
                    })
                    .child(
                        // 选集浮层入口（Tauri immersive 同款：ListVideo 图标 +
                        // 总集数；浮层**贴按钮**向上弹——Tauri 的 EpisodePicker
                        // 就挂在按钮 wrapper 里，右缘与按钮对齐）
                        div()
                            .id("stage-picker-wrap")
                            .relative()
                            .child(
                                Button::new("stage-picker")
                                    .icon(gpui_kit::assets::IconName::ListVideo)
                                    .label(match self.now.as_ref() {
                                        Some(now) if now.episode_count > 0 => {
                                            format!(
                                                "{} · {}",
                                                crate::i18n::t("player.episodes"),
                                                crate::i18n::tf(
                                                    "player.totalEpisodes",
                                                    &[("count", &now.episode_count.to_string())],
                                                )
                                            )
                                        }
                                        _ => crate::i18n::t("player.episodes"),
                                    })
                                    .custom(chrome_variant(cx))
                                    .cursor_pointer()
                                    .small()
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_picker(cx))),
                            )
                            .children(self.picker_open.then(|| {
                                // deferred：后画的兄弟钮（下载/全屏…）不许盖住浮层
                                gpui_kit::deferred(self.render_picker(cx)).with_priority(1)
                            })),
                    )
                    .child(
                        // 下载入口（Tauri download-sheet 同款位置：右段）
                        Button::new("stage-downloads")
                            .icon(gpui_kit::assets::IconName::Download)
                            .label(crate::i18n::t("player.download"))
                            .custom(chrome_variant(cx))
                            .cursor_pointer()
                            .small()
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.toggle_downloads(window, cx)
                                }),
                            ),
                    )
                    .child(
                        // 小屏播放（Tauri onOpenMini 同款）：**同一个窗口**缩成
                        // 480×270 落工作区右下角，播放零中断
                        Button::new("stage-mini")
                            .icon(player_icons::mini_window())
                            .custom(chrome_variant(cx))
                            .cursor_pointer()
                            .small()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.enter_mini(window, cx);
                            })),
                    )
                    .child({
                        // 弹幕设置：B 站同款「弹」字方框 + 齿轮角标（自绘，见
                        // `player_icons`）+ 上方面板（透明度/字号/密度/显示区域）。
                        // 开合 = 点它（Tauri 同款点击语义），点外部由舞台捕获层收
                        // （picker 同款，见 stage 层）——原先的 hover 并集开合指针
                        // 一晃就收，滑杆根本没法拖。
                        // 与弹幕开关同一套自绘钮（Tauri IconButton 同款）：透明底、
                        // 开=纯白、关=70% 白，hover 铺 20% 白——组件 `Button` 的
                        // 自绘图标塞不进去（.icon 只收 Icon），且 `open` 态会带一块
                        // 常驻底色，与 Tauri 真机并排对拍不一致。
                        div()
                            .id("dm-settings")
                            .relative()
                            .child({
                                let color = if self.dm_panel_open {
                                    gpui_kit::rgba(0xffffffff)
                                } else {
                                    gpui_kit::rgba(0xffffffb3)
                                };
                                div()
                                    .id("stage-dm-settings")
                                    .flex()
                                    .w(gpui_kit::px(32.0))
                                    .h(gpui_kit::px(28.0))
                                    .justify_center()
                                    .items_center()
                                    .rounded(gpui_kit::px(6.0))
                                    .cursor_pointer()
                                    .role(gpui_kit::Role::Button)
                                    .aria_label(crate::i18n::t("player.danmakuSettings"))
                                    .text_color(color)
                                    .hover(|s| s.bg(gpui_kit::rgba(0xffffff33)))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.dm_panel_open = !this.dm_panel_open;
                                        cx.notify();
                                    }))
                                    .child(player_icons::danmaku_settings(color))
                            })
                            .children(self.dm_panel_open.then(|| {
                                // Tauri 面板几何/皮肤同款：w-60(240) / p-4 / gap-4，
                                // 黑 85% 底 + 白 10% 描边（此前 pt-3 pb-4 gap-3、
                                // 无描边，行距比 Tauri 紧一档）
                                v_flex()
                                    .id("dm-settings-panel")
                                    .absolute()
                                    .right_0()
                                    // 贴着按钮上沿（bottom-full）再抬 12px
                                    //（Tauri 面板 mb-3 同款留白）
                                    .bottom(gpui_kit::relative(1.0))
                                    .mb_3()
                                    .w(gpui_kit::px(240.0))
                                    .gap_4()
                                    .p_4()
                                    .rounded(gpui_kit::px(12.0))
                                    .bg(gpui_kit::rgba(0x000000d9))
                                    .border_1()
                                    .border_color(gpui_kit::rgba(0xffffff1a))
                                    .on_mouse_down(
                                        gpui_kit::MouseButton::Left,
                                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                                    )
                                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                                    .child(self.dm_setting_row(
                                        crate::i18n::t("player.danmakuOpacity"),
                                        &self.dm_opacity_slider,
                                        self.dm_opacity,
                                    ))
                                    .child(self.dm_setting_row(
                                        // Tauri 面板同文案（「字体大小」）
                                        crate::i18n::t("player.danmakuFontSize"),
                                        &self.dm_font_slider,
                                        self.dm_font_scale,
                                    ))
                                    .child(self.dm_setting_row(
                                        crate::i18n::t("player.danmakuDensity"),
                                        &self.dm_density_slider,
                                        self.dm_density,
                                    ))
                                    .child(self.dm_setting_row(
                                        crate::i18n::t("player.danmakuArea"),
                                        &self.dm_area_slider,
                                        self.dm_area,
                                    ))
                                    .into_any_element()
                            }))
                            .into_any_element()
                    })
                    .child({
                        // 弹幕开关（B 站同款「弹」字方框，关=斜杠+压暗）。
                        // 自绘开关钮（Tauri IconButton 同款）：透明底、开=纯白、
                        // 关=40% 白，hover 铺 20% 白。组件 `selected` 态会带一块
                        // 常驻底色，与 Tauri 真机不一致（并排对拍）；自绘图标的
                        // 颜色也拿不到组件内部的 hover/text 通道，所以整钮自绘。
                        let color = if self.danmaku_on {
                            gpui_kit::rgba(0xffffffff)
                        } else {
                            gpui_kit::rgba(0xffffff66)
                        };
                        div()
                            .id("stage-danmaku")
                            .flex()
                            .w(gpui_kit::px(32.0))
                            .h(gpui_kit::px(28.0))
                            .justify_center()
                            .items_center()
                            .rounded(gpui_kit::px(6.0))
                            .cursor_pointer()
                            .role(gpui_kit::Role::Button)
                            .aria_label(crate::i18n::t("player.danmaku"))
                            .text_color(color)
                            .hover(|s| s.bg(gpui_kit::rgba(0xffffff33)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.danmaku_on = !this.danmaku_on;
                                // 关=清屏+游标落当前位（重开不补放积压）；落盘
                                this.danmaku_flights.clear();
                                this.danmaku_cursor = this
                                    .player
                                    .as_ref()
                                    .map(|p| {
                                        this.danmaku_items.partition_point(|d| {
                                            (d.offset_ms as f64) / 1000.0 <= p.position_secs()
                                        })
                                    })
                                    .unwrap_or(0);
                                this.persist_prefs();
                                cx.notify();
                            }))
                            .child(player_icons::danmaku_switch(self.danmaku_on, color))
                    })
                    .child({
                        // 音量（Tauri VolumePopup 同款）：图标按钮单击=静音切换；
                        // 悬停向上弹竖向滑杆，弹层与按钮几何贴合，按钮/弹层两
                        // 个 hover 取并集裁决开合（间隙 = 移过去就收的元凶）
                        div()
                            .id("volume-wrapper")
                            .relative()
                            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                this.volume_hovered = *hovered;
                                this.update_volume_popup(cx);
                            }))
                            .child(
                                Button::new("stage-mute")
                                    .icon(if self.muted || self.volume == 0.0 {
                                        gpui_kit::assets::IconName::VolumeX
                                    } else if self.volume < 0.5 {
                                        gpui_kit::assets::IconName::Volume1
                                    } else {
                                        gpui_kit::assets::IconName::Volume2
                                    })
                                    .custom(chrome_variant(cx))
                                    .cursor_pointer()
                                    .small()
                                    .tooltip(crate::i18n::t("player.mute"))
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))),
                            )
                            .children(self.volume_popup_open.then(|| {
                                v_flex()
                                    .id("volume-popup")
                                    .absolute()
                                    // 贴合按钮上沿（bottom-full），视觉留白进
                                    // 自己的 padding
                                    .bottom(gpui_kit::relative(1.0))
                                    .left(gpui_kit::relative(0.5))
                                    .ml(gpui_kit::px(-30.0))
                                    .items_center()
                                    .pt_1()
                                    .pb_3()
                                    .px_3()
                                    .rounded(gpui_kit::px(8.0))
                                    .bg(gpui_kit::rgba(0x000000cc))
                                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                        this.volume_popup_hovered = *hovered;
                                        this.update_volume_popup(cx);
                                    }))
                                    .on_mouse_down(
                                        gpui_kit::MouseButton::Left,
                                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                                    )
                                    .on_mouse_up(
                                        gpui_kit::MouseButton::Left,
                                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                                    )
                                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                                    // 百分比读数（Tauri VolumePopup 同款）
                                    .child(
                                        div()
                                            .mb_1()
                                            .text_size(gpui_kit::px(10.0))
                                            .font_family("monospace")
                                            .text_color(gpui_kit::rgba(0xffffffcc))
                                            .child(format!(
                                                "{}",
                                                ((if self.muted { 0.0 } else { self.volume })
                                                    * 100.0)
                                                    .round()
                                                    as i64
                                            )),
                                    )
                                    .child(self.render_volume_slider(cx))
                                    .into_any_element()
                            }))
                    })
                    .child(
                        // 隐身模式（Tauri incognito 同款）：开着时光标脱离窗口 →
                        // 暂停 + 整窗藏起，回到窗口矩形 → 原地重现 + 续播。
                        // 亮暗态走 `selected` 语义态（弹幕开关同款，悬停免疫）
                        Button::new("stage-incognito")
                            .icon(gpui_kit::assets::IconName::Eye)
                            .custom(chrome_variant(cx))
                            .cursor_pointer()
                            .small()
                            .selected(self.incognito)
                            .tooltip(if self.incognito {
                                crate::i18n::t("player.exitIncognito")
                            } else {
                                crate::i18n::t("player.incognito")
                            })
                            .text_color(if self.incognito {
                                gpui_kit::rgba(0xffffffff)
                            } else {
                                gpui_kit::rgba(0xffffff8c)
                            })
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.toggle_incognito(window, cx)
                                }),
                            ),
                    )
                    // 置顶入口已上移壳顶栏（Tauri 同址：顶栏钉住钮 + 小屏
                    // 紧凑条；正常播放的控制栏没有这一项）
                    .child(
                        // 竖屏播放（Tauri 控制栏竖屏钮同位）：窗口收窄成按视频
                        // 宽高比算宽的竖条，竖屏视频刚好包住画面不留两侧黑边。
                        // 竖屏中的控件换成紧凑条竖屏形态，这里只做「进入」入口
                        Button::new("stage-portrait")
                            .icon(gpui_kit::assets::IconName::RectangleVertical)
                            .custom(chrome_variant(cx))
                            .cursor_pointer()
                            .small()
                            .tooltip(crate::i18n::t("player.portrait"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_portrait(window, cx);
                            })),
                    )
                    .child(
                        // 无边框播放（Tauri 控制栏同位新增，issue #11）：chrome 全藏
                        // 但不进 OS 全屏，窗口保持原几何；macOS 红绿灯一并藏。
                        // 与全屏互斥（toggle 内部裁决）。
                        // 状态可见：图标亮/暗跟 `frameless` 走（Tauri
                        // `frameless ? 'text-white' : 'text-white/70'` 同款）——
                        // 之前两态一个样，用户只能从「顶栏还在不在」猜。
                        Button::new("stage-frameless")
                            .icon(player_icons::web_fullscreen())
                            .custom(chrome_variant(cx))
                            .cursor_pointer()
                            .small()
                            .text_color(if self.frameless {
                                gpui_kit::rgba(0xffffffff)
                            } else {
                                gpui_kit::rgba(0xffffffb3)
                            })
                            .tooltip(crate::i18n::t(if self.frameless {
                                "player.exitFrameless"
                            } else {
                                "player.frameless"
                            }))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_frameless(window, cx);
                            })),
                    )
                    .child(
                        // 播放器全屏（真沉浸：OS 全屏 + chrome 全藏；顶栏最大化钮是
                        // 另一档「应用全屏」chrome 保留）；图标随状态换
                        Button::new("stage-fullscreen")
                            .icon(player_icons::fullscreen(self.fullscreen))
                            .custom(chrome_variant(cx))
                            .cursor_pointer()
                            .small()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_immersive_fullscreen(window, cx);
                            })),
                    )
                    .into_any_element(),
            )
            .into_any_element()
    }

    /// 进度轨道（自绘）：细条填充按播放位置实时走（帧事件驱动重渲染）；
    /// 悬停加粗并亮出滑块圆点（Tauri ScrubBar 同款）；点击即跳转，按住
    /// 拖动为预览（时间读数跟手），松手才 seek。轨道矩形经 prepainted
    /// 哨兵存取，拖动事件只带窗口坐标。
    pub(super) fn render_progress_track(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let duration = self
            .player
            .as_ref()
            .map(|p| p.duration_secs())
            .unwrap_or(0.0);
        // 显示位置钳在时长内：挂钟收尾时钟会轻微越过结尾（~1s），不钳的
        // 话时间读数会走出「043/0:59 → 1:01/0:59」这种对不上的观感
        let position = self
            .scrubbing
            .unwrap_or_else(|| {
                self.player
                    .as_ref()
                    .map(|p| p.position_secs())
                    .unwrap_or(0.0)
            })
            .min(duration.max(0.0));
        let frac = if duration > 0.0 {
            (position / duration).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let bounds = Rc::clone(&self.progress_bounds);
        let hovered = self.progress_hovered;
        h_flex()
            .w_full()
            .items_center()
            .cursor_pointer()
            // 命中区加高（好点），视觉条只有 4px
            .h_4()
            .relative()
            .child(
                div()
                    .relative()
                    .w_full()
                    // 静止态 2px 半透明细条贴着画面，悬停/拖动涨到 4px 并
                    // 提亮（ScrubBar 同款「平时隐身、上手好用」）
                    .when(hovered, |s| s.h_1())
                    .when(!hovered, |s| s.h(gpui_kit::px(2.0)))
                    .rounded_full()
                    .bg(gpui_kit::rgba(0xffffff33))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .when(hovered, |s| s.h_1())
                            .when(!hovered, |s| s.h(gpui_kit::px(2.0)))
                            .w(gpui_kit::relative(frac as f32))
                            .rounded_full()
                            .bg(gpui_kit::rgba(if hovered {
                                0xffffffe6
                            } else {
                                0xffffff99
                            })),
                    )
                    // 滑块圆点（Tauri ScrubBar 同款：悬停轨道才亮）
                    .children(hovered.then(|| {
                        div()
                            .absolute()
                            .top(gpui_kit::px(-4.0))
                            .left(gpui_kit::relative(frac as f32))
                            .ml(gpui_kit::px(-6.0))
                            .size_3()
                            .rounded_full()
                            .bg(gpui_kit::rgb(0xffffff))
                    })),
            )
            // prepainted 哨兵是 Div 的方法：必须在 .id() 变 Stateful 之前挂
            .on_children_prepainted(move |children, _window, _cx| {
                // 第一个子层是视觉条；矩形存哨兵供指针位置换算
                if let Some(b) = children.first() {
                    bounds.set(Some(*b));
                }
            })
            .id("stage-progress-track")
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if this.progress_hovered != *hovered {
                    this.progress_hovered = *hovered;
                    cx.notify();
                }
            }))
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, ev: &gpui_kit::MouseDownEvent, _window, cx| {
                    // 对齐 Tauri ScrubBar 的 pointerdown：按下即定位（不是只
                    // 起预览），随后拖动持续跟手
                    if let Some(target) = this.scrub_target_at(ev.position.x) {
                        this.scrubbing = Some(target);
                        this.scrub_seek_now(target, cx);
                    }
                }),
            )
            .on_drag(ScrubDrag, |_, _, _, cx| cx.new(|_| ScrubGhost))
            .on_drag_move::<ScrubDrag>(cx.listener(
                |this, ev: &gpui_kit::DragMoveEvent<ScrubDrag>, _window, cx| {
                    if let Some(target) = this.scrub_target_at(ev.event.position.x) {
                        this.scrubbing = Some(target);
                        // 实时 scrub：拖到哪放到哪（引擎侧节流限频）
                        this.scrub_seek_throttled(target, cx);
                    }
                },
            ))
            // 松手收口：轨道内/外都要算（拖出轨道再松手也是一次明确的定位）
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _: &gpui_kit::MouseUpEvent, _window, cx| {
                    this.commit_scrub(cx);
                }),
            )
            .on_mouse_up_out(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _: &gpui_kit::MouseUpEvent, _window, cx| {
                    this.commit_scrub(cx);
                }),
            )
            .into_any_element()
    }
}
