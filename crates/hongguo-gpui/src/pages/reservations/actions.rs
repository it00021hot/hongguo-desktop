//! 预约页动作：取消预约、批量删除与回执、跳转（详情/起播）。

use super::*;

impl ReservationView {
    /// 单条取消预约（未上线卡片的行尾动作）。
    pub(super) fn cancel_reserve(&mut self, series_id: String, cx: &mut Context<Self>) {
        if !self.cancelling.insert(series_id.clone()) {
            return;
        }
        cx.notify();
        let state = Arc::clone(&self.state);
        let sid = series_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::reservation::reserve_series(&sid, false, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.cancelling.remove(&series_id);
                match result {
                    Ok(()) => {
                        window.push_notification(
                            Notification::success(crate::i18n::t("reservation.cancelled")),
                            cx,
                        );
                        view.load(cx);
                    }
                    Err(e) => {
                        log::warn!("[Reservations] 取消预约失败: {e}");
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

    /// 批量删除预约（一个请求，Tauri useReservationsDelete 同款）。
    fn batch_delete(&mut self, cx: &mut Context<Self>) {
        if self.deleting {
            return;
        }
        let ids: Vec<String> = self
            .current_items()
            .iter()
            .filter(|i| self.selected.contains(&i.series_id))
            .map(|i| i.series_id.clone())
            .collect();
        if ids.is_empty() {
            return;
        }
        self.deleting = true;
        cx.notify();
        let total = ids.len();
        let online = self.online_tab;
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::reservation::delete_reservations(&ids, false, &[], online, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.deleting = false;
                match result {
                    Ok(()) => {
                        window.push_notification(
                            Notification::success(crate::i18n::tf(
                                "batch.done",
                                &[("count", &total.to_string())],
                            )),
                            cx,
                        );
                        view.managing = false;
                        view.selected.clear();
                    }
                    Err(e) => {
                        log::warn!("[Reservations] 批量删除失败: {e}");
                        window.push_notification(
                            Notification::error(crate::i18n::tf(
                                "batch.partialFail",
                                &[("error", &crate::i18n::app_error_text(&e))],
                            )),
                            cx,
                        );
                    }
                }
                view.load(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn confirm_batch_delete(&self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let count = self.selected.len();
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            let view = view.clone();
            dialog
                .title(crate::i18n::t("reservation.batchCancelTitle"))
                .description(crate::i18n::tf(
                    "batch.confirmBody",
                    &[("count", &count.to_string())],
                ))
                .ok_text(crate::i18n::t("common.confirm"))
                .cancel_text(crate::i18n::t("common.cancel"))
                .show_cancel(true)
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _window, cx| {
                    view.update(cx, |view, cx| view.batch_delete(cx));
                    true
                })
        });
    }

    pub(super) fn open_detail(&self, item: &CalendarItem, cx: &mut Context<Self>) {
        cx.emit(ReservationEvent::OpenDetail {
            series_id: item.series_id.clone(),
            prefill: DetailPrefill {
                title: item.title.clone(),
                cover: item.cover.clone(),
                tags: item
                    .rec_tags
                    .iter()
                    .filter(|t| !t.is_empty())
                    .cloned()
                    .collect(),
                desc: item.description.clone(),
                reserved: item.has_subscribed,
            },
        });
    }

    pub(super) fn play_series(&self, series_id: &str, cx: &mut Context<Self>) {
        let (vid_index, start_secs) =
            hongguo_service::play_service::resume::resolve_entry(&self.state.store, series_id, &[])
                .ok()
                .unwrap_or((1, 0.0));
        cx.emit(ReservationEvent::Play {
            series_id: series_id.to_string(),
            vid_index,
            start_secs,
        });
    }
}
