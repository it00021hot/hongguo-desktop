//! 下载管理页（Tauri `/tasks` = `tasks-page.tsx` 的 GPUI 对齐）。
//!
//! 表格：多选列 + 剧名（含失败原因）+ 集数 + 状态徽标 + 大小（下载中给
//! 进度条）+ 行内操作（停止/重试/打开文件夹/删除）；顶部一行队列状态与
//! 批量操作（一键启动/暂停/重试全部失败/全选/选中失败项/扫描补登记）；
//! 有选中时吸底条给「可释放体积 + 删除」。数据与操作走
//! `hongguo_service::download_service`（queue 快照 + actions），与 Tauri
//! 的 download command 同一段；进度靠 `download-*` 事件即时刷新。

use std::collections::BTreeSet;
use std::sync::Arc;

use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, SharedString, Window, div};
use hongguo_common::model::{DownloadTask, QueueStatus, TaskStatus};
use hongguo_service::app_state::AppStateInner;
use hongguo_service::events::ServiceEvent;

use crate::home::fmt::format_bytes;
use crate::ui_button::button as Button;

/// 列宽（表头与数据行共用一份，改一处两处同步）。
/// 勾选列按复选框本身的宽度给窄列；标题列自适应，其余定宽。
const COL_CHECK_W: f32 = 16.0;
const COL_EPISODE_W: f32 = 96.0;
const COL_STATUS_W: f32 = 112.0;
const COL_SIZE_W: f32 = 128.0;
const COL_ACTIONS_W: f32 = 120.0;

pub(crate) struct TasksView {
    state: Arc<AppStateInner>,
    /// 全量任务快照（None = 还没拉过）。
    tasks: Option<Vec<DownloadTask>>,
    queue_status: Option<QueueStatus>,
    selected: BTreeSet<String>,
    deleting: bool,
    rescanning: bool,
}

impl TasksView {
    pub(crate) fn new(state: Arc<AppStateInner>, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            state,
            tasks: None,
            queue_status: None,
            selected: BTreeSet::new(),
            deleting: false,
            rescanning: false,
        };
        view.subscribe_download_events(cx);
        view
    }

    /// 进入页面时拉一次快照。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        self.refresh(cx);
    }

    /// 任务与队列状态都是本地内存快照，直读即刷（与下载面板同款）。
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.tasks = Some(self.state.queue().all());
        self.queue_status = Some(self.state.queue().status());
        // 已不存在的 task id 从选中集里摘掉
        if let Some(tasks) = &self.tasks {
            let ids: std::collections::HashSet<&str> =
                tasks.iter().map(|t| t.id.as_str()).collect();
            self.selected.retain(|id| ids.contains(id.as_str()));
        }
        cx.notify();
    }

    /// 订阅下载事件：进度/状态变化即时刷新（Tauri query 失效重拉同口径）。
    fn subscribe_download_events(&mut self, cx: &mut Context<Self>) {
        let mut rx = crate::events::subscribe(&self.state);
        cx.spawn(async move |this, cx| {
            while let Some(event) = rx.recv().await {
                let relevant = matches!(
                    event,
                    ServiceEvent::DownloadTaskAdded(_)
                        | ServiceEvent::DownloadProgress(_)
                        | ServiceEvent::DownloadCompleted(_)
                        | ServiceEvent::DownloadFailed(_)
                        | ServiceEvent::DownloadStopped(_)
                        | ServiceEvent::DownloadQueueChanged(_)
                );
                if !relevant {
                    continue;
                }
                let ok = this.update(cx, |view, cx| view.refresh(cx)).is_ok();
                if !ok {
                    break; // 视图已销毁
                }
            }
        })
        .detach();
    }

    fn failed_ids(&self) -> Vec<String> {
        self.tasks
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter(|t| t.status == TaskStatus::Failed)
            .map(|t| t.id.clone())
            .collect()
    }

    /// 选中项里已完成任务的字节和（只有这些删文件才真能释放空间）。
    fn freeable_of(&self, ids: &[String]) -> u64 {
        let set: std::collections::HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        self.tasks
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter(|t| set.contains(t.id.as_str()) && t.status == TaskStatus::Completed)
            .map(|t| t.total)
            .sum()
    }

    fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.selected.remove(id) {
            self.selected.insert(id.to_string());
        }
        cx.notify();
    }

    fn toggle_all(&mut self, cx: &mut Context<Self>) {
        let all: Vec<String> = self
            .tasks
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|t| t.id.clone())
            .collect();
        if !all.is_empty() && self.selected.len() >= all.len() {
            self.selected.clear();
        } else {
            self.selected = all.into_iter().collect();
        }
        cx.notify();
    }

    fn delete_tasks(
        &mut self,
        ids: Vec<String>,
        with_files: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.deleting {
            return;
        }
        self.deleting = true;
        cx.notify();
        let result =
            hongguo_service::download_service::actions::delete_tasks(&self.state, &ids, with_files);
        self.deleting = false;
        match result {
            Ok(_) => {
                self.selected.clear();
                self.refresh(cx);
            }
            Err(e) => {
                log::warn!("[Tasks] 删除任务失败: {e}");
                let message = crate::i18n::app_error_text(&e);
                window.push_notification(
                    Notification::error(crate::i18n::tf(
                        "common.failedWith",
                        &[("message", message.as_str())],
                    )),
                    cx,
                );
                // 失败留在列表上（refresh 会照出真实状态），不吞
                self.refresh(cx);
            }
        }
    }

    /// 删除确认（Tauri AlertDialog 的两个动作合成底部三键：取消 / 仅删除
    /// 记录 / 删除任务和文件——勾选框在静态弹窗里做不到联动回显，
    /// 把选择直接展开成两个明确动作，操作集不变）。
    fn confirm_delete(&self, ids: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let n = ids.len();
        let completed = self
            .tasks
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter(|t| ids.contains(&t.id) && t.status == TaskStatus::Completed)
            .count();
        let freeable = format_bytes(self.freeable_of(&ids));
        window.open_dialog(cx, move |dialog, _window, _cx| {
            let ids_only = ids.clone();
            let ids_files = ids.clone();
            let view_only = view.clone();
            let view_files = view.clone();
            dialog
                .title(crate::i18n::tf(
                    "tasks.removeConfirm",
                    &[("count", &n.to_string())],
                ))
                .content({
                    let freeable = freeable.clone();
                    move |content, _window, _cx| {
                        content.child(
                            v_flex()
                                .gap_2()
                                .child(SharedString::from(crate::i18n::tf(
                                    "tasks.removeSummary",
                                    &[
                                        ("done", &completed.to_string()),
                                        ("size", freeable.as_str()),
                                    ],
                                )))
                                .child(
                                    div()
                                        .text_sm()
                                        .child(crate::i18n::t("tasks.removeKeepHint")),
                                ),
                        )
                    }
                })
                .footer(
                    h_flex()
                        .items_center()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("tasks-del-cancel")
                                .label(crate::i18n::t("common.cancel"))
                                .ghost()
                                .small()
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                }),
                        )
                        .child(
                            Button::new("tasks-del-only")
                                .label(crate::i18n::t("tasks.removeOnly"))
                                .outline()
                                .small()
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    view_only.update(cx, |view, cx| {
                                        view.delete_tasks(ids_only.clone(), false, window, cx)
                                    });
                                }),
                        )
                        .child(
                            Button::new("tasks-del-files")
                                .label(crate::i18n::t("tasks.removeAndFiles"))
                                .danger()
                                .small()
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    view_files.update(cx, |view, cx| {
                                        view.delete_tasks(ids_files.clone(), true, window, cx)
                                    });
                                }),
                        ),
                )
        });
    }

    /// 扫描补登记（fs 扫描，走阻塞池；结果 toast——补回 N 条 / 无可补）。
    fn rescan(&mut self, cx: &mut Context<Self>) {
        if self.rescanning {
            return;
        }
        self.rescanning = true;
        cx.notify();
        let state = Arc::clone(&self.state);
        let rx = crate::bridge::spawn(async move {
            tokio::task::spawn_blocking(move || {
                hongguo_service::download_service::actions::rescan_downloads(&state)
            })
            .await
            .map_err(|e| format!("阻塞池任务失败: {e}"))
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.rescanning = false;
                // 阻塞池 join 失败没有 AppError 可翻译，直接透出短句。
                let outcome: Result<usize, String> = match result {
                    Ok(Ok(count)) => Ok(count),
                    Ok(Err(e)) => {
                        log::warn!("[Tasks] 扫描补登记失败: {e}");
                        Err(crate::i18n::app_error_text(&e))
                    }
                    Err(msg) => {
                        log::warn!("[Tasks] 扫描补登记失败: {msg}");
                        Err(msg)
                    }
                };
                match outcome {
                    Ok(count) if count > 0 => {
                        window.push_notification(
                            Notification::success(crate::i18n::tf(
                                "tasks.rescanFound",
                                &[("count", &count.to_string())],
                            )),
                            cx,
                        );
                    }
                    Ok(_) => {
                        window.push_notification(
                            Notification::info(crate::i18n::t("tasks.rescanNone")),
                            cx,
                        );
                    }
                    Err(message) => {
                        window.push_notification(
                            Notification::error(crate::i18n::tf(
                                "common.failedWith",
                                &[("message", message.as_str())],
                            )),
                            cx,
                        );
                    }
                }
                view.refresh(cx);
            });
        })
        .detach();
    }

    /// 打开某剧的下载文件夹（首个有文件路径的任务之文件定位）。
    fn open_folder(&self, series_id: &str) {
        let path = self
            .state
            .queue()
            .of_series(series_id)
            .into_iter()
            .map(|t| t.file_path)
            .find(|p| !p.is_empty());
        match path {
            Some(p) => crate::pages::reveal_in_folder(&p),
            None => log::warn!("[Tasks] 剧集 {series_id} 没有已下载文件"),
        }
    }

    // —— 渲染 ——

    fn render_toolbar(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let status_line = self.queue_status.as_ref().map(|s| {
            let queue = crate::i18n::tf(
                "tasks.queueStatus",
                &[
                    ("running", &s.running.to_string()),
                    ("pending", &s.pending.to_string()),
                    ("completed", &s.completed.to_string()),
                    ("failed", &s.failed.to_string()),
                ],
            );
            let concurrency = crate::i18n::tf(
                "tasks.concurrency",
                &[
                    ("active", &s.active.to_string()),
                    ("limit", &s.limit.to_string()),
                ],
            );
            format!("{queue} · {concurrency}")
        });
        let all_empty = self.tasks.as_deref().map(|t| t.is_empty()).unwrap_or(true);
        let failed = self.failed_ids();
        let all_ids: Vec<String> = self
            .tasks
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|t| t.id.clone())
            .collect();
        let all_checked = !all_ids.is_empty() && self.selected.len() >= all_ids.len();
        h_flex()
            .w_full()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .children(status_line.map(SharedString::from)),
            )
            .child(div().flex_1())
            .child(
                Button::new("tasks-resume-all")
                    .icon(gpui_kit::assets::IconName::Play)
                    .label(crate::i18n::t("tasks.startAll"))
                    .outline()
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let _ = hongguo_service::download_service::actions::resume_all(&this.state);
                        this.refresh(cx);
                    })),
            )
            .child(
                Button::new("tasks-pause-all")
                    .icon(gpui_kit::assets::IconName::Pause)
                    .label(crate::i18n::t("tasks.pauseAll"))
                    .outline()
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let _ = hongguo_service::download_service::actions::pause_all(&this.state);
                        this.refresh(cx);
                    })),
            )
            .child(
                Button::new("tasks-retry-all")
                    .icon(gpui_kit::assets::IconName::RotateCcw)
                    .label(crate::i18n::t("tasks.retryAll"))
                    .outline()
                    .small()
                    .disabled(failed.is_empty())
                    .when(failed.is_empty(), |b| b.cursor_default())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let _ = hongguo_service::download_service::actions::retry_tasks(
                            &this.state,
                            &this.failed_ids(),
                        );
                        this.refresh(cx);
                    })),
            )
            .child(
                Button::new("tasks-select-all")
                    .label(if all_checked {
                        crate::i18n::t("tasks.deselectAll")
                    } else {
                        crate::i18n::t("tasks.selectAll")
                    })
                    .outline()
                    .small()
                    .disabled(all_empty)
                    .when(all_empty, |b| b.cursor_default())
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_all(cx))),
            )
            .child(
                Button::new("tasks-select-failed")
                    .label(crate::i18n::t("tasks.selectFailed"))
                    .outline()
                    .small()
                    .disabled(failed.is_empty())
                    .when(failed.is_empty(), |b| b.cursor_default())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.selected = this.failed_ids().into_iter().collect();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("tasks-rescan")
                    .icon(gpui_kit::assets::IconName::ScanSearch)
                    .label(if self.rescanning {
                        crate::i18n::t("common.loading")
                    } else {
                        crate::i18n::t("tasks.rescan")
                    })
                    .outline()
                    .small()
                    .disabled(self.rescanning)
                    .when(self.rescanning, |b| b.cursor_default())
                    .on_click(cx.listener(|this, _, _, cx| this.rescan(cx))),
            )
            .into_any_element()
    }

    fn status_tag(status: TaskStatus) -> (String, Tag) {
        match status {
            TaskStatus::Pending => (crate::i18n::t("tasks.status.pending"), Tag::secondary()),
            TaskStatus::Running => (crate::i18n::t("tasks.status.running"), Tag::primary()),
            TaskStatus::Completed => (crate::i18n::t("tasks.status.completed"), Tag::success()),
            TaskStatus::Failed => (crate::i18n::t("tasks.status.failed"), Tag::danger()),
            TaskStatus::Stopped => (crate::i18n::t("tasks.status.stopped"), Tag::warning()),
        }
    }

    /// 表头行。列宽与数据行共用同一组常量，标题列自适应。
    fn render_header_row(&self, cx: &mut Context<Self>) -> TableRow {
        let theme = cx.theme();
        let total = self.tasks.as_deref().map(|t| t.len()).unwrap_or(0);
        let all_checked = total > 0 && self.selected.len() >= total;
        TableRow::new()
            .px_4()
            .py_2()
            .gap_3()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(
                TableHead::new()
                    .w(gpui_kit::px(COL_CHECK_W))
                    .min_w(gpui_kit::px(COL_CHECK_W))
                    .flex_shrink_0()
                    .px_0()
                    .py_0()
                    .child(
                        Checkbox::new("tasks-check-all")
                            .checked(all_checked)
                            .on_click(cx.listener(|this, _: &bool, _, cx| {
                                this.toggle_all(cx);
                            })),
                    ),
            )
            .child(
                TableHead::new()
                    .flex_1()
                    .min_w_0()
                    .px_0()
                    .py_0()
                    .child(div().flex_1().child(crate::i18n::t("nav.tasks.title"))),
            )
            .child(
                TableHead::new()
                    .w(gpui_kit::px(COL_EPISODE_W))
                    .min_w(gpui_kit::px(COL_EPISODE_W))
                    .flex_shrink_0()
                    .px_0()
                    .py_0()
                    .child(crate::i18n::t("tasks.columns.episode")),
            )
            .child(
                TableHead::new()
                    .w(gpui_kit::px(COL_STATUS_W))
                    .min_w(gpui_kit::px(COL_STATUS_W))
                    .flex_shrink_0()
                    .px_0()
                    .py_0()
                    .child(crate::i18n::t("tasks.columns.status")),
            )
            .child(
                TableHead::new()
                    .w(gpui_kit::px(COL_SIZE_W))
                    .min_w(gpui_kit::px(COL_SIZE_W))
                    .flex_shrink_0()
                    .px_0()
                    .py_0()
                    .child(crate::i18n::t("tasks.columns.size")),
            )
            .child(
                TableHead::new()
                    .w(gpui_kit::px(COL_ACTIONS_W))
                    .min_w(gpui_kit::px(COL_ACTIONS_W))
                    .flex_shrink_0()
                    .px_0()
                    .py_0(),
            )
    }

    fn render_row(&self, task: &DownloadTask, cx: &mut Context<Self>) -> TableRow {
        let theme = cx.theme();
        let checked = self.selected.contains(&task.id);
        let percent = if task.total > 0 {
            (task.downloaded as f64 / task.total as f64 * 100.0) as f32
        } else {
            0.0
        };
        let (_, tag) = Self::status_tag(task.status);
        TableRow::new()
            .px_4()
            .py_2()
            .gap_3()
            .when(checked, |s| s.bg(theme.secondary))
            .child(
                TableCell::new()
                    .w(gpui_kit::px(COL_CHECK_W))
                    .min_w(gpui_kit::px(COL_CHECK_W))
                    .flex_shrink_0()
                    .px_0()
                    .py_0()
                    .child(
                        Checkbox::new(ElementId::from(format!("tasks-check-{}", task.id)))
                            .checked(checked)
                            .on_click({
                                let id = task.id.clone();
                                cx.listener(move |this, checked: &bool, _, cx| {
                                    let _ = checked;
                                    this.toggle(&id, cx);
                                })
                            }),
                    ),
            )
            .child(
                TableCell::new().flex_1().min_w_0().px_0().py_0().child(
                    v_flex()
                        .w_full()
                        .min_w_0()
                        .gap_0p5()
                        .child(
                            div()
                                .truncate()
                                .text_sm()
                                .child(SharedString::from(task.series_title.clone())),
                        )
                        .children((!task.error.is_empty()).then(|| {
                            div()
                                .truncate()
                                .text_xs()
                                .text_color(theme.danger)
                                .child(SharedString::from(task_error_text(&task.error)))
                        })),
                ),
            )
            .child(
                TableCell::new()
                    .w(gpui_kit::px(COL_EPISODE_W))
                    .min_w(gpui_kit::px(COL_EPISODE_W))
                    .flex_shrink_0()
                    .px_0()
                    .py_0()
                    .child(
                        div()
                            .w_full()
                            .text_sm()
                            .child(SharedString::from(task.vid_index.to_string())),
                    ),
            )
            .child(
                TableCell::new()
                    .w(gpui_kit::px(COL_STATUS_W))
                    .min_w(gpui_kit::px(COL_STATUS_W))
                    .flex_shrink_0()
                    .px_0()
                    .py_0()
                    .child(tag.child(SharedString::from(Self::status_tag(task.status).0))),
            )
            .child(
                TableCell::new()
                    .w(gpui_kit::px(COL_SIZE_W))
                    .min_w(gpui_kit::px(COL_SIZE_W))
                    .flex_shrink_0()
                    .px_0()
                    .py_0()
                    .child(if task.status == TaskStatus::Running {
                        v_flex()
                            .w_full()
                            .gap_1()
                            .child(Progress::new("task-progress").value(percent))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(
                                SharedString::from(format!(
                                    "{} / {}",
                                    format_bytes(task.downloaded),
                                    format_bytes(task.total)
                                )),
                            ))
                    } else {
                        div()
                            .w_full()
                            .text_xs()
                            .child(SharedString::from(format_bytes(task.total)))
                    }),
            )
            .child(
                TableCell::new()
                    .w(gpui_kit::px(COL_ACTIONS_W))
                    .min_w(gpui_kit::px(COL_ACTIONS_W))
                    .flex_shrink_0()
                    .px_0()
                    .py_0()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .justify_end()
                            .gap_1()
                            .children((task.status == TaskStatus::Running).then(|| {
                                let id = task.id.clone();
                                Button::new(ElementId::from(format!("tasks-stop-{}", task.id)))
                                    .icon(gpui_kit::assets::IconName::Pause)
                                    .ghost()
                                    .small()
                                    .tooltip(crate::i18n::t("tasks.actions.stop"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let _ =
                                            hongguo_service::download_service::actions::stop_task(
                                                &this.state,
                                                &id,
                                            );
                                        this.refresh(cx);
                                    }))
                            }))
                            .children(
                                matches!(task.status, TaskStatus::Failed | TaskStatus::Stopped)
                                    .then(|| {
                                        let id = task.id.clone();
                                        Button::new(ElementId::from(format!(
                                            "tasks-retry-{}",
                                            task.id
                                        )))
                                        .icon(gpui_kit::assets::IconName::RotateCcw)
                                        .ghost()
                                        .small()
                                        .tooltip(crate::i18n::t("tasks.actions.retry"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let _ = hongguo_service::download_service::actions::retry_task(
                                                &this.state,
                                                &id,
                                            );
                                            this.refresh(cx);
                                        }))
                                    }),
                            )
                            .children((task.status == TaskStatus::Completed).then(|| {
                                let series_id = task.series_id.clone();
                                Button::new(ElementId::from(format!("tasks-folder-{}", task.id)))
                                    .icon(gpui_kit::assets::IconName::FolderOpen)
                                    .ghost()
                                    .small()
                                    .tooltip(crate::i18n::t("tasks.actions.openFolder"))
                                    .on_click(cx.listener(move |this, _, _, _| {
                                        this.open_folder(&series_id);
                                    }))
                            }))
                            .child({
                                let id = task.id.clone();
                                Button::new(ElementId::from(format!("tasks-del-{}", task.id)))
                                    .icon(gpui_kit::assets::IconName::Trash)
                                    .ghost()
                                    .small()
                                    .tooltip(crate::i18n::t("tasks.actions.remove"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.confirm_delete(vec![id.clone()], window, cx);
                                    }))
                            }),
                    ),
            )
    }
}

impl gpui_kit::Render for TasksView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if self.tasks.is_none() {
            // 首拉加载：行骨架铺在表格容器里（与真实表格同 padding，
            // 数据到位不跳版）。行高按 GPUI 真实行算，不照抄 Tauri 的 h-24：
            // 行内上下 p_2 16 + 行内容高 31（运行中的大小列：进度条 8 +
            // 间距 4 + 文字行 19；状态标签 25 / 操作按钮 24 都比它矮）= 47。
            // 表格行之间没有行距（靠 1px 边框分隔），骨架按同值给 1。
            div()
                .id("tasks-skeleton")
                .flex_1()
                .overflow_y_scroll()
                .px_6()
                .child(crate::pages::skeleton::rows(cx, 6, 47.0, 1.0))
                .into_any_element()
        } else if self.tasks.as_deref().unwrap_or(&[]).is_empty() {
            crate::pages::states::empty_state(cx, None, crate::i18n::t("tasks.empty"), None)
        } else {
            let theme = cx.theme();
            let tasks = self.tasks.clone().unwrap_or_default();
            div()
                .id("tasks-table")
                .flex_1()
                .overflow_y_scroll()
                .px_6()
                .child(
                    v_flex()
                        .w_full()
                        .rounded(crate::tokens::radius::CARD)
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.group_box)
                        .overflow_hidden()
                        .child(
                            Table::new()
                                .accessibility_label(crate::i18n::t("nav.tasks.title"))
                                .small()
                                .bg(theme.group_box)
                                .child(
                                    TableHeader::new()
                                        .bg(theme.group_box)
                                        .child(self.render_header_row(cx)),
                                )
                                .child(
                                    TableBody::new()
                                        .children(tasks.iter().map(|t| self.render_row(t, cx))),
                                ),
                        ),
                )
                .into_any_element()
        };
        let selected_count = self.selected.len();
        let freeable = self.freeable_of(&self.selected.iter().cloned().collect::<Vec<_>>());
        let bottom_bar = (selected_count > 0).then(|| {
            let theme = cx.theme();
            let ids: Vec<String> = self.selected.iter().cloned().collect();
            h_flex()
                .w_full()
                .flex_shrink_0()
                .items_center()
                .gap_3()
                .border_t_1()
                .border_color(theme.border)
                .bg(theme.background)
                .px_4()
                .py_3()
                .child(
                    div()
                        .text_sm()
                        .child(SharedString::from(selected_count.to_string())),
                )
                .children((freeable > 0).then(|| {
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(SharedString::from(crate::i18n::tf(
                            "tasks.freeable",
                            &[("size", format_bytes(freeable).as_str())],
                        )))
                }))
                .child(div().flex_1())
                .child(
                    Button::new("tasks-clear-selection")
                        .icon(gpui_kit::assets::IconName::X)
                        .ghost()
                        .small()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.selected.clear();
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("tasks-delete-selected")
                        .icon(gpui_kit::assets::IconName::Trash)
                        .label(crate::i18n::t("common.delete"))
                        .danger()
                        .small()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.confirm_delete(ids.clone(), window, cx);
                        })),
                )
                .into_any_element()
        });
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .id("tasks-page")
            .children((self.tasks.is_some()).then(|| {
                // 首拉加载期不渲染筛选工具栏：加载态整页居中、不受页内控件
                // 影响（用户定稿口径）
                div()
                    .w_full()
                    .px_6()
                    .pt_6()
                    .pb_4()
                    .child(self.render_toolbar(cx))
            }))
            .child(body)
            .children(bottom_bar)
    }
}

/// task.error 存 i18n key（`error.*`），也混有历史错误原文（存量无法静态
/// 枚举）。已知 key 翻成中文，未知原样透出（Tauri t() 回落同口径）。
fn task_error_text(key: &str) -> String {
    if key.starts_with("error.") {
        crate::i18n::t(key)
    } else {
        key.to_string()
    }
}

impl TasksView {
    /// 预览种子（screenshot feature）：灌假任务与队列状态，跳过真实队列。
    /// `tasks-empty` = 空态；`tasks-manage` = 有选中（吸底条可见）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(&mut self, scene: &str, cx: &mut Context<Self>) {
        let tasks = if scene == "tasks-empty" {
            Vec::new()
        } else {
            crate::preview::fixtures::download_tasks()
        };
        let count = |status: TaskStatus| tasks.iter().filter(|t| t.status == status).count();
        self.queue_status = Some(QueueStatus {
            pending: count(TaskStatus::Pending),
            running: count(TaskStatus::Running),
            completed: count(TaskStatus::Completed),
            failed: count(TaskStatus::Failed),
            limit: 3,
            active: count(TaskStatus::Running),
        });
        if scene == "tasks-manage" {
            self.selected = tasks.iter().take(2).map(|t| t.id.clone()).collect();
        }
        self.tasks = Some(tasks);
        cx.notify();
    }
}
