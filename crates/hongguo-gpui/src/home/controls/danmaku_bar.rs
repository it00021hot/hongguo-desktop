//! 控制条上的弹幕输入区：发送 pill 与表情弹层。

use super::*;

use gpui_kit::component::Icon;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::v_flex;
use gpui_kit::div;

impl HomeView {
    /// 弹幕发送框（Tauri 同款：无框输入 + 内嵌表情 + 发送）。
    pub(super) fn render_danmaku_box(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        // 弹幕发送 pill（Tauri 同款）：无框输入 + 内嵌表情 +
        // 发送；表情弹层从 pill 上方拉起（与评论面板同源
        // 表情表，光标处插入 [名字]，上屏由弹幕渲染分段画）。
        // ml-2 与时间读数的间距（Tauri 同款微调）
        div()
            .id("danmaku-box")
            .ml_2()
            .relative()
            // 表情/提及预览（浮在胶囊上方，同款深底小条）：GPUI 的 Input 是
            // 纯文本编辑器，插进去的 `[名字]` 只是代码，这里把同一段文本按
            // 弹幕层那套分段渲染出表情图与蓝色 @名字。表情面板开着时让位
            // （那会儿面板才是主角），且它同样浮在控制条上方 → 自己也拦一层
            // 鼠标，别让点击漏到画面层（点画面 = 暂停）。
            .children(
                (!self.danmaku_emoji_open)
                    .then(|| {
                        self.render_rich_preview(
                            &self
                                .danmaku_input
                                .as_ref()
                                .map(|i| i.read(cx).value().to_string())
                                .unwrap_or_default(),
                        )
                    })
                    .flatten()
                    .map(|preview| {
                        div()
                            .absolute()
                            .left_0()
                            .bottom(gpui_kit::px(38.0))
                            .px_2()
                            .py_1()
                            .rounded(gpui_kit::px(6.0))
                            .bg(gpui_kit::rgba(0x000000d9))
                            .on_mouse_down(
                                gpui_kit::MouseButton::Left,
                                cx.listener(|_, _, _, cx| cx.stop_propagation()),
                            )
                            .on_mouse_up(
                                gpui_kit::MouseButton::Left,
                                cx.listener(|_, _, _, cx| cx.stop_propagation()),
                            )
                            .child(preview)
                    }),
            )
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
                            .on_click(cx.listener(|this, _, _, cx| this.submit_danmaku(cx)))
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
                                                // 光标处插入 [名字]
                                                input.update(cx, |state, cx| {
                                                    state.insert(code.clone(), window, cx);
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
