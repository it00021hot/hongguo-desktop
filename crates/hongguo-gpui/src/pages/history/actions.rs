//! 历史页动作：删除记录（批量 + 确认弹窗）、继续观看/跳集起播、多选切换。

use super::*;
use hongguo_common::AppError;

impl HistoryView {
    /// 批量删除历史：云端一条请求（useWatchHistoryDelete 同款）；本地逐剧
    /// 删（useLocalHistoryDelete 同款）。
    fn batch_delete(&mut self, cx: &mut Context<Self>) {
        if self.deleting {
            return;
        }
        let refs: Vec<(String, String, i64)> = self
            .items
            .iter()
            .filter(|i| self.selected.contains(&i.series_id))
            .map(|i| (i.series_id.clone(), i.vid.clone(), i.vid_index))
            .collect();
        if refs.is_empty() {
            return;
        }
        self.deleting = true;
        cx.notify();
        let total = refs.len();
        let local_mode = !self.logged_in();
        let state_local = Arc::clone(&self.state);
        let state_cloud = Arc::clone(&self.state);
        let ids: Vec<String> = refs.iter().map(|(sid, _, _)| sid.clone()).collect();
        let rx = bridge::spawn(async move {
            if local_mode {
                // 本地模式：逐剧删（只动本机，Tauri localDelete 同款）
                tokio::task::spawn_blocking(move || state_local.store.delete_local_history(&ids))
                    .await
                    .map_err(|e| AppError::Internal(format!("本地历史删除阻塞池任务失败: {e}")))?
            } else {
                let env = state_cloud.api_env();
                hongguo_api::history::delete_watch_history(&refs, &env).await
            }
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
                        log::warn!("[History] 批量删除失败: {e}");
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
                .title(crate::i18n::t("history.deleteConfirmTitle"))
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

    pub(super) fn open_item(&self, item: &WatchHistoryItem, cx: &mut Context<Self>) {
        // 已看完的剧不续播：在历史列表里按顺序找下一部未看完的（末尾回绕）。
        // 这一层先挡一次，即使 HomeView 的历史缓存还没就绪也不会起播放器。
        let target = if item.is_finished() {
            let start = self
                .items
                .iter()
                .position(|row| row.series_id == item.series_id)
                .map(|index| index + 1)
                .unwrap_or(0);
            self.items[start..]
                .iter()
                .chain(self.items[..start].iter())
                .find(|row| !row.is_finished())
        } else {
            Some(item)
        };
        let Some(target) = target else {
            log::info!("[History] 已看完且无未完成项，跳过 {}", item.series_id);
            return;
        };
        // 云端 vid_index 1 起（历史数据可能混 0，钳到第 1 集）；进度直接
        // 取自本行（页数据源就是云端历史，等价 Tauri 播放器的云端兜底）。
        // 剩余 <2s 视为这一集已看完：直接给下一集 0s，绝不起播在尾部。
        let mut vid_index = target.vidx_1based() as u32;
        let mut start_secs = (target.position_ms as f64 / 1000.0).max(0.0);
        if target.duration_ms > 0 && target.duration_ms - target.position_ms <= 2000 {
            vid_index = vid_index.saturating_add(1);
            start_secs = 0.0;
            log::info!(
                "[History] 《{}》剩余不足 2s → 下一集 {vid_index}",
                target.series_id
            );
        }
        if target.series_id != item.series_id {
            log::info!(
                "[History] 已看完《{}》→ 跳过，接下一部《{}》",
                item.series_id,
                target.series_id
            );
        }
        cx.emit(HistoryEvent::Play {
            series_id: target.series_id.clone(),
            vid_index,
            start_secs,
        });
    }

    pub(super) fn toggle_selected(&mut self, series_id: &str, cx: &mut Context<Self>) {
        if !self.selected.remove(series_id) {
            self.selected.insert(series_id.to_string());
        }
        cx.notify();
    }
}
