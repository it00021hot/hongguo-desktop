//! 排行榜行卡：名次/封面/元信息/热度行，以及行内播放、详情与预约动作。

use super::*;

impl super::RankView {
    fn play_series(&self, series_id: String, cx: &mut Context<Self>) {
        let (vid_index, start_secs) = hongguo_service::play_service::resume::resolve_entry(
            &self.state.store,
            &series_id,
            &[],
        )
        .ok()
        .unwrap_or((1, 0.0));
        cx.emit(RankEvent::Play {
            series_id,
            vid_index,
            start_secs,
        });
    }

    fn open_detail(&self, item: &RankItem, cx: &mut Context<Self>) {
        cx.emit(RankEvent::OpenDetail {
            series_id: item.series_id.clone(),
            prefill: DetailPrefill {
                title: item.title.clone(),
                cover: item.cover.clone(),
                tags: item.tags.clone(),
                desc: item.description.clone(),
                reserved: self.reserved_of(item),
            },
        });
    }

    fn reserved_of(&self, item: &RankItem) -> bool {
        self.reserved_local
            .get(&item.series_id)
            .copied()
            .unwrap_or(item.reserved)
    }

    /// 行内预约切换（乐观翻转，与 Tauri RankRow 同款）。
    fn toggle_reserve(&mut self, item: &RankItem, cx: &mut Context<Self>) {
        let sid = item.series_id.clone();
        if !self.reserving.insert(sid.clone()) {
            return;
        }
        let next = !self.reserved_of(item);
        cx.notify();
        let state = Arc::clone(&self.state);
        let sid_for_call = sid.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::reservation::reserve_series(&sid_for_call, next, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.reserving.remove(&sid);
                match result {
                    Ok(()) => {
                        view.reserved_local.insert(sid.clone(), next);
                        window.push_notification(
                            Notification::success(if next {
                                crate::i18n::t("reservation.done")
                            } else {
                                crate::i18n::t("reservation.cancelled")
                            }),
                            cx,
                        );
                    }
                    Err(e) => {
                        log::warn!("[Rank] 预约失败: {e}");
                        let message = crate::i18n::app_error_text(&e);
                        window.push_notification(
                            Notification::error(crate::i18n::tf(
                                "common.failedWith",
                                &[("message", message.as_str())],
                            )),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_row(
        &self,
        item: &RankItem,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let cover = self.covers.get(&item.cover).cloned();
        let reserved = self.reserved_of(item);
        let rank_color: gpui_kit::Hsla = if item.rank <= 3 {
            // 星语言（Tauri 榜单前三 text-star 同款）
            crate::tokens::star(theme.mode).into()
        } else {
            theme.muted_foreground.opacity(0.5)
        };
        let mut meta: Vec<String> = Vec::new();
        if !item.sub_title.is_empty() {
            meta.push(item.sub_title.clone());
        }
        if item.score > 0.0 {
            meta.push(format!("{:.1}", item.score));
        }
        meta.extend(item.tags.clone());
        let secondary: Vec<String> = item
            .secondary_infos
            .iter()
            .filter(|s| !s.is_empty() && **s != item.rec_text)
            .cloned()
            .collect();
        h_flex()
            .id(ElementId::from(format!("rank-row-{}", item.series_id)))
            .items_center()
            .gap_4()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.group_box)
            .p_3()
            .cursor_pointer()
            .hover(|s| s.border_color(theme.foreground.opacity(0.3)).shadow_md())
            .on_click({
                let item = item.clone();
                cx.listener(move |this, _, _, cx| this.open_detail(&item, cx))
            })
            .children((item.rank > 0).then(|| {
                div()
                    .w_8()
                    .flex_shrink_0()
                    .text_center()
                    .text_2xl()
                    .font_weight(gpui_kit::FontWeight::BLACK)
                    .text_color(rank_color)
                    .child(SharedString::from(item.rank.to_string()))
            }))
            .child(
                div()
                    .w(gpui_kit::px(80.0))
                    .h(gpui_kit::px(107.0))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .rounded(theme.radius + gpui_kit::px(2.0))
                    .bg(theme.secondary)
                    .child(match cover {
                        Some(bitmap) => img(ImageSource::Render(bitmap))
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element(),
                        None => div().size_full().into_any_element(),
                    }),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .gap_1()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1p5()
                            .child(
                                div()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(SharedString::from(item.title.clone())),
                            )
                            .children((!item.season.is_empty()).then(|| {
                                div()
                                    .flex_shrink_0()
                                    .rounded(gpui_kit::px(4.0))
                                    .border_1()
                                    .border_color(theme.border)
                                    .px_1()
                                    .text_size(gpui_kit::px(10.0))
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(item.season.clone()))
                            })),
                    )
                    .children((!meta.is_empty()).then(|| {
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(meta.join(" · ")))
                    }))
                    .children((!item.description.is_empty()).then(|| {
                        div()
                            .text_xs()
                            .line_height(gpui_kit::relative(1.625))
                            .text_color(theme.muted_foreground.opacity(0.8))
                            .line_clamp(2)
                            .overflow_hidden()
                            .child(SharedString::from(item.description.clone()))
                    }))
                    .children(
                        (!item.rec_text.is_empty() || !secondary.is_empty()).then(|| {
                            h_flex()
                                .items_center()
                                .gap_2()
                                .text_xs()
                                .children((!item.rec_text.is_empty()).then(|| {
                                    h_flex()
                                        .flex_shrink_0()
                                        .items_center()
                                        .gap_0p5()
                                        // 推荐语（热度）走业务令牌（Tauri 榜单行 text-warm）
                                        .text_color(crate::tokens::warm(theme.mode))
                                        .child(
                                            gpui_kit::component::Icon::new(
                                                gpui_kit::assets::IconName::Flame,
                                            )
                                            .small(),
                                        )
                                        .child(SharedString::from(item.rec_text.clone()))
                                }))
                                .children((!secondary.is_empty()).then(|| {
                                    div()
                                        .truncate()
                                        .text_color(theme.muted_foreground)
                                        .child(SharedString::from(secondary.join("  ")))
                                }))
                        }),
                    ),
            )
            .child(
                v_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_1p5()
                    .children(if item.upcoming {
                        let item_clone = item.clone();
                        let pending = self.reserving.contains(&item.series_id);
                        vec![
                            Button::new(ElementId::from(format!(
                                "rank-reserve-{}",
                                item.series_id
                            )))
                            .icon(if reserved {
                                gpui_kit::assets::IconName::Check
                            } else {
                                gpui_kit::assets::IconName::Bell
                            })
                            .label(if reserved {
                                crate::i18n::t("reservation.reserved")
                            } else {
                                crate::i18n::t("reservation.action")
                            })
                            .outline()
                            .small()
                            .h_8()
                            .px_2p5()
                            .disabled(pending)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.toggle_reserve(&item_clone, cx);
                            }))
                            .into_any_element(),
                        ]
                    } else {
                        let sid = item.series_id.clone();
                        let item_clone = item.clone();
                        vec![
                            Button::new(ElementId::from(format!("rank-play-{}", item.series_id)))
                                .icon(gpui_kit::assets::IconName::Play)
                                .label(crate::i18n::t("player.play"))
                                .outline()
                                .small()
                                .h_8()
                                .px_2p5()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.play_series(sid.clone(), cx);
                                }))
                                .into_any_element(),
                            Button::new(ElementId::from(format!("rank-detail-{}", item.series_id)))
                                .label(crate::i18n::t("rank.detail"))
                                .text()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.open_detail(&item_clone, cx);
                                }))
                                .into_any_element(),
                        ]
                    }),
            )
            .into_any_element()
    }
}
