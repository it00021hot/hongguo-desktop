//! 剧评 tab：发评框（评分 + 表情 + 文本，Tauri ReviewComposer 同款）+
//! 评分摘要 + 分页列表（Tauri ReviewList 同款）。数据走
//! fetch_series_comments_page（group_type=1 剧级一条线，与播放器单集评论
//! 分属不同 group_id）；发送走 send_series_review（十分制 = 星 ×2）。

use std::sync::Arc;

use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, ImageSource, ObjectFit, SharedString, Window, div, img};
use hongguo_api::danmaku::fetch_series_comments_page;

use super::DetailView;
use super::header::star_color;
use crate::bridge;

impl DetailView {
    /// 剧评首页（评分摘要取自首页 extra；换剧重置由 load 统一做）。
    pub(super) fn load_reviews_first_page(&mut self, cx: &mut Context<Self>) {
        if self.reviews_loading {
            return;
        }
        self.reviews_loading = true;
        let sid = self.series_id.clone();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            fetch_series_comments_page(&sid, "", &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.reviews_loading = false;
                match result {
                    Ok(page) => {
                        view.reviews_total = page.page.total;
                        view.reviews_score = page.score.clone();
                        view.reviews_score_cnt = page.score_cnt;
                        view.reviews_has_more = page.page.has_more;
                        view.reviews_cursor = page.page.next_cursor.clone();
                        view.reviews = Arc::new(page.page.items.clone());
                        for url in page.page.items.iter().map(|c| c.avatar.clone()) {
                            view.load_image(url, cx);
                        }
                        // 评分摘要变了要回头刷新头部统计行
                        cx.notify();
                    }
                    Err(e) => log::warn!("[Detail] 剧评拉取失败: {e}"),
                }
            });
        })
        .detach();
    }

    /// 剧评翻页（相邻页重叠条目按 comment_id 去重，首页评论同款）。
    pub(super) fn load_reviews_next(&mut self, cx: &mut Context<Self>) {
        if self.reviews_loading || !self.reviews_has_more {
            return;
        }
        self.reviews_loading = true;
        let sid = self.series_id.clone();
        let cursor = self.reviews_cursor.clone();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            fetch_series_comments_page(&sid, &cursor, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.reviews_loading = false;
                if let Ok(page) = result {
                    view.reviews_has_more = page.page.has_more;
                    view.reviews_cursor = page.page.next_cursor;
                    let mut items = (*view.reviews).clone();
                    for c in page.page.items {
                        if !items.iter().any(|x| x.comment_id == c.comment_id) {
                            view.load_image(c.avatar.clone(), cx);
                            items.push(c);
                        }
                    }
                    view.reviews = Arc::new(items);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 发表剧评（Tauri ReviewComposer 同款）：评分必选（十分制 = 星 ×2）、
    /// 正文非空；成功后回执 + 清空输入框 + 重拉首页让新剧评置顶。
    pub(super) fn submit_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = &self.review_input else {
            return;
        };
        let text = input.read(cx).value().trim().to_string();
        if text.is_empty() || self.review_stars == 0 || self.review_sending {
            return;
        }
        if !self.logged_in() {
            // 登录墙（Tauri 由后端拒绝 + 全局 toast；这里就地给回执）
            window.push_notification(Notification::error("登录后才能发表剧评"), cx);
            return;
        }
        let score = i64::from(self.review_stars) * 2;
        self.review_sending = true;
        cx.notify();
        let state = Arc::clone(&self.state);
        let sid = self.series_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::danmaku::send_series_review(&sid, &text, score, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.review_sending = false;
                match result {
                    Ok(_) => {
                        window.push_notification(
                            Notification::success(format!("剧评已发布（{score} 分）")),
                            cx,
                        );
                        // 评分保留、正文清空（Tauri setText('') 同款）
                        view.review_clear_input = true;
                        // 重拉首页置顶新剧评（Tauri invalidate 重取同款）
                        view.load_reviews_first_page(cx);
                    }
                    Err(e) => {
                        log::warn!("[Detail] 剧评发布失败: {e}");
                        window.push_notification(Notification::error(format!("发布失败：{e}")), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 发评框（Tauri ReviewComposer 同款：我的评分五星 + 表情 + 文本 +
    /// 「发表剧评」；未选评分或正文为空禁发；Enter 提交见 mod.rs 的订阅）。
    fn render_review_composer(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let shown = if self.review_hover_stars > 0 {
            self.review_hover_stars
        } else {
            self.review_stars
        };
        let has_text = self
            .review_input
            .as_ref()
            .is_some_and(|i| !i.read(cx).value().trim().is_empty());
        let can_send = has_text && self.review_stars >= 1 && !self.review_sending;
        let mut stars = h_flex().items_center().gap_2().child(
            div()
                .text_sm()
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .child("我的评分"),
        );
        for s in 1..=5u8 {
            stars = stars.child(
                div()
                    .id(ElementId::from(format!("review-star-{s}")))
                    .p_0p5()
                    .cursor_pointer()
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        let next = if *hovered {
                            s
                        } else if this.review_hover_stars == s {
                            0
                        } else {
                            this.review_hover_stars
                        };
                        if this.review_hover_stars != next {
                            this.review_hover_stars = next;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.review_stars = s;
                        this.review_hover_stars = 0;
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::Star)
                            .size_5()
                            .text_color(if s <= shown {
                                star_color().into()
                            } else {
                                theme.muted_foreground
                            }),
                    ),
            );
        }
        stars = stars.children((self.review_stars > 0).then(|| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child("已选评分，发布后生效")
        }));
        let mut card = v_flex()
            .rounded(gpui_kit::px(8.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.group_box)
            .p_3()
            .gap_2()
            .child(stars)
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("review-emoji-btn")
                            .px_1()
                            .cursor_pointer()
                            .text_color(if self.review_emoji_open {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            })
                            .hover(|s| s.text_color(theme.foreground))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.review_emoji_open = !this.review_emoji_open;
                                cx.notify();
                            }))
                            .child(Icon::new(gpui_kit::assets::IconName::FaceGrinning).size_4()),
                    )
                    .children(
                        self.review_input.as_ref().map(|input| {
                            Input::new(input).flex_1().h(gpui_kit::px(40.0)).text_sm()
                        }),
                    )
                    .child(
                        Button::new("review-send")
                            .label(if self.review_sending {
                                "发布中…"
                            } else {
                                "发表剧评"
                            })
                            .small()
                            .disabled(!can_send)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.submit_review(window, cx);
                            })),
                    ),
            );
        if self.review_emoji_open {
            // 表情网格（8 列，与评论面板同一套资产；点格子光标处插入
            // `[名字]`，连选不关）
            card = card.child(
                div()
                    .id("review-emoji-grid")
                    .max_h(gpui_kit::px(132.0))
                    .overflow_y_scroll()
                    .flex()
                    .flex_wrap()
                    .gap_0p5()
                    .children(crate::danmaku_emoji::EMOJI_FILES.iter().map(|(name, _)| {
                        let code = name.to_string();
                        let img = crate::danmaku_emoji::emoji_by_name(name);
                        let input = self.review_input.clone();
                        div()
                            .id(ElementId::from(format!("review-emoji-{code}")))
                            .size_7()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(gpui_kit::px(6.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.secondary_hover))
                            .on_click(move |_, window, cx| {
                                if let Some(input) = &input {
                                    input.update(cx, |state, cx| {
                                        state.insert(code.clone(), window, cx);
                                    });
                                }
                            })
                            .children(img.map(|img| {
                                gpui_kit::img(gpui_kit::ImageSource::Render(img))
                                    .size_5()
                                    .object_fit(gpui_kit::ObjectFit::Contain)
                            }))
                    })),
            );
        }
        card.into_any_element()
    }

    pub(super) fn render_reviews(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        v_flex()
            .mt_4()
            .gap_4()
            // 发评框（Tauri ReviewComposer 同位：列表在它下面）
            .child(self.render_review_composer(cx))
            // 评分摘要（剧均评分 + 评分人数；空 = 暂无评分整行不渲染）
            .children((!self.reviews_score.is_empty()).then(|| {
                h_flex()
                    .items_baseline()
                    .gap_2()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(gpui_kit::FontWeight::BOLD)
                            .text_color(gpui_kit::rgba(0xfacc15ff))
                            .child(format!(
                                "{:.1}",
                                self.reviews_score.parse::<f64>().unwrap_or(0.0)
                            )),
                    )
                    .child(div().text_sm().text_color(theme.muted_foreground).child(
                        if self.reviews_score_cnt > 0 {
                            format!("{}人评分", DetailView::fmt_count(self.reviews_score_cnt))
                        } else {
                            "剧均评分".to_string()
                        },
                    ))
            }))
            // 剧评列表（头像 + 昵称 + 评分行 + 时间 + 正文 + 点赞数；
            // 评分行只有剧评有——单集评论 score 恒空）
            .children(self.reviews.iter().map(|c| {
                let avatar = self.images.get(&c.avatar).cloned();
                h_flex()
                    .items_start()
                    .gap_3()
                    .child(
                        div()
                            .size_9()
                            .flex_shrink_0()
                            .overflow_hidden()
                            .rounded_full()
                            .bg(theme.secondary)
                            .children(avatar.map(|bitmap| {
                                img(ImageSource::Render(bitmap))
                                    .size_full()
                                    .object_fit(ObjectFit::Cover)
                            })),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .child(SharedString::from(c.user_name.clone())),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .children((!c.score.is_empty()).then(|| {
                                        div()
                                            .px_1p5()
                                            .rounded(gpui_kit::px(4.0))
                                            .text_xs()
                                            .bg(gpui_kit::rgba(0xfacc1526))
                                            .text_color(gpui_kit::rgba(0xfacc15ff))
                                            .child(SharedString::from(format!("{}分", c.score)))
                                    }))
                                    .child(
                                        div().text_xs().text_color(theme.muted_foreground).child(
                                            SharedString::from(crate::home::fmt::relative_time(
                                                c.create_time,
                                            )),
                                        ),
                                    ),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .line_height(gpui_kit::relative(1.6))
                                    .child(SharedString::from(c.text.clone())),
                            )
                            .children((c.digg_count > 0).then(|| {
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(format!("♥ {}", c.digg_count)))
                            })),
                    )
            }))
            .children(self.reviews.is_empty().then(|| {
                div()
                    .py_6()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(if self.reviews_loading {
                        "加载中…"
                    } else {
                        "暂无剧评"
                    })
            }))
            // 加载更多（Tauri 是滚动懒加载；GPUI 详情页滚动容器裁判复杂，
            // 按钮等价且省一档哨兵）
            .children(
                (self.reviews_has_more && !self.reviews.is_empty()).then(|| {
                    Button::new("detail-reviews-more")
                        .label(if self.reviews_loading {
                            "加载中…"
                        } else {
                            "加载更多剧评"
                        })
                        .secondary()
                        .small()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.load_reviews_next(cx);
                        }))
                }),
            )
            .into_any_element()
    }
}
