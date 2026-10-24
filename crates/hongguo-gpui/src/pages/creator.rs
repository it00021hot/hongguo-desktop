//! 创作者主页（Tauri `/creator` = `creator-home-page.tsx` 的 GPUI 对齐）。
//!
//! 详情页「出品方/演职员」芯片点入。资料头 = reading/user/profile/get
//! （昵称/头像/简介，资料失败不打断作品列表——官方头部同样可缺）；
//! 作品列表 = commentapi/user/profile/{uid}（游标翻页，3:4 封面卡：
//! 标题 + 集数/热度/播放行，点击起播——统一首页沉浸流）。页面发事件给壳
//! （Play/Back），页面间不互相持有。

use std::sync::Arc;

use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, SharedString, Window, div};
use hongguo_api::danmaku::{CreatorProfile, CreatorWork};
use hongguo_service::app_state::AppStateInner;

use crate::bridge;
use crate::home::fmt::format_play_count;
use crate::ui_button::button as Button;

/// 创作者页发往壳的事件（壳订阅后切目的地/起播）。
#[derive(Debug, Clone)]
pub(crate) enum CreatorEvent {
    Play {
        series_id: String,
        vid_index: u32,
        start_secs: f64,
    },
    /// 返回：回到来源页（详情页）或首页（壳裁决）。
    Back,
}

pub(crate) struct CreatorView {
    state: Arc<AppStateInner>,
    user_id: String,
    /// 资料头（失败为 None，头部不渲染不拦列表——Tauri 同口径）。
    profile: Option<CreatorProfile>,
    // —— 作品列表 ——
    works: Vec<CreatorWork>,
    cursor: String,
    has_more: bool,
    /// 作品总数（首页 `hightlight.total`，标题行「作品 · N」）。
    total: i64,
    /// 首页是否已到手（标题行渲染门槛，Tauri `works.data` 同语义）。
    works_loaded: bool,
    loading: bool,
    loading_more: bool,
    failed: Option<String>,
    /// 滚动哨兵：滚到底自动续页（按钮留作兜底）。
    scroll: crate::pages::scroll_load::ScrollLoad,
}

impl CreatorView {
    pub(crate) fn new(state: Arc<AppStateInner>) -> Self {
        Self {
            state,
            user_id: String::new(),
            profile: None,
            works: Vec::new(),
            cursor: String::new(),
            has_more: false,
            total: 0,
            works_loaded: false,
            loading: false,
            loading_more: false,
            failed: None,
            scroll: crate::pages::scroll_load::ScrollLoad::new(),
        }
    }

    /// 装载一位创作者（资料 + 作品首页并行，各降各的级）。
    pub(crate) fn load(&mut self, user_id: String, cx: &mut Context<Self>) {
        self.user_id = user_id.clone();
        self.profile = None;
        self.works.clear();
        self.cursor.clear();
        self.has_more = false;
        self.total = 0;
        self.works_loaded = false;
        self.loading = true;
        self.loading_more = false;
        self.failed = None;
        cx.notify();

        // 资料头（独立降级：失败只丢头部）
        let state = Arc::clone(&self.state);
        let uid = user_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::danmaku::fetch_creator_profile(&uid, &env).await
        });
        let uid_profile = user_id.clone();
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                if view.user_id != uid_profile {
                    return; // 已切人，迟到响应丢弃
                }
                match result {
                    Ok(profile) => {
                        view.profile = Some(profile);
                        cx.notify();
                    }
                    Err(e) => log::warn!("[Creator] 资料头失败（不拦列表）: {e}"),
                }
            });
        })
        .detach();

        // 作品首页
        let state = Arc::clone(&self.state);
        let uid = user_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::danmaku::fetch_creator_works(&uid, "", &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                if view.user_id != user_id {
                    return;
                }
                view.loading = false;
                match result {
                    Ok(page) => {
                        log::info!("[Creator] GPUI 壳作品首页 -> {} 条", page.works.len());
                        view.total = page.total.max(page.works.len() as i64);
                        view.works = page.works;
                        view.cursor = page.cursor;
                        view.has_more = page.has_more;
                        view.works_loaded = true;
                    }
                    Err(e) => {
                        log::warn!("[Creator] GPUI 壳作品失败: {e}");
                        view.failed = Some(crate::i18n::app_error_text(&e));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 翻页（游标原样续传）。
    fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.loading_more || self.loading || !self.has_more || self.cursor.is_empty() {
            return;
        }
        self.loading_more = true;
        cx.notify();
        let state = Arc::clone(&self.state);
        let uid = self.user_id.clone();
        let cursor = self.cursor.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::danmaku::fetch_creator_works(&uid, &cursor, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.loading_more = false;
                match result {
                    Ok(page) => {
                        view.works.extend(page.works);
                        view.cursor = page.cursor;
                        view.has_more = page.has_more;
                    }
                    Err(e) => {
                        log::warn!("[Creator] GPUI 壳翻页失败: {e}");
                        window.push_notification(
                            gpui_kit::component::notification::Notification::error(
                                crate::i18n::tf(
                                    "common.failedWith",
                                    &[("message", &crate::i18n::app_error_text(&e))],
                                ),
                            ),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn play_series(&self, series_id: &str, cx: &mut Context<Self>) {
        let (vid_index, start_secs) =
            hongguo_service::play_service::resume::resolve_entry(&self.state.store, series_id, &[])
                .ok()
                .unwrap_or((1, 0.0));
        cx.emit(CreatorEvent::Play {
            series_id: series_id.to_string(),
            vid_index,
            start_secs,
        });
    }

    // —— 渲染 ——

    fn render_back_bar(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        h_flex()
            .items_center()
            .child(
                Button::new("creator-back")
                    .icon(gpui_kit::assets::IconName::ArrowLeft)
                    .label(crate::i18n::t("detail.back"))
                    .ghost()
                    .small()
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.emit(CreatorEvent::Back);
                    })),
            )
            .into_any_element()
    }

    /// 资料头：头像（64px 圆）+ 昵称 + 简介（2 行截断）。
    fn render_profile(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let Some(profile) = &self.profile else {
            return div().into_any_element();
        };
        // 头像走 gpui 图片元素：圆形裁剪与尺寸由外层容器负责，组件库管
        // 解码与缓存；没有头像地址时退回通用图标。
        let avatar = if profile.avatar.is_empty() {
            h_flex()
                .size(gpui_kit::px(64.0))
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(theme.muted)
                .child(
                    gpui_kit::component::Icon::new(gpui_kit::assets::IconName::UserRound)
                        .text_color(theme.muted_foreground),
                )
                .into_any_element()
        } else {
            div()
                .size(gpui_kit::px(64.0))
                .flex_shrink_0()
                .overflow_hidden()
                .rounded_full()
                .child(crate::cover_img::avatar(&profile.avatar))
                .into_any_element()
        };
        h_flex()
            .mt_4()
            .items_center()
            .gap_4()
            .child(avatar)
            .child(
                v_flex()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_size(gpui_kit::px(20.0))
                            .font_weight(gpui_kit::FontWeight::BOLD)
                            .child(SharedString::from(profile.user_name.clone())),
                    )
                    .children((!profile.description.is_empty()).then(|| {
                        div()
                            .mt_1()
                            .text_sm()
                            .line_clamp(2)
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(profile.description.clone()))
                    })),
            )
            .into_any_element()
    }

    /// 作品卡（3:4 封面 + 底部渐变信息条：标题/集数/热度/播放）。
    fn render_card(&self, work: &CreatorWork, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let cover = crate::cover_img::card(&work.cover, crate::cover_img::CoverShape::Detail);
        let hot = if work.hot > 0 {
            format_play_count(work.hot)
        } else {
            String::new()
        };
        let plays = if work.play_cnt > 0 {
            crate::i18n::tf(
                "browse.playCount",
                &[("count", &format_play_count(work.play_cnt))],
            )
        } else {
            String::new()
        };
        let episodes = if work.episode_cnt > 0 {
            crate::i18n::tf(
                "detail.allEpisodesBadge",
                &[("count", &work.episode_cnt.to_string())],
            )
        } else {
            String::new()
        };
        // 卡片封面下的信息条：Tauri 是 from-black/85 到透明的上向渐变——
        // 贴图叠加的既有口径（预约页封面角标同族），用同样的半透明黑。
        let overlay_bg = gpui_kit::linear_gradient(
            0.,
            gpui_kit::linear_color_stop(crate::tokens::scrim(85), 0.),
            gpui_kit::linear_color_stop(crate::tokens::scrim(0), 1.),
        );
        // 整卡可点 → 用 base::Button 承担 role/focus/Enter-Space（键盘用户能
        // 走到作品卡）。库强制 flex row + 居中，这里要复原原 div 的块级竖排：
        // flex_col + items_stretch（子件撑满卡宽）+ justify_start（内容从顶
        // 上排）。
        BaseButton::new(ElementId::from(format!("creator-work-{}", work.series_id)))
            .accessibility_label(work.title.clone())
            // group 名不能丢：封面用 group_hover("creator-work") 做悬停变淡。
            .group("creator-work")
            // 卡片宽度对齐 Tauri 网格密度（xl:grid-cols-5 @ max-w-6xl ≈
            // 205px/列；172 会把「全N集 · 热度 · 播放」行挤出截断）
            .w(gpui_kit::px(200.0))
            .flex_shrink_0()
            .rounded(crate::tokens::radius::CARD)
            .overflow_hidden()
            .bg(theme.secondary)
            .cursor_pointer()
            .flex_col()
            .items_stretch()
            .justify_start()
            // base Button 强制 line_height(relative(1.))，文字行盒比默认矮一
            // 截；钉回默认行高才与原 div 像素一致。
            .line_height(gpui_kit::phi())
            // 键盘焦点没有默认焦点环：按 Tauri/shadcn 同款补 border-ring + 3px 外圈。
            .focus_visible(|s| {
                s.border_color(theme.ring)
                    .shadow(crate::tokens::focus_ring(theme))
            })
            .tooltip({
                let title = work.title.clone();
                move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(SharedString::from(title.clone()))
                        .build(window, cx)
                }
            })
            .on_click({
                let sid = work.series_id.clone();
                cx.listener(move |this, _, _, cx| {
                    this.play_series(&sid, cx);
                })
            })
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(gpui_kit::px(267.0))
                    .child(
                        div()
                            .size_full()
                            // Tauri group-hover:opacity-80（悬停封面变淡）
                            .group_hover("creator-work", |s| s.opacity(0.8))
                            .child(cover),
                    )
                    .child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .bg(overlay_bg)
                            // 渐变条贴着卡片下缘，底角跟着卡片圆角（Tauri 侧
                            // 是卡片 overflow-hidden 裁出来的同款效果）
                            .rounded_b(crate::tokens::radius::CARD)
                            .p_2p5()
                            .pt_8()
                            .child(
                                div()
                                    .text_sm()
                                    .line_clamp(2)
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .text_color(crate::tokens::on_media(100))
                                    .child(SharedString::from(work.title.clone())),
                            )
                            .child(
                                h_flex()
                                    .mt_1()
                                    .items_center()
                                    .gap_2()
                                    .text_xs()
                                    .text_color(crate::tokens::on_media(70))
                                    .children(
                                        (!episodes.is_empty()).then(|| {
                                            div().child(SharedString::from(episodes.clone()))
                                        }),
                                    )
                                    .children((!hot.is_empty()).then(|| {
                                        h_flex()
                                            .items_center()
                                            .gap_0p5()
                                            .child(
                                                gpui_kit::component::Icon::new(
                                                    gpui_kit::assets::IconName::Flame,
                                                )
                                                .small(),
                                            )
                                            .child(SharedString::from(hot.clone()))
                                    }))
                                    .children(
                                        (!plays.is_empty()).then(|| {
                                            div().child(SharedString::from(plays.clone()))
                                        }),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_body(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        if let Some(err) = self.failed.clone() {
            return crate::pages::states::error_state(
                cx,
                None,
                crate::i18n::t("creator.loadFailed"),
                Some(err),
                Some((
                    "creator-retry",
                    crate::pages::states::retry_on(cx.entity(), |this, cx| {
                        let uid = this.user_id.clone();
                        this.load(uid, cx);
                    }),
                )),
            );
        }
        if self.works.is_empty() {
            return crate::pages::states::empty_state(
                cx,
                None,
                crate::i18n::t("creator.empty"),
                None,
            );
        }
        let mut column = v_flex().w_full().gap_3();
        column = column.child(
            h_flex()
                .w_full()
                .flex_wrap()
                .items_start()
                .gap_3()
                .children(
                    self.works
                        .clone()
                        .iter()
                        .map(|w| self.render_card(w, cx))
                        .collect::<Vec<_>>(),
                ),
        );
        if self.has_more {
            column = column.child(
                h_flex().w_full().justify_center().pt_2().child(
                    Button::new("creator-more")
                        .label(if self.loading_more {
                            crate::i18n::t("common.loading")
                        } else {
                            crate::i18n::t("follow.loadMore")
                        })
                        .outline()
                        .small()
                        .disabled(self.loading_more)
                        .when(self.loading_more, |b| b.cursor_default())
                        .on_click(cx.listener(|this, _, _, cx| this.load_more(cx))),
                ),
            );
        }
        column.into_any_element()
    }
}

impl gpui_kit::EventEmitter<CreatorEvent> for CreatorView {}

impl gpui_kit::Render for CreatorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 封面预热：排上转码（gpui 资产机制去重），成品图落盘后 gpui 刷新窗口。
        let cover_urls: Vec<String> = self.works.iter().map(|w| w.cover.clone()).collect();
        crate::cover_img::prewarm_covers(&cover_urls, cx);
        // 创作者头像单独一档（64）：与封面不是同一档（成品图按 URL 哈希 + 档位
        // 落盘），不排头像就是整块空白。
        let avatar_urls: Vec<String> = self
            .profile
            .as_ref()
            .map(|p| vec![p.avatar.clone()])
            .unwrap_or_default();
        crate::cover_img::prewarm_avatars(&avatar_urls, cx);
        // 滚动哨兵：滚到底自动续页（Tauri creator-home-page.tsx 的
        // IntersectionObserver 同语义；按钮留作兜底）。
        if self.scroll.should_load(self.has_more, self.loading_more) {
            self.load_more(cx);
        }
        let profile = self.render_profile(cx);
        let heading = (self.works_loaded && !self.works.is_empty()).then(|| {
            div()
                .mt_6()
                .text_base()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .child(SharedString::from(crate::i18n::tf(
                    "creator.works",
                    &[("count", &self.total.to_string())],
                )))
        });
        let body = self.render_body(cx);
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .id("creator-page")
            .child(div().w_full().px_8().pt_6().child(self.render_back_bar(cx)))
            .child(if self.loading && !self.works_loaded {
                // 首拉加载：封面网格骨架铺在作品网格的位置（与真实作品区同
                // 滚动容器 / 同 padding / 同卡宽，数据到位不跳版）
                div()
                    .id("creator-skeleton")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_8()
                    .pb_8()
                    .child(
                        h_flex().w_full().justify_center().items_start().child(
                            v_flex()
                                .w_full()
                                .max_w(gpui_kit::px(1152.0))
                                .min_w_0()
                                .child(crate::pages::skeleton::cover_grid(cx, 10, 200.0, 267.0)),
                        ),
                    )
                    .into_any_element()
            } else {
                div()
                    .id("creator-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(self.scroll.handle())
                    .px_8()
                    .pb_8()
                    .child(
                        h_flex().w_full().justify_center().items_start().child(
                            v_flex()
                                .w_full()
                                .max_w(gpui_kit::px(1152.0))
                                .min_w_0()
                                .child(profile)
                                .children(heading)
                                .child(body),
                        ),
                    )
                    .into_any_element()
            })
    }
}

#[cfg(feature = "screenshot")]
impl CreatorView {
    /// 预览种子：`creator` = 资料头 + 作品网格；`creator-empty` = 空作品态；
    /// `creator-loading` = 首拉加载态（作品网格骨架）。
    pub(crate) fn debug_preview_seed(&mut self, scene: &str, cx: &mut Context<Self>) {
        self.user_id = "2007062623837002".into();
        self.loading = scene == "creator-loading";
        self.failed = None;
        self.profile = Some({
            let mut profile = crate::preview::fixtures::creator_profile();
            profile.avatar = "preview://creator-avatar".into();
            crate::preview::fixtures::put_preview_image(
                profile.avatar.clone(),
                crate::preview::fixtures::cover(9),
            );
            profile
        });
        self.works = if scene == "creator-empty" || scene == "creator-loading" {
            Vec::new()
        } else {
            crate::preview::fixtures::creator_works()
        };
        for (i, work) in self.works.iter_mut().enumerate() {
            let url = format!("preview://creator-work-{i}");
            work.cover = url.clone();
            crate::preview::fixtures::put_preview_image(
                url,
                crate::preview::fixtures::cover(i + 2),
            );
        }
        self.total = self.works.len() as i64;
        self.works_loaded = scene != "creator-loading";
        self.has_more = false;
        cx.notify();
    }
}
