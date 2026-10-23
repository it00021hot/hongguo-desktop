//! 舞台信息叠加层：热度/剧名/集数/简介压在画面左下，控制条弹出时抬
//! 到其上沿，闲置淡出由调用方按 controls_visible 决定渲染。

use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{SharedString, div};

impl super::HomeView {
    /// 信息叠加层（Tauri immersive-info-overlay 对齐）：热度/剧名/集数/
    /// 简介压在画面左下，控制条弹出时抬到其上沿（bottom_12），闲置淡出
    /// 由调用方按 controls_visible 决定渲染。
    pub(super) fn render_overlay(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let Some(now) = &self.now else {
            return div().into_any_element();
        };
        // 展示标记跟着**在播的剧**（起播时抄存），不跟游标：换一批/钉住条目
        // 位移时游标可能落在别的条目上，跟游标会把隔壁剧的热度/角标贴错。
        let badge = (!now.badge.is_empty()).then(|| now.badge.clone());
        let season = (!now.season_tag.is_empty()).then(|| now.season_tag.clone());
        let heat = (!now.heat_text.is_empty()).then(|| now.heat_text.clone());
        let intro = self.intros.get(&now.series_id).cloned();
        let intro_expandable = intro.as_ref().is_some_and(|i| i.chars().count() > 40);
        // 「控制栏出现时抬起」（Tauri immersive-info-overlay 同款双向位
        // 移）：控制栏可见 → 112px（抬到控制栏上沿）；隐藏 → 56px。此前
        // 恒 48px，抬起语义丢失。绝对 px 与表情/音量弹层同口径（刻度
        // 助手没有这一档）
        let lifted = self.controls_visible();
        v_flex()
            .absolute()
            .when(lifted, |s| s.bottom(gpui_kit::px(112.0)))
            .when(!lifted, |s| s.bottom(gpui_kit::px(56.0)))
            .left_3()
            // **定宽**而不是 max_w：简介行的百分比上限要挂在有确定宽度的
            // 父级上才生效——overlay 宽度由内容决定时，子级的
            // max_w(58%) 解析不出基准，简介一行冲出屏幕不换行（实测）。
            // 62% 对齐 Tauri max-w-[62%]。
            .w(gpui_kit::relative(0.62))
            .gap_1()
            // 信息层压在画面上：整块拦下鼠标（gpui 重叠兄弟分支都派发，
            // 不拦点信息文字会同时触发「点画面暂停」；子级的标题/展开钮
            // 另有自己的拦与点击）
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .children(heat.map(|h| {
                // 热度行（hgplayer 同款：剧名上方，琥珀色）
                div()
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .text_color(gpui_kit::rgba(0xfcd34de6))
                    .child(SharedString::from(h))
            }))
            .child(
                h_flex()
                    .items_center()
                    .gap_1p5()
                    .children(badge.map(|b| {
                        div()
                            .px_1()
                            .rounded(gpui_kit::px(2.0))
                            .bg(gpui_kit::rgba(0xef4444e6))
                            .child(
                                div()
                                    .text_size(gpui_kit::px(10.0))
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .text_color(gpui_kit::rgb(0xffffff))
                                    .child(SharedString::from(b)),
                            )
                    }))
                    .children(season.map(|s| {
                        div()
                            .px_1()
                            .rounded(gpui_kit::px(2.0))
                            .bg(gpui_kit::rgba(0xffffff33))
                            .child(
                                div()
                                    .text_size(gpui_kit::px(10.0))
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .text_color(gpui_kit::rgb(0xffffff))
                                    .child(SharedString::from(s)),
                            )
                    }))
                    .children((!now.title.is_empty()).then(|| {
                        // Tauri 同款交互：点剧名离开播放器看档案。GPUI 壳
                        // 无详情路由，先打开官方详情页（系统默认浏览器）；
                        // stopPropagation：点标题不能同时触发「点画面暂停」。
                        div()
                            .id("overlay-title")
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(gpui_kit::rgb(0xffffff))
                            .truncate()
                            .cursor_pointer()
                            .hover(|s| s.text_color(gpui_kit::rgba(0xffd7d7ff)))
                            .on_mouse_down(
                                gpui_kit::MouseButton::Left,
                                cx.listener(|_, _, _, cx| cx.stop_propagation()),
                            )
                            .on_click({
                                let sid = now.series_id.clone();
                                cx.listener(move |this, _, window, cx| {
                                    // 点剧名 → 应用内详情页（壳订阅事件切目的
                                    // 地）；stop_propagation 防触发画面暂停。
                                    // 离开播放页先退竖屏（Tauri 路由离开 =
                                    // exitPortraitScreen 同款；竖条几何是单份
                                    // 快照，带走会写坏小屏/竖屏的还原基线）
                                    this.exit_portrait(window, cx);
                                    cx.stop_propagation();
                                    cx.emit(super::HomeEvent::OpenDetail(sid.clone()));
                                })
                            })
                            .child(SharedString::from(format!("@{}", now.title)))
                    })),
            )
            .child(
                div()
                    .mt_0p5()
                    // Tauri 这里是一个 <p> 内联 <span>；GPUI 必须显式 flex，
                    // 否则第二个 div 会换行成「Ep. 1 / · 53 episodes」。
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(gpui_kit::rgba(0xffffffd9))
                    .child(crate::i18n::tf(
                        "player.episodeNo",
                        &[("index", &now.vid_index.to_string())],
                    ))
                    .children((now.episode_count > 0).then(|| {
                        div().text_color(gpui_kit::rgba(0xffffffb3)).child(format!(
                            " · {}",
                            crate::i18n::tf(
                                "player.totalEpisodes",
                                &[("count", &now.episode_count.to_string())],
                            )
                        ))
                    })),
            )
            .children(intro.map(|intro| {
                // 简介块只占舞台约三分之一（Tauri max-w-[36%] 同款：overlay
                // 定宽 62% × 本行 58% ≈ 舞台 36%）；块内点击不落回舞台
                //（防触发暂停/继续），展开交互只在按钮上。文本
                // flex_1 + min_w_0：在行内收敛宽度强制换行——没有这一步
                // 文本按 max-content 一行铺开跑出屏幕（实测）。
                let expanded = self.intro_expanded;
                h_flex()
                    .id("overlay-intro")
                    .mt_1()
                    .max_w(gpui_kit::relative(0.58))
                    .items_end()
                    .gap_2()
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .line_height(gpui_kit::relative(1.6))
                            .text_color(gpui_kit::rgba(0xffffffb3))
                            // 收起态钳两整行（12px × 1.6 行高 ≈ 19.2/行），
                            // 32px 会把第二行拦腰截断
                            .when(!expanded, |s| s.h(gpui_kit::px(38.0)).overflow_hidden())
                            .child(SharedString::from(intro.clone())),
                    )
                    .children((intro_expandable).then(|| {
                        div()
                            .id("intro-toggle")
                            .flex_shrink_0()
                            .pb(gpui_kit::px(2.0))
                            .text_xs()
                            .cursor_pointer()
                            .text_color(gpui_kit::rgba(0xffffff99))
                            .hover(|s| s.text_color(gpui_kit::rgb(0xffffff)))
                            .when(expanded, |s| {
                                s.on_click(cx.listener(|this, _, _, cx| {
                                    this.intro_expanded = false;
                                    cx.notify();
                                }))
                            })
                            .when(!expanded, |s| {
                                s.on_click(cx.listener(|this, _, _, cx| {
                                    this.intro_expanded = true;
                                    cx.notify();
                                }))
                            })
                            .child(if expanded {
                                crate::i18n::t("player.introCollapse")
                            } else {
                                crate::i18n::t("player.introExpand")
                            })
                    }))
            }))
            .into_any_element()
    }
}
