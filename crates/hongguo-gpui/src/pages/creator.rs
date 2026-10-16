//! 创作者主页（Tauri `/creator` = `creator-home-page.tsx` 的 GPUI 对齐）。
//!
//! 详情页「出品方/演职员」芯片点入。资料头 = reading/user/profile/get
//! （昵称/头像/简介，资料失败不打断作品列表——官方头部同样可缺）；
//! 作品列表 = commentapi/user/profile/{uid}（游标翻页，3:4 封面卡：
//! 标题 + 集数/热度/播放行，点击起播——统一首页沉浸流）。页面发事件给壳
//! （Play/Back），页面间不互相持有。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, ImageSource, ObjectFit, RenderImage, SharedString, Window, div, img};
use hongguo_api::danmaku::{CreatorProfile, CreatorWork};
use hongguo_service::app_state::AppStateInner;

use crate::bridge;
use crate::home::fmt::format_play_count;
use crate::pages::list_common::decode_cover;

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
    covers: HashMap<String, Arc<RenderImage>>,
    covers_pending: HashSet<String>,
    avatar: Option<Arc<RenderImage>>,
    avatar_pending: bool,
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
            covers: HashMap::new(),
            covers_pending: HashSet::new(),
            avatar: None,
            avatar_pending: false,
        }
    }

    /// 装载一位创作者（资料 + 作品首页并行，各降各的级）。
    pub(crate) fn load(&mut self, user_id: String, cx: &mut Context<Self>) {
        self.user_id = user_id.clone();
        self.profile = None;
        self.avatar = None;
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
                        if !profile.avatar.is_empty() {
                            view.load_avatar(profile.avatar.clone(), cx);
                        }
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
                        view.load_covers(cx);
                    }
                    Err(e) => {
                        log::warn!("[Creator] GPUI 壳作品失败: {e}");
                        view.failed = Some(format!("{e}"));
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
            let _ = this.update(cx, |view, cx| {
                view.loading_more = false;
                match result {
                    Ok(page) => {
                        view.works.extend(page.works);
                        view.cursor = page.cursor;
                        view.has_more = page.has_more;
                        view.load_covers(cx);
                    }
                    Err(e) => log::warn!("[Creator] GPUI 壳翻页失败: {e}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 资料头头像（独立于作品封面缓存：头像是圆形小图，不进 covers 表）。
    fn load_avatar(&mut self, url: String, cx: &mut Context<Self>) {
        if self.avatar_pending {
            return;
        }
        self.avatar_pending = true;
        let rx = crate::cover_img::fetch(url);
        cx.spawn(async move |this, cx| {
            let bytes = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.avatar_pending = false;
                if let Ok(bytes) = bytes
                    && let Ok(decoded) = image::load_from_memory(&bytes)
                {
                    view.avatar = Some(Arc::new(crate::image_upload::render_image(
                        decoded.to_rgba8(),
                    )));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 逐条取作品封面（条目自带 CDN 签名 URL，HEIC 走同一封面阶梯）。
    fn load_covers(&mut self, cx: &mut Context<Self>) {
        for work in &self.works {
            let key = work.series_id.clone();
            if work.cover.is_empty() || !self.covers_pending.insert(key.clone()) {
                continue;
            }
            let rx = crate::cover_img::fetch(work.cover.clone());
            cx.spawn(async move |this, cx| {
                let bytes = rx.await.expect("tokio 桥不会丢弃发送端");
                this.update(cx, |view, cx| {
                    view.covers_pending.remove(&key);
                    if let Ok(bytes) = bytes
                        && let Some(image) = decode_cover(&bytes)
                    {
                        view.covers.insert(key, image);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
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
                    .label("返回")
                    .ghost()
                    .small()
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.emit(CreatorEvent::Back);
                    })),
            )
            .into_any_element()
    }

    /// 资料头：头像（64px 圆）+ 昵称 + 简介（2 行截断）。
    fn render_profile(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let Some(profile) = &self.profile else {
            return div().into_any_element();
        };
        let avatar = self.avatar.clone();
        h_flex()
            .mt_4()
            .items_center()
            .gap_4()
            .child(match avatar {
                Some(image) => img(ImageSource::Render(image))
                    .size(gpui_kit::px(64.0))
                    .flex_shrink_0()
                    .rounded_full()
                    .object_fit(ObjectFit::Cover)
                    .into_any_element(),
                None => h_flex()
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
                    .into_any_element(),
            })
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
        let cover = self.covers.get(&work.series_id).cloned();
        let hot = if work.hot > 0 {
            format_play_count(work.hot)
        } else {
            String::new()
        };
        let plays = if work.play_cnt > 0 {
            format!("{}次播放", format_play_count(work.play_cnt))
        } else {
            String::new()
        };
        let episodes = if work.episode_cnt > 0 {
            format!("全{}集", work.episode_cnt)
        } else {
            String::new()
        };
        // 卡片封面下的信息条：Tauri 是 from-black/85 到透明的上向渐变——
        // 贴图叠加的既有口径（预约页封面角标同族），用同样的半透明黑。
        let overlay_bg = gpui_kit::linear_gradient(
            0.,
            gpui_kit::linear_color_stop(gpui_kit::rgba(0x000000d9), 0.),
            gpui_kit::linear_color_stop(gpui_kit::rgba(0x00000000), 1.),
        );
        div()
            .id(ElementId::from(format!("creator-work-{}", work.series_id)))
            .group("creator-work")
            .w(gpui_kit::px(172.0))
            .flex_shrink_0()
            .rounded(gpui_kit::px(12.0))
            .overflow_hidden()
            .bg(theme.secondary)
            .cursor_pointer()
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
                    .h(gpui_kit::px(229.0))
                    .children(cover.map(|bitmap| {
                        img(ImageSource::Render(bitmap))
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            // Tauri group-hover:opacity-80（悬停封面变淡）
                            .group_hover("creator-work", |s| s.opacity(0.8))
                    }))
                    .child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .bg(overlay_bg)
                            .p_2p5()
                            .pt_8()
                            .child(
                                div()
                                    .text_sm()
                                    .line_clamp(2)
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .text_color(gpui_kit::rgb(0xffffffff))
                                    .child(SharedString::from(work.title.clone())),
                            )
                            .child(
                                h_flex()
                                    .mt_1()
                                    .items_center()
                                    .gap_2()
                                    .text_xs()
                                    .text_color(gpui_kit::rgba(0xffffffb3))
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
        let theme = cx.theme();
        if self.loading && !self.works_loaded {
            return v_flex()
                .pt_20()
                .items_center()
                .child(Spinner::new().small())
                .into_any_element();
        }
        if let Some(err) = self.failed.clone() {
            return v_flex()
                .pt_20()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("主页加载失败"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(SharedString::from(err)),
                )
                .child(
                    Button::new("creator-retry")
                        .label("重试")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let uid = this.user_id.clone();
                            this.load(uid, cx);
                        })),
                )
                .into_any_element();
        }
        if self.works.is_empty() {
            return div()
                .mt_6()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("TA 还没有发布作品")
                .into_any_element();
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
                            "加载中…"
                        } else {
                            "加载更多"
                        })
                        .outline()
                        .small()
                        .disabled(self.loading_more)
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
        let profile = self.render_profile(cx);
        let heading = (self.works_loaded && !self.works.is_empty()).then(|| {
            div()
                .mt_6()
                .text_base()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .child(SharedString::from(format!("作品 · {}", self.total)))
        });
        let body = self.render_body(cx);
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .id("creator-page")
            .child(div().w_full().px_8().pt_6().child(self.render_back_bar(cx)))
            .child(
                div()
                    .id("creator-scroll")
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
                                .child(profile)
                                .children(heading)
                                .child(body),
                        ),
                    ),
            )
    }
}

#[cfg(feature = "screenshot")]
impl CreatorView {
    /// 预览种子：`creator` = 资料头 + 作品网格；`creator-empty` = 空作品态。
    pub(crate) fn debug_preview_seed(&mut self, scene: &str, cx: &mut Context<Self>) {
        self.user_id = "2007062623837002".into();
        self.loading = false;
        self.failed = None;
        self.profile = Some(crate::preview::fixtures::creator_profile());
        self.avatar = Some(crate::preview::fixtures::cover(9));
        self.works = if scene == "creator-empty" {
            Vec::new()
        } else {
            crate::preview::fixtures::creator_works()
        };
        for (i, work) in self.works.iter().enumerate() {
            self.covers.insert(
                work.series_id.clone(),
                crate::preview::fixtures::cover(i + 2),
            );
        }
        self.total = self.works.len() as i64;
        self.works_loaded = true;
        self.has_more = false;
        cx.notify();
    }
}
