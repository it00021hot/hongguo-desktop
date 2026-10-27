use super::*;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, InteractiveElementExt as _, Sizable};
use gpui_kit::div;
use gpui_kit::prelude::*;

use crate::ui_button::button as Button;

impl super::Shell {
    /// 顶栏（自绘，Tauri `app-shell` header 的 44px 横贯薄行对齐）：
    /// 中部是各页分类 tab 的居中槽（Tauri `TOP_BAR_SLOT_ID` portal 同位），
    /// 右侧置顶 / 外观 / 窗口钮；空白带按住拖窗（>4px 才起拖）、双击最大化。
    ///
    /// 为什么自绘：组件库 `TitleBar` 为 Windows 把整条 bar 标成
    /// `WindowControlArea::Drag`（OS 级 HTCAPTION），落进去的可点控件点击
    /// 全被原生命中测试吞成拖拽（置顶钮上一版就栽在这）。自绘后拖拽改手动
    /// （`start_window_move`），控件走正常命中路径；窗口钮仍用
    /// `WindowControlArea`（Min/Max/Close）交给 OS 原生处理——关闭请求照旧
    /// 走 WM_CLOSE 的退出确认守卫。
    pub(super) fn render_toolbar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let border = theme.border;
        let bg = theme.sidebar;
        let fg = theme.secondary_foreground;
        let hover_bg = theme.secondary_hover;
        let danger = theme.danger;
        let danger_fg = theme.danger_foreground;
        let mac = cfg!(target_os = "macos");

        // 竖屏不渲染 tab 槽（Tauri 4376496 同款：388px 窄条里整排 tab 与
        // 右侧钮必然叠压；竖屏=沉浸看剧，切 tab 退出竖屏再说）
        let in_portrait = crate::portrait::is_active();
        let tabs = (!in_portrait).then(|| self.render_top_tabs(cx)).flatten();
        let pinned = self.feed.read(cx).pinned();
        let is_maximized = window.is_maximized();

        // 置顶（Tauri 顶栏钉住钮同位）：与小屏紧凑条共用同一份状态
        let pin_button = Button::new("toolbar-pin")
            .icon(if pinned {
                IconName::Pin
            } else {
                IconName::PinOff
            })
            .ghost()
            .small()
            .tooltip(crate::i18n::t("player.pin"))
            .on_click(cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                this.feed
                    .update(cx, |view, cx| view.toggle_pinned(window, cx));
                cx.notify();
            }));

        // 语言（Tauri ThemeSwitch 的语言菜单同位）：资源与 Tauri 共用；
        // 切换后刷新整个窗口，子实体不需要各自保存一份 locale。
        let current_locale = self.locale;
        let language_view = cx.entity();
        let language_menu = move |menu: gpui_kit::component::menu::PopupMenu,
                                  _window: &mut Window,
                                  _cx: &mut Context<gpui_kit::component::menu::PopupMenu>|
              -> gpui_kit::component::menu::PopupMenu {
            let mut menu = menu.label(crate::i18n::t("search.langLabel"));
            for locale in crate::i18n::Locale::ALL {
                let view = language_view.clone();
                menu = menu.item(
                    PopupMenuItem::new(crate::i18n::t(locale.label_key()))
                        .checked(current_locale == locale)
                        .on_click(move |_, window, cx| {
                            view.update(cx, |shell, cx| shell.set_locale(locale, window, cx));
                        }),
                );
            }
            menu
        };
        let language_button = Button::new("toolbar-language")
            .icon(IconName::Languages)
            .ghost()
            .small()
            .tooltip(crate::i18n::t("search.langLabel"))
            .dropdown_menu(language_menu);

        // 外观（Tauri ThemeSwitch 同位）：三档菜单语义与设置页外观卡一致，
        // 点击回调拿不到壳的 Context，经实体句柄回写
        let view = cx.entity();
        let current = self.theme;
        let theme_menu = move |menu: gpui_kit::component::menu::PopupMenu,
                               _window: &mut Window,
                               _cx: &mut Context<gpui_kit::component::menu::PopupMenu>|
              -> gpui_kit::component::menu::PopupMenu {
            let mut menu = menu.label(crate::i18n::t("settings.appearance"));
            for (value, label) in [
                (
                    crate::ui_prefs::ThemePref::Auto,
                    crate::i18n::t("settings.themeAuto"),
                ),
                (
                    crate::ui_prefs::ThemePref::Light,
                    crate::i18n::t("settings.themeLight"),
                ),
                (
                    crate::ui_prefs::ThemePref::Dark,
                    crate::i18n::t("settings.themeDark"),
                ),
            ] {
                let view = view.clone();
                menu = menu.item(
                    PopupMenuItem::new(label)
                        .checked(current == value)
                        .on_click(move |_, _window, cx| {
                            view.update(cx, |shell, cx| shell.set_theme_pref(value, cx));
                        }),
                );
            }
            menu
        };
        let theme_icon = match current {
            crate::ui_prefs::ThemePref::Auto => IconName::Monitor,
            crate::ui_prefs::ThemePref::Light => IconName::Sun,
            crate::ui_prefs::ThemePref::Dark => IconName::Moon,
        };
        let theme_button = Button::new("toolbar-theme")
            .icon(theme_icon)
            .ghost()
            .small()
            .tooltip(crate::i18n::t("settings.appearance"))
            .dropdown_menu(theme_menu);

        // 窗口钮（Windows/Linux；macOS 是原生红绿灯）：Tauri WindowButtons
        // 同款 40px 方块、关闭键 hover 红底；点击交 OS 原生命中处理
        let window_controls = (!mac).then(|| {
            let button = |id: &'static str,
                          icon: IconName,
                          area: gpui_kit::WindowControlArea,
                          is_close: bool| {
                div()
                    .id(id)
                    .flex()
                    .w(gpui_kit::px(40.0))
                    .h_full()
                    .justify_center()
                    .items_center()
                    .text_color(fg)
                    .hover(move |style| {
                        if is_close {
                            style.bg(danger).text_color(danger_fg)
                        } else {
                            style.bg(hover_bg)
                        }
                    })
                    .window_control_area(area)
                    .child(Icon::new(icon).small())
            };
            h_flex()
                .h_full()
                .flex_shrink_0()
                .child(button(
                    "win-min",
                    IconName::WindowMinimize,
                    gpui_kit::WindowControlArea::Min,
                    false,
                ))
                .child(button(
                    "win-max",
                    if is_maximized {
                        IconName::WindowRestore
                    } else {
                        IconName::WindowMaximize
                    },
                    gpui_kit::WindowControlArea::Max,
                    false,
                ))
                .child(button(
                    "win-close",
                    IconName::WindowClose,
                    gpui_kit::WindowControlArea::Close,
                    true,
                ))
        });

        h_flex()
            .id("app-toolbar")
            .relative()
            .h(gpui_kit::px(44.0))
            .flex_shrink_0()
            .items_center()
            .bg(bg)
            .border_b_1()
            .border_color(border)
            // 左右不留内边距：绝对定位的 tab 槽按内容盒定位，留出左内边距
            // 会把槽整体推偏几个像素（Tauri 的 inset-x-0 按边框盒，居中在
            // 整窗中线）。顶栏左侧本就不放内容（Tauri 同为空），macOS 的
            // 原生红绿灯不受影响
            .pr_0()
            // 拖窗/双击（见函数头注释）：阈值防按钮误拖
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, ev: &gpui_kit::MouseDownEvent, _, _| {
                    this.toolbar_drag = Some(ev.position);
                }),
            )
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.toolbar_drag = None;
                }),
            )
            .on_mouse_move(
                cx.listener(|this, ev: &gpui_kit::MouseMoveEvent, window, _cx| {
                    // 只认**按住左键**的移动：按钮内部会 stop 掉 mouse up
                    //（toolbar_drag 滞留 Some），不查按键的话，松开后的移动
                    // 也会累计位移误起拖（headless 交互探针实测踩中）。
                    if ev.pressed_button != Some(gpui_kit::MouseButton::Left) {
                        return;
                    }
                    let Some(start) = this.toolbar_drag else {
                        return;
                    };
                    let dx = (ev.position.x - start.x).as_f32().abs();
                    let dy = (ev.position.y - start.y).as_f32().abs();
                    if dx > 4.0 || dy > 4.0 {
                        this.toolbar_drag = None;
                        #[cfg(target_os = "windows")]
                        {
                            let hwnd = crate::win32::hwnd_of(window);
                            log::info!("[Toolbar] 起拖窗口（hwnd {hwnd:?}，位移 {dx:.0},{dy:.0}）");
                            // WM_NCLBUTTONDOWN 会同步进系统移动循环，必须挪到
                            // 实体更新之外——事件分发当口同步派发 WM_* 会撞
                            // gpui 的借用（隐身藏显 ShowWindow 同款坑，见
                            // `home/incognito.rs` 的 IncognitoStep 模式）。
                            // 延迟一拍只是微秒级，物理按键仍按着，移动循环照进
                            _cx.spawn(async move |_this, _cx| {
                                if let Some(hwnd) = hwnd {
                                    crate::win32::start_window_move(hwnd);
                                }
                            })
                            .detach();
                        }
                        #[cfg(not(target_os = "windows"))]
                        window.start_window_move();
                    }
                }),
            )
            .on_double_click(cx.listener(|this, _, window, _cx| {
                this.toolbar_drag = None;
                // macOS 要的是系统标题栏双击语义（含「移动并调整大小」等
                // 原生行为）；其余平台切最大化。同样挪出实体更新（见上）
                if cfg!(target_os = "macos") {
                    window.titlebar_double_click();
                    return;
                }
                #[cfg(target_os = "windows")]
                {
                    let hwnd = crate::win32::hwnd_of(window);
                    _cx.spawn(async move |_this, _cx| {
                        if let Some(hwnd) = hwnd {
                            crate::win32::toggle_maximize(hwnd);
                        }
                    })
                    .detach();
                }
                #[cfg(not(target_os = "windows"))]
                window.zoom_window();
            }))
            // 中部 tab 槽：绝对定位真居中（Tauri 同款——左右组宽度不等时
            // flex 剩余空间居中有肉眼可见偏移）；容器无命中盒，空白带不挡拖窗
            .children(tabs.map(|tabs| {
                h_flex()
                    .absolute()
                    .left_0()
                    .right_0()
                    .h_full()
                    .justify_center()
                    .items_center()
                    .child(tabs)
            }))
            .child(
                h_flex()
                    .ml_auto()
                    .h_full()
                    .items_center()
                    .gap_1()
                    .child(pin_button)
                    .child(language_button)
                    .child(theme_button)
                    .children(window_controls),
            )
            .into_any_element()
    }
}
