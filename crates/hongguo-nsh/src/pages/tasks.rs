//! 下载任务页（Tauri `/tasks` / GPUI `pages/tasks` 的 NSH 对齐）。
//!
//! 任务与队列状态都是本地内存快照（`state.queue()`），直读即刷；下载事件
//! 经 pump 的 `download_tick` 触发重读（GPUI 的事件订阅同口径）。顶部队列
//! 状态行 + 一键启动/暂停/重试全部失败；行 = 标题（剧名 第N集）+ 状态 +
//! 进度，行内动作按状态分档（GPUI 同款）：下载中可停止、失败/停止可重试、
//! 完成可打开文件夹，任何状态可删记录（仅移除任务，磁盘文件保留；
//! 带文件删除走 P3 批量）。

use nsh_core::ui::prelude::*;

use hongguo_common::model::{DownloadTask, QueueStatus, TaskStatus};

use crate::i18n::t;
use crate::pages::common::{PAGE_PAD, empty_body, pill};

#[component]
pub fn TasksPage() -> impl IntoElement {
    let tasks: Signal<Vec<DownloadTask>> = signal_with(Vec::new(), SignalEquals::Never);
    let queue = signal(QueueStatus::default());
    let events = crate::pump::events();

    // 挂载直读一次；任一业务事件重读内存快照（快照读取本身廉价，
    // 事件粒度对齐 pump 的 SignalEquals::Never）。
    {
        effect(move || {
            events.get();
            let (tasks, queue) = (tasks, queue);
            refresh(tasks, queue);
        });
    }

    view()
        .flex_1()
        .w_full()
        .h_full()
        .overflow_hidden()
        .flex_col()
        .px(px(PAGE_PAD))
        .pt(px(PAGE_PAD))
        .gap(px(12.0))
        // 队列状态 + 全局动作。
        .child(
            view()
                .w_full()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    text(move || {
                        let q = queue.get();
                        t("tasks.queueStatus")
                            .replace("{running}", &q.running.to_string())
                            .replace("{pending}", &q.pending.to_string())
                            .replace("{completed}", &q.completed.to_string())
                            .replace("{failed}", &q.failed.to_string())
                    })
                    .text_size(px(12.0))
                    .text_color(theme().muted_foreground),
                )
                .child(view().flex_1())
                .child(pill(
                    "tasks-resume-all",
                    move || t("tasks.startAll"),
                    move || theme().muted,
                    move || theme().foreground,
                    || {
                        let state = crate::state::state();
                        if let Err(e) =
                            hongguo_service::download_service::actions::resume_all(&state)
                        {
                            log::warn!("[Tasks] 一键启动失败: {e}");
                        }
                    },
                ))
                .child(pill(
                    "tasks-pause-all",
                    move || t("tasks.pauseAll"),
                    move || theme().muted,
                    move || theme().foreground,
                    || {
                        let state = crate::state::state();
                        if let Err(e) =
                            hongguo_service::download_service::actions::pause_all(&state)
                        {
                            log::warn!("[Tasks] 一键暂停失败: {e}");
                        }
                    },
                ))
                .child(If::new(
                    move || queue.get().failed > 0,
                    move || {
                        pill(
                            "tasks-retry-all",
                            move || t("tasks.retryAll"),
                            move || theme().muted,
                            move || theme().foreground,
                            || {
                                let state = crate::state::state();
                                let ids: Vec<String> = state
                                    .queue()
                                    .all()
                                    .iter()
                                    .filter(|task| task.status == TaskStatus::Failed)
                                    .map(|task| task.id.clone())
                                    .collect();
                                if ids.is_empty() {
                                    return;
                                }
                                if let Err(e) =
                                    hongguo_service::download_service::actions::retry_tasks(
                                        &state, &ids,
                                    )
                                {
                                    log::warn!("[Tasks] 重试全部失败: {e}");
                                }
                            },
                        )
                    },
                )),
        )
        // 主体：空态 / 列表。
        .child(If::new(
            move || tasks.get().is_empty(),
            move || empty_body("tasks.empty", None),
        ))
        .child(If::new(
            move || !tasks.get().is_empty(),
            move || {
                scroll()
                    .flex_1()
                    .w_full()
                    .flex_col()
                    .gap(px(8.0))
                    .child(Each::new(
                        move || tasks.get(),
                        |task: &DownloadTask| task.id.clone(),
                        move |task: ReadSignal<DownloadTask>, _| row(task),
                    ))
                    .into_element()
            },
        ))
}

/// 一行任务：标题 + 状态/进度 + 行内动作（按状态分档，GPUI 同款）。
fn row(task: ReadSignal<DownloadTask>) -> impl IntoElement {
    let id = task.get().id.clone();
    let key = format!("task-{id}");
    let stop_id = id.clone();
    let retry_id = id.clone();
    let delete_id = id.clone();
    view()
        .w_full()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .p(px(10.0))
        .rounded(px(10.0))
        .bg(theme().group_box)
        .child(
            view()
                .flex_1()
                .overflow_hidden()
                .flex_col()
                .gap(px(4.0))
                .child(
                    text(move || {
                        let task = task.get();
                        let episode = format!("第{}集", task.vid_index);
                        if task.ep_title.is_empty() {
                            format!("{} {episode}", task.series_title)
                        } else {
                            format!("{} {episode} · {}", task.series_title, task.ep_title)
                        }
                    })
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme().foreground),
                )
                .child(
                    text(move || progress_line(&task.get()))
                        .text_size(px(12.0))
                        .text_color(theme().muted_foreground),
                ),
        )
        // 下载中：停止。
        .child(If::new(
            move || task.get().status == TaskStatus::Running,
            move || {
                let stop_id = stop_id.clone();
                action_pill(
                    "task-stop",
                    move || t("tasks.actions.stop"),
                    move || {
                        let state = crate::state::state();
                        if let Err(e) =
                            hongguo_service::download_service::actions::stop_task(&state, &stop_id)
                        {
                            log::warn!("[Tasks] 停止任务失败: {e}");
                        }
                    },
                )
            },
        ))
        // 失败/已停止：重试。
        .child(If::new(
            move || matches!(task.get().status, TaskStatus::Failed | TaskStatus::Stopped),
            move || {
                let retry_id = retry_id.clone();
                action_pill(
                    "task-retry",
                    move || t("tasks.actions.retry"),
                    move || {
                        let state = crate::state::state();
                        if let Err(e) = hongguo_service::download_service::actions::retry_task(
                            &state, &retry_id,
                        ) {
                            log::warn!("[Tasks] 重试任务失败: {e}");
                        }
                    },
                )
            },
        ))
        // 已完成：打开文件夹（定位到成品文件）。
        .child(If::new(
            move || task.get().status == TaskStatus::Completed && !task.get().file_path.is_empty(),
            move || {
                let path = task.get().file_path;
                action_pill(
                    "task-folder",
                    move || t("tasks.actions.openFolder"),
                    move || reveal_in_folder(&path),
                )
            },
        ))
        // 删除记录（仅移除任务；磁盘文件保留，带文件删除走 P3 批量）。
        .child(If::new(
            move || true,
            move || {
                let delete_id = delete_id.clone();
                action_pill(
                    "task-delete",
                    move || t("tasks.actions.remove"),
                    move || {
                        let ids = vec![delete_id.clone()];
                        if let Err(e) = hongguo_service::download_service::actions::delete_tasks(
                            &crate::state::state(),
                            &ids,
                            false,
                        ) {
                            log::warn!("[Tasks] 删除任务失败: {e}");
                        }
                    },
                )
            },
        ))
        .key(key)
}

/// 状态/进度行：状态文案 + 字节进度（完成给体积，失败给原因）。
fn progress_line(task: &DownloadTask) -> String {
    let status = match task.status {
        TaskStatus::Pending => t("tasks.status.pending"),
        TaskStatus::Running => t("tasks.status.running"),
        TaskStatus::Completed => t("tasks.status.completed"),
        TaskStatus::Failed => t("tasks.status.failed"),
        TaskStatus::Stopped => t("tasks.status.stopped"),
    };
    let detail = match task.status {
        TaskStatus::Running if task.total > 0 => {
            format!(
                "{}/{}",
                format_bytes(task.downloaded),
                format_bytes(task.total)
            )
        }
        TaskStatus::Completed => format_bytes(task.downloaded),
        TaskStatus::Failed if !task.error.is_empty() => task.error.clone(),
        _ => String::new(),
    };
    if detail.is_empty() {
        status
    } else {
        format!("{status} · {detail}")
    }
}

/// 一个小动作钮（muted 底、紧凑；key 与动作收在内部，语义同 pill）。
fn action_pill(
    key: &'static str,
    label: impl Fn() -> String + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    view()
        .h(px(26.0))
        .flex_row()
        .items_center()
        .px(px(10.0))
        .rounded(px(999.0))
        .bg(theme().muted)
        .key(key)
        .on_press(move |_: &()| on_press())
        .child(
            text(label)
                .text_size(px(12.0))
                .text_color(theme().foreground),
        )
}

/// 在文件管理器中定位到文件（Tauri opener `reveal_item_in_dir` 同款；
/// GPUI `pages::reveal_in_folder` 的 NSH 份，当前只有任务页在用）。
fn reveal_in_folder(path: &str) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let result = std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{path}\""))
            .spawn();
        if let Err(e) = result {
            log::warn!("[Tasks] 打开文件夹失败: {e}");
        }
    }
    #[cfg(target_os = "macos")]
    {
        let result = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
        if let Err(e) = result {
            log::warn!("[Tasks] 打开文件夹失败: {e}");
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = path;
        log::warn!("[Tasks] 打开文件夹在该平台未实现");
    }
}

/// 字节短格式（GPUI `format_bytes` 同口径：1024 阶梯、一位小数）。
fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_owned();
    }
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut i = 0;
    while value >= 1024.0 && i < units.len() - 1 {
        value /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", units[i])
    }
}

/// 重读内存队列快照（同步、快；事件 tick 与挂载各调一次）。
fn refresh(tasks: Signal<Vec<DownloadTask>>, queue: Signal<QueueStatus>) {
    let state = crate::state::state();
    let snapshot = state.queue();
    tasks.set(snapshot.all());
    queue.set(snapshot.status());
}
