//! 控制条菜单与浮层：倍速/清晰度下拉、选集入口、弹幕设置面板。

use super::*;

use gpui_kit::Window;
use gpui_kit::component::Disableable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::v_flex;
use gpui_kit::div;

use crate::home::player_icons;

impl HomeView {
    /// 倍速入口（Tauri 同款下拉菜单）。
    pub(super) fn render_rate_button(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
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
            })
            .into_any_element()
    }

    /// 清晰度入口（Tauri 同款菜单；本地集保留置灰钮）。
    pub(super) fn render_definition_button(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
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
                        let mut menu = menu.label(crate::i18n::t("player.definition"));
                        menu = menu.item(
                            PopupMenuItem::new(crate::i18n::t("player.definitionAuto"))
                                .checked(current == 0)
                                .on_click({
                                    let view = view.clone();
                                    move |_, _, cx| {
                                        view.update(cx, |v, cx| v.switch_definition(None, cx))
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
    }

    /// 选集浮层入口（Tauri immersive 同款：ListVideo 图标 + 总集数）。
    pub(super) fn render_picker_entry(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
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
            }))
            .into_any_element()
    }

    /// 弹幕设置入口与面板（Tauri 同款：透明度/字号/密度/显示区域）。
    pub(super) fn render_dm_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
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
    }
}
