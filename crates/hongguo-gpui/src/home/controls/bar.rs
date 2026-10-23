//! 大屏控制条：进度条 + 一行按钮（播放/切集/时间/弹幕/倍速/清晰度/选集/
//! 下载/小屏/设置/音量/隐身/竖屏/无边框/全屏）。

use super::*;

use gpui_kit::component::IconName;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::div;

use crate::home::player_icons;

impl HomeView {
    /// 播放控制条（底部悬浮）：暂停/继续、进度（拖动 seek）、倍速菜单、
    /// 弹幕发送框与开关、清晰度、选集、音量。
    pub(in crate::home) fn render_controls(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
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
                            .flex_shrink_0()
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
                            .flex_shrink_0()
                            .on_click(cx.listener(|this, _, _, cx| this.step_episode(-1, cx))),
                    )
                    .child(
                        Button::new("stage-next-episode")
                            .icon(gpui_kit::assets::IconName::SkipForward)
                            .custom(chrome_variant(cx))
                            .cursor_pointer()
                            .small()
                            .flex_shrink_0()
                            .on_click(cx.listener(|this, _, _, cx| this.step_episode(1, cx))),
                    )
                    // 时间读数（Tauri ml-1 微调；mm:ss 分钟补零）。紧凑档
                    // （窄舞台）收起：读数不是图标，手机端同样没有常驻时间，
                    // 这里是「图标一个不裁」要的最后一个让位方。
                    .children((!self.controls_compact()).then(|| {
                        div()
                            .ml_1()
                            .flex_shrink_0()
                            .text_xs()
                            .font_family("monospace")
                            .text_color(gpui_kit::rgba(0xffffffcc))
                            .child(format!(
                                "{} / {}",
                                Self::fmt_secs(position),
                                Self::fmt_secs(duration)
                            ))
                            .into_any_element()
                    }))
                    .child(self.render_danmaku_box(cx))
                    // 左右分组撑杆（Tauri 右组 ml-auto 同款）：倍速起到全屏
                    // 止的右组贴控制栏右缘，不与左组顺排。缩窗时**图标一律
                    // 不缩不裁**——flex 收缩全由发送框让位（compact 档见
                    // danmaku_bar），所以右组每个成员都钉死收缩系数
                    .child(div().flex_1())
                    .child(div().flex_shrink_0().child(self.render_rate_button(cx)))
                    .child(
                        div()
                            .flex_shrink_0()
                            .child(self.render_definition_button(cx)),
                    )
                    .child(div().flex_shrink_0().child(self.render_picker_entry(cx)))
                    .child(
                        // 下载入口（Tauri download-sheet 同款位置：右段）
                        Button::new("stage-downloads")
                            .icon(gpui_kit::assets::IconName::Download)
                            .label(crate::i18n::t("player.download"))
                            .custom(chrome_variant(cx))
                            .cursor_pointer()
                            .small()
                            .flex_shrink_0()
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.toggle_downloads(window, cx)
                                }),
                            ),
                    )
                    .child(div().flex_shrink_0().child(self.render_mini_entry(cx)))
                    .child(div().flex_shrink_0().child(self.render_dm_settings(cx)))
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
                            .flex_shrink_0()
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
                    .child(div().flex_shrink_0().child(self.render_volume_controls(cx)))
                    .child(
                        // 隐身模式（Tauri incognito 同款）：开着时光标脱离窗口 →
                        // 暂停 + 整窗藏起，回到窗口矩形 → 原地重现 + 续播。
                        // 亮暗态走 `selected` 语义态（弹幕开关同款，悬停免疫）
                        Button::new("stage-incognito")
                            .icon(gpui_kit::assets::IconName::Eye)
                            .custom(chrome_variant(cx))
                            .cursor_pointer()
                            .small()
                            .flex_shrink_0()
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
                    .child(div().flex_shrink_0().child(self.render_portrait_entry(cx)))
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
                            .flex_shrink_0()
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
                            .flex_shrink_0()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_immersive_fullscreen(window, cx);
                            })),
                    )
                    .into_any_element(),
            )
            .into_any_element()
    }
}
