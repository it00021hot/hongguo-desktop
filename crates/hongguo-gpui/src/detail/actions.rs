//! 详情页动作：下半区 tab 切换、「继续看」目标决策与相关推荐预约/取消。

use super::*;

use gpui_kit::component::WindowExt;

impl DetailView {
    /// 程序化切换下半区 tab（取证开关 `HONGGUO_DETAIL_TAB` 用）。
    pub(crate) fn select_tab(&mut self, tab: DetailTab, cx: &mut Context<Self>) {
        self.tab = tab;
        if tab == DetailTab::Related {
            self.load_related_reservations(cx);
        }
        cx.notify();
    }

    /// 「继续看」目标集与起点（Tauri `series-detail-page.tsx` 的
    /// continueIndex 同口径三段决策）：
    /// 1) 本地断点 >3 秒 → 那集那位置（第一真值，播放中 5 秒一写）；
    /// 2) 云端观看历史 >3 秒 → 云端那集那位置（本地 ≤3 秒也按没看过让位）；
    /// 3) 本地有记录（≤3 秒）→ 仍视作看到这一集，从头播（详情页特例）；
    /// 4) 都没有 → 第 1 集从头。
    pub(super) fn resume_entry(&self) -> (u32, f64) {
        let local = self
            .state
            .store
            .series_last_position(&self.series_id)
            .ok()
            .flatten();
        let (idx, start, duration) = if let Some((idx, pos)) = &local
            && pos.is_resumable()
        {
            (*idx, pos.current_time, pos.duration)
        } else {
            let threshold_ms =
                (hongguo_common::model::playback::RESUME_THRESHOLD_SECS * 1000.0) as i64;
            if let Some(h) = self.history.iter().find(|h| h.series_id == self.series_id)
                && h.position_ms > threshold_ms
            {
                (
                    h.vid_index.max(1) as u32,
                    h.position_ms as f64 / 1000.0,
                    h.duration_ms as f64 / 1000.0,
                )
            } else if let Some((idx, _)) = local {
                (idx, 0.0, 0.0)
            } else {
                (1, 0.0, 0.0)
            }
        };
        // 剩余 <2s：这一集已经看完，绝不起播在尾部；还有下一集就直接给
        // 下一集 0s。末集由 play_from_detail 按已看完规则跳下一部。
        if duration > 0.0 && duration - start <= 2.0 {
            let count = self
                .series
                .as_ref()
                .map(|series| series.episodes.len() as u32)
                .unwrap_or(0);
            if idx < count {
                log::info!("[Detail] 第 {idx} 集剩余不足 2s → 下一集 {}", idx + 1);
                return (idx + 1, 0.0);
            }
        }
        (idx, start)
    }

    /// 相关推荐卡「预约/已预约」切换（Tauri ReserveButton 同款：乐观翻转、
    /// 失败回滚、成功 toast；按钮点击不触发整卡跳详情）。
    pub(super) fn toggle_related_reserve(
        &mut self,
        series_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.related_reserve_pending.contains(&series_id) {
            return;
        }
        if !self.logged_in() {
            window.push_notification(
                Notification::info(crate::i18n::t("player.interact.loginRequired")),
                cx,
            );
            return;
        }
        let next = !self.related_reserved.contains(&series_id);
        self.related_reserve_pending.insert(series_id.clone());
        if next {
            self.related_reserved.insert(series_id.clone());
        } else {
            self.related_reserved.remove(&series_id);
        }
        cx.notify();
        let state = Arc::clone(&self.state);
        let sid = series_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::reservation::reserve_series(&sid, next, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.related_reserve_pending.remove(&series_id);
                match result {
                    Ok(()) => window.push_notification(
                        Notification::success(if next {
                            crate::i18n::t("reservation.done")
                        } else {
                            crate::i18n::t("reservation.cancelled")
                        }),
                        cx,
                    ),
                    Err(e) => {
                        // 回滚乐观态
                        if next {
                            view.related_reserved.remove(&series_id);
                        } else {
                            view.related_reserved.insert(series_id.clone());
                        }
                        log::warn!("[Detail] 相关推荐预约失败: {e}");
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
}
