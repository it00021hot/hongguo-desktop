//! 控制条上的弹幕输入区：发送 pill 与表情弹层；窄舞台折叠成「弹」字笔钮
//! （手机端同款）+ 点开上方浮层输入。

use super::*;

use gpui_kit::Focusable;
use gpui_kit::component::Icon;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::v_flex;
use gpui_kit::div;

use crate::home::player_icons;

/// 舞台宽度低于此值时，发送框折叠成「弹」字笔钮（手机端同款），时间读数
/// 一并收起。
///
/// 阈值依据（1024/860/640 三档真窗截图实测）：全图标 + 最窄发送框约需
/// 900px，折叠后全图标约需 830px——920 之上 inline 放得下（含最窄胶囊），
/// 之下折叠才能保住「图标一个不裁」。
const COMPACT_BELOW_STAGE_W: f32 = 920.0;

/// pill 的输入宽度档：inline 跟随舞台收缩（图标让位的余量全吃在这里），
/// 浮层里固定宽。
enum PillWidth {
    /// 控制栏行内：胶囊 176px 起、可缩到 120px，输入 flex 吃满胶囊。
    Inline,
    /// 浮层里：固定 176px。
    Fixed,
}

impl HomeView {
    /// 控制栏紧凑档判定（窄舞台：发送框折叠、时间读数收起，图标一个不裁）。
    /// 舞台宽度取上一帧布局（`stage_rects`），一帧滞后无碍。
    pub(in crate::home) fn controls_compact(&self) -> bool {
        self.stage_rects
            .get()
            .map(|rect| rect[0] < COMPACT_BELOW_STAGE_W)
            .unwrap_or(false)
    }

    /// 弹幕发送框（Tauri 同款：无框输入 + 内嵌表情 + 发送）。窄舞台折叠成
    /// 「弹」字笔钮，点开上方浮层输入（图标不因缩窗被裁的让位方就是它）。
    pub(super) fn render_danmaku_box(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        if self.controls_compact() {
            return self.render_danmaku_compact(cx);
        }
        // ml-2 与时间读数的间距（Tauri 同款微调）。宽度基准（Tauri w-44 =
        // 176px）在本盒上、可收缩（min 120）：缩窗时图标全靠它让位，输入
        // flex 吃满胶囊。
        div()
            .id("danmaku-box")
            .ml_2()
            .relative()
            .w(gpui_kit::px(176.0))
            .min_w(gpui_kit::px(120.0))
            .flex_shrink_1()
            .child(self.render_danmaku_pill(cx, PillWidth::Inline))
            .into_any_element()
    }

    /// 窄舞台档：「弹」字笔钮；点开上方浮层输入（点外部/切剧收起）。
    fn render_danmaku_compact(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let color = gpui_kit::rgba(0xffffffff);
        div()
            .id("danmaku-box-compact")
            .ml_2()
            .relative()
            .flex_shrink_0()
            // 浮层输入（同表情面板的定位与拦层纪律：浮在控制条上方，得自己
            // 拦鼠标 + 点外部收起；深底衬才压得住画面——pill 本体只有 15% 白）
            .children(self.danmaku_compact_open.then(|| {
                div()
                    .absolute()
                    .left_0()
                    .bottom(gpui_kit::px(38.0))
                    .p_1()
                    .rounded_full()
                    .bg(gpui_kit::rgba(0x000000d9))
                    .border_1()
                    .border_color(gpui_kit::rgba(0xffffff14))
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.danmaku_compact_open = false;
                        cx.notify();
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
                    .child(self.render_danmaku_pill(cx, PillWidth::Fixed))
            }))
            .child(
                div()
                    .id("stage-danmaku-write")
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
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.danmaku_compact_open = !this.danmaku_compact_open;
                        if this.danmaku_compact_open {
                            // 开即聚焦：手机端点它就是为了打字
                            if let Some(input) = &this.danmaku_input {
                                let handle = input.read(cx).focus_handle(cx);
                                window.focus(&handle, cx);
                            }
                        }
                        cx.notify();
                    }))
                    .child(player_icons::danmaku_write(color)),
            )
            .into_any_element()
    }

    /// 发送 pill：无框输入 + 内嵌表情 + 发送（hgplayer 固定深色皮肤，Tauri
    /// 同款，不跟主题走）。inline 挂在控制栏行里（可收缩），浮层里固定宽。
    ///
    /// `overlay_bottom` 是表情网格相对本 pill 容器的 bottom：inline 直接悬
    /// 在控制栏上方（38），浮层里还要再让过一层浮层 Wrapper（40）。
    fn render_danmaku_pill(&self, cx: &Context<Self>, width: PillWidth) -> gpui_kit::AnyElement {
        let overlay_bottom = match width {
            PillWidth::Inline => 38.0,
            PillWidth::Fixed => 40.0,
        };
        // 弹幕输入：hgplayer 固定深色皮肤（Tauri 同款，
        // 不跟主题走）——15% 白底胶囊、白字、白光标、
        // 白 60% 占位符。光标/占位符/选区靠
        // [`skinned_input`] 在渲染当期改主题 token。
        let ink: gpui_kit::Hsla = gpui_kit::rgba(0xffffffff).into();
        let emoji_active = ink;
        let emoji_idle: gpui_kit::Hsla = gpui_kit::rgba(0xffffff99).into();
        let input = self.danmaku_input.as_ref().map(|input| {
            // 内联 token（Tauri RichEmojiInput 对齐）：`[名字]` 在输入框里
            // 直接渲染成表情图，value 仍是代码纯文本——预览条随之取消。
            let rich =
                Input::new(input).token(|ctx, _, _| crate::danmaku_emoji::render_emoji_token(ctx));
            // 透明输入（Tauri inputClassName 的 `bg-transparent`）：
            // `appearance(false)` 才不吃主题底色——否则亮色主题下胶囊里
            // 会嵌一块白底方块（用户实测截图）。inline 是 `flex-1`（Tauri
            // 同款）：撑满胶囊、随胶囊收缩；浮层里固定 176px。
            let sized = match width {
                PillWidth::Inline => rich
                    .appearance(false)
                    .bordered(false)
                    .h_full()
                    .flex_1()
                    .min_w_0(),
                PillWidth::Fixed => rich
                    .appearance(false)
                    .bordered(false)
                    .h_full()
                    .w(gpui_kit::px(176.0)),
            };
            skinned_input(sized.text_xs().text_color(ink))
        });
        let pill = h_flex()
            .w_full()
            .items_center()
            .gap_1()
            .pl_3()
            .pr_1()
            .h(gpui_kit::px(32.0))
            .rounded_full()
            .bg(gpui_kit::rgba(0xffffff26))
            .when_some(input, |this, input| this.child(input))
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
                    .is_none_or(|input| input.read(cx).value().trim().is_empty());
                Button::new("stage-danmaku-send")
                    .label(crate::i18n::t("player.interact.send"))
                    .primary()
                    .small()
                    .h(gpui_kit::px(24.0))
                    .rounded_full()
                    .cursor_pointer()
                    .flex_shrink_0()
                    .opacity(if empty || self.danmaku_sending {
                        0.5
                    } else {
                        1.0
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.submit_danmaku(cx)))
            });
        div()
            .relative()
            .child(pill)
            .children(self.danmaku_emoji_open.then(|| {
                // 表情网格（评论面板同款：连选不关面板、光标处
                // 插入不夺焦；点外部 on_mouse_down_out 收起）
                let danmaku_input = self.danmaku_input.clone();
                v_flex()
                    .absolute()
                    .left_0()
                    .bottom(gpui_kit::px(overlay_bottom))
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
                    // 面板自己拦一层鼠标（评论面板的表情面板同款）：**这个面板
                    // 浮在控制条上方**（bottom 38px + 176px 高），控制条自己的
                    // bounds 罩不到它 → 控制条那层的 stop_propagation 拦不住
                    // 这次点击，事件会一路落到舞台画面层（点画面 = 暂停）。
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .on_mouse_up(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
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
                            .children(crate::danmaku_emoji::EMOJI_FILES.iter().map(|(name, _)| {
                                let code = name.to_string();
                                let emoji = crate::danmaku_emoji::emoji_by_name(name);
                                div()
                                    .id(gpui_kit::ElementId::from(format!("dm-emoji-{}", code)))
                                    .size_8()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(gpui_kit::px(6.0))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(gpui_kit::rgba(0xffffff26)))
                                    .on_click({
                                        let input = danmaku_input.clone();
                                        move |_, window, cx| {
                                            if let Some(input) = &input {
                                                // 光标处插入 `[名字]` 并附成原子
                                                // 内联 token（Tauri insertEmoji 对齐）；
                                                // token 插入被拒（理论路径）时退回
                                                // 纯文本插入，宁可见代码不可丢输入
                                                input.update(cx, |state, cx| {
                                                    let token =
                                                        crate::danmaku_emoji::emoji_inline_token(
                                                            &code,
                                                        );
                                                    if state
                                                        .replace_with_token(token, window, cx)
                                                        .is_err()
                                                    {
                                                        log::warn!(
                                                            "[Home] 表情 token 插入被拒，退回纯文本"
                                                        );
                                                        state.insert(code.clone(), window, cx);
                                                    }
                                                });
                                            }
                                        }
                                    })
                                    .children(emoji.map(|bitmap| {
                                        gpui_kit::img(gpui_kit::ImageSource::Render(bitmap))
                                            .size_6()
                                    }))
                            })),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }
}
