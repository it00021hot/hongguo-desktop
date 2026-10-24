//! 一键合并页（Tauri `/merge` = `merge-page.tsx` 的 GPUI 对齐）。
//!
//! 布局：居中窄列两张卡——「一键合并」表单（选剧 + 输出名 + 快速/兼容
//! 模式 + 合并前校验 + 开始按钮）与「合并任务」列表（状态 / 体积 / 进度 /
//! 打开文件夹 / 删除记录）。数据与操作走 `hongguo_service::merge_service`
//! 与 `store.merge_tasks`，与 Tauri 的 `get_merge_*` / `merge_*` command
//! 同一段服务面；进度与成败经 `ServiceEvent::Merge*` 事件流。

use std::sync::Arc;

use gpui_kit::component::Disableable;
use gpui_kit::component::button::{ButtonVariant, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::label::Label;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, Entity, SharedString, Window, div};
use hongguo_common::model::{MergeCandidate, MergeMode, MergePreflight, MergeStatus, MergeTask};
use hongguo_service::app_state::AppStateInner;
use hongguo_service::events::ServiceEvent;

use crate::bridge;
use crate::home::fmt::format_bytes;
use crate::pages::card;
use crate::ui_button::button as Button;

/// 下拉候选项：行文案「剧名 · N 集」，值 = series_id。
#[derive(Clone)]
struct CandidateItem {
    series_id: SharedString,
    label: SharedString,
}

impl SearchableListItem for CandidateItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &SharedString {
        &self.series_id
    }
}

impl From<&MergeCandidate> for CandidateItem {
    fn from(c: &MergeCandidate) -> Self {
        Self {
            series_id: c.series_id.clone().into(),
            label: crate::i18n::tf(
                "merge.candidate",
                &[
                    ("title", c.series_title.as_str()),
                    ("count", &c.episode_count.to_string()),
                ],
            )
            .into(),
        }
    }
}

pub(crate) struct MergeView {
    state: Arc<AppStateInner>,
    /// 可合并的剧（有已下载分集的那些）。None = 还没拉过。
    candidates: Option<Vec<MergeCandidate>>,
    /// 合并任务（None = 还没拉过；空列表不渲染任务卡）。
    tasks: Option<Vec<MergeTask>>,
    select: Entity<SelectState<SearchableVec<CandidateItem>>>,
    output_input: Entity<InputState>,
    /// 用户手动改过的输出名；空串 = 跟随所选剧名（Tauri `outputName || title`）。
    user_output: String,
    selected: Option<MergeCandidate>,
    /// 合并前校验结果（选剧后异步拉；换剧时旧结果作废）。
    preflight: Option<MergePreflight>,
    preflight_pending: bool,
    /// 模式偏好；快速合并被禁时本次实际用兼容（Tauri effectiveMode 同款派生）。
    mode: MergeMode,
    /// 阻塞池任务失败的短句（Some = 整页换错误态 + 重试）。
    failed: Option<String>,
}

impl MergeView {
    pub(crate) fn new(
        state: Arc<AppStateInner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let select = cx.new(|cx| {
            SelectState::new(SearchableVec::new(Vec::new()), None, window, cx).searchable(true)
        });
        let output_input = cx.new(|cx| InputState::new(window, cx));
        // 选中一部剧 → 记下候选 + 预检；清空输入框 → 回落到剧名（Tauri
        // controlled input 的同款回弹）。用 subscribe_in：回填 InputState
        // 要窗口（set_value 的入参），event 回调里只有它带窗口。
        cx.subscribe_in(
            &select,
            window,
            |this, _, event: &SelectEvent<SearchableVec<CandidateItem>>, window, cx| {
                if let SelectEvent::Confirm(Some(series_id)) = event {
                    this.pick_series(series_id.to_string(), window, cx);
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &output_input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = this.output_input.read(cx).value().to_string();
                    if value.trim().is_empty() {
                        this.user_output.clear();
                        this.snap_output_to_title(window, cx);
                    } else {
                        this.user_output = value;
                    }
                    cx.notify();
                }
            },
        )
        .detach();
        let mut view = Self {
            state,
            candidates: None,
            tasks: None,
            select,
            output_input,
            user_output: String::new(),
            selected: None,
            preflight: None,
            preflight_pending: false,
            mode: MergeMode::Quick,
            failed: None,
        };
        view.subscribe_merge_events(cx);
        view
    }

    /// 进入页面时拉候选与任务列表（本地数据，走阻塞池）。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        // 重试时先清旧错误，否则失败态会一直挂在页面上。
        self.failed = None;
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            tokio::task::spawn_blocking(move || {
                (
                    hongguo_service::merge_service::candidates(&state),
                    state.store.merge_tasks(),
                )
            })
            .await
            .map_err(|e| format!("阻塞池任务失败: {e}"))
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                match result {
                    Ok((candidates, tasks)) => {
                        view.failed = None;
                        view.candidates = Some(candidates);
                        // 候选灌进下拉（SelectState::set_items 需要窗口）
                        let items: Vec<CandidateItem> = view
                            .candidates
                            .as_deref()
                            .unwrap_or(&[])
                            .iter()
                            .map(CandidateItem::from)
                            .collect();
                        view.select.update(cx, |select, cx| {
                            select.set_items(SearchableVec::new(items), window, cx)
                        });
                        match tasks {
                            Ok(t) => view.tasks = Some(t),
                            Err(e) => log::warn!("[Merge] 任务列表读取失败: {e}"),
                        }
                    }
                    Err(msg) => {
                        view.failed = Some(msg);
                        view.candidates = None;
                        view.tasks = None;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 订阅合并事件：任务进度/成败推到列表（事件载荷即最新快照）。
    fn subscribe_merge_events(&mut self, cx: &mut Context<Self>) {
        let mut rx = crate::events::subscribe(&self.state);
        cx.spawn(async move |this, cx| {
            while let Some(event) = rx.recv().await {
                // 只吃合并四类；事件载荷即任务最新快照
                let task = match event {
                    ServiceEvent::MergeTaskAdded(task)
                    | ServiceEvent::MergeProgress(task)
                    | ServiceEvent::MergeCompleted(task)
                    | ServiceEvent::MergeFailed(task) => task,
                    _ => continue,
                };
                let ok = this
                    .update(cx, |view, cx| {
                        view.upsert_task(task);
                        cx.notify();
                    })
                    .is_ok();
                if !ok {
                    break; // 视图已销毁
                }
            }
        })
        .detach();
    }

    fn upsert_task(&mut self, task: MergeTask) {
        let list = self.tasks.get_or_insert_with(Vec::new);
        match list.iter_mut().find(|t| t.id == task.id) {
            Some(slot) => *slot = task,
            None => list.push(task),
        }
    }

    fn pick_series(&mut self, series_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let picked = self
            .candidates
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .find(|c| c.series_id == series_id)
            .cloned();
        let Some(picked) = picked else {
            return;
        };
        // 输出名：用户改过就保留，没改过跟所选剧名（Tauri `outputName || title`）
        let output = if self.user_output.is_empty() {
            picked.series_title.clone()
        } else {
            self.user_output.clone()
        };
        self.set_output_text(output, window, cx);
        self.selected = Some(picked);
        self.preflight = None;
        self.run_preflight(cx);
        cx.notify();
    }

    /// 输入框清空后回弹到剧名。
    fn snap_output_to_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(series) = &self.selected {
            let title = series.series_title.clone();
            self.set_output_text(title, window, cx);
        }
    }

    /// 回填输出名输入框（set_value 不触发 Change 事件，无回环）。
    fn set_output_text(&self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.output_input
            .update(cx, |state, cx| state.set_value(text, window, cx));
    }

    /// 合并前校验（读每集头部探测编码，走阻塞池；换剧后旧结果按 id 作废）。
    fn run_preflight(&mut self, cx: &mut Context<Self>) {
        let Some(series) = self.selected.clone() else {
            return;
        };
        self.preflight_pending = true;
        let state = Arc::clone(&self.state);
        let series_id = series.series_id.clone();
        let series_id_check = series_id.clone();
        let rx = bridge::spawn(async move {
            tokio::task::spawn_blocking(move || {
                hongguo_service::merge_service::prepare::preflight(&state, &series_id)
            })
            .await
            .map_err(|e| format!("阻塞池任务失败: {e}"))
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            this.update_in(cx, |view, window, cx| {
                // 期间换过剧：丢弃（结果归属旧选择）
                if view.selected.as_ref().map(|s| s.series_id.as_str())
                    != Some(series_id_check.as_str())
                {
                    return;
                }
                view.preflight_pending = false;
                // 阻塞池 join 失败没有 AppError 可翻译，直接透出短句。
                let outcome: Result<MergePreflight, String> = match result {
                    Ok(Ok(preflight)) => Ok(preflight),
                    Ok(Err(e)) => {
                        log::warn!("[Merge] 预检失败: {e}");
                        Err(crate::i18n::app_error_text(&e))
                    }
                    Err(msg) => {
                        log::warn!("[Merge] 预检失败: {msg}");
                        Err(msg)
                    }
                };
                match outcome {
                    Ok(preflight) => view.preflight = Some(preflight),
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
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn quick_unavailable(&self) -> bool {
        self.preflight
            .as_ref()
            .map(|p| p.episode_count > 0 && !p.codec_consistent)
            .unwrap_or(false)
    }

    fn effective_mode(&self) -> MergeMode {
        if self.quick_unavailable() {
            MergeMode::Compat
        } else {
            self.mode
        }
    }

    /// 实际输出名（Tauri `outputName || 剧名`）。
    fn effective_output(&self) -> String {
        if !self.user_output.is_empty() {
            return self.user_output.clone();
        }
        self.selected
            .as_ref()
            .map(|s| s.series_title.clone())
            .unwrap_or_default()
    }

    fn busy(&self) -> bool {
        let output = self.effective_output();
        self.tasks
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .any(|t| t.status == MergeStatus::Running && t.output_name == output)
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(series), output) = (self.selected.clone(), self.effective_output()) else {
            return;
        };
        if series.series_id.is_empty() || output.is_empty() {
            return;
        }
        match hongguo_service::merge_service::start::merge_series(
            &self.state,
            &series.series_id,
            &output,
            self.effective_mode(),
        ) {
            Ok(task) => {
                // 后端已派发到阻塞池：这里只确认「已启动」，进度走事件流
                window
                    .push_notification(Notification::success(crate::i18n::t("merge.started")), cx);
                self.upsert_task(task);
                cx.notify();
            }
            Err(e) => {
                log::warn!("[Merge] 启动失败: {e}");
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
    }

    fn remove_task(&mut self, task: &MergeTask, window: &mut Window, cx: &mut Context<Self>) {
        // running 记录先协作式取消后台线程再删（Tauri delete_merge_task 同款：
        // 只删记录的话线程照样转码，跑完还会把记录写回来）
        if task.status == MergeStatus::Running {
            hongguo_service::merge_service::guard::cancel(&task.output_name);
        }
        match hongguo_service::merge_service::remove_task(&self.state, &task.id) {
            Ok(()) => {
                if let Some(tasks) = self.tasks.as_mut() {
                    tasks.retain(|t| t.id != task.id);
                }
                cx.notify();
            }
            Err(e) => {
                log::warn!("[Merge] 删除记录失败: {e}");
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
    }

    fn confirm_remove(&self, task: MergeTask, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            let view = view.clone();
            let task = task.clone();
            dialog
                .title(crate::i18n::t("merge.removeConfirm"))
                .description(crate::i18n::t("merge.removeDesc"))
                .ok_text(crate::i18n::t("common.delete"))
                .cancel_text(crate::i18n::t("common.cancel"))
                .show_cancel(true)
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |view, cx| view.remove_task(&task, window, cx));
                    true
                })
        });
    }

    // —— 渲染 ——

    fn render_form_card(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let no_candidates = self
            .candidates
            .as_ref()
            .map(|c| c.is_empty())
            .unwrap_or(false);
        let quick_unavailable = self.quick_unavailable();
        let effective = self.effective_mode();
        let output = self.effective_output();
        let busy = self.busy();
        let can_start = !busy
            && self.selected.is_some()
            && !output.is_empty()
            && self
                .preflight
                .as_ref()
                .map(|p| p.episode_count > 0)
                .unwrap_or(false);

        card(cx)
            .child(
                div()
                    .text_base()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(crate::i18n::t("merge.title")),
            )
            // 选剧
            .child(
                v_flex()
                    .gap_2()
                    .child(Label::new(crate::i18n::t("merge.selectSeries")))
                    .child(
                        div()
                            .w_full()
                            .when(no_candidates, |s| s.opacity(0.5))
                            .child(
                                Select::new(&self.select)
                                    .placeholder(crate::i18n::t("merge.pickSeries"))
                                    .w_full(),
                            ),
                    )
                    .children(no_candidates.then(|| {
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("merge.noCandidates"))
                    })),
            )
            // 输出名
            .child(
                v_flex()
                    .gap_2()
                    .child(Label::new(crate::i18n::t("merge.outputName")))
                    .child(Input::new(&self.output_input)),
            )
            // 模式
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        TabBar::new("merge-mode")
                            .segmented()
                            .w_full()
                            .selected_index(if effective == MergeMode::Quick { 0 } else { 1 })
                            .on_click(cx.listener(|this, index: &usize, _, cx| {
                                this.mode = if *index == 0 {
                                    MergeMode::Quick
                                } else {
                                    MergeMode::Compat
                                };
                                cx.notify();
                            }))
                            .child(
                                Tab::new()
                                    .flex_1()
                                    // 组件带 .icon() 时按设计只画图标；要让
                                    // 「图标 + 文字」同排（Tauri tab 同款）
                                    // 得走 children。
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                gpui_kit::component::Icon::new(
                                                    gpui_kit::assets::IconName::Zap,
                                                )
                                                .small(),
                                            )
                                            .child(crate::i18n::t("merge.quick")),
                                    )
                                    .disabled(quick_unavailable),
                            )
                            .child(
                                Tab::new().flex_1().child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            gpui_kit::component::Icon::new(
                                                gpui_kit::assets::IconName::Gauge,
                                            )
                                            .small(),
                                        )
                                        .child(crate::i18n::t("merge.compat")),
                                ),
                            ),
                    )
                    .child(div().text_sm().text_color(theme.muted_foreground).child(
                        if effective == MergeMode::Quick {
                            if quick_unavailable {
                                crate::i18n::t("merge.quickUnavailable")
                            } else {
                                crate::i18n::t("merge.quickDesc")
                            }
                        } else {
                            crate::i18n::t("merge.compatDesc")
                        },
                    )),
            )
            // 预检结果
            .children(self.preflight.as_ref().map(|p| {
                v_flex()
                    .w_full()
                    .gap_1()
                    .rounded(gpui_kit::px(8.0))
                    .border_1()
                    .border_color(if p.ok { theme.border } else { theme.warning })
                    .px_4()
                    .py_3()
                    .children((p.episode_count > 0).then(|| {
                        div()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .child(format!(
                                "{} · {}{}",
                                crate::i18n::tf(
                                    "common.episodeCount",
                                    &[("count", &p.episode_count.to_string())],
                                ),
                                format_bytes(p.estimated_size),
                                p.free_space
                                    .map(|f| {
                                        format!(
                                            " · {}",
                                            crate::i18n::tf(
                                                "merge.freeSpace",
                                                &[("size", format_bytes(f).as_str())],
                                            )
                                        )
                                    })
                                    .unwrap_or_default()
                            ))
                    }))
                    .children((!p.warnings.is_empty()).then(|| {
                        div().text_sm().text_color(theme.muted_foreground).child(
                            p.warnings
                                .iter()
                                .map(|key| merge_warning_text(key, p.codec_mismatch_episode))
                                .collect::<Vec<_>>()
                                .join("；"),
                        )
                    }))
            }))
            .child(
                Button::new("merge-start")
                    .icon(gpui_kit::assets::IconName::Combine)
                    .label(if busy {
                        crate::i18n::t("merge.running")
                    } else {
                        crate::i18n::t("merge.start")
                    })
                    .primary()
                    .disabled(!can_start)
                    .when(!can_start, |b| b.cursor_default())
                    .on_click(cx.listener(|this, _, window, cx| this.start(window, cx))),
            )
            .into_any_element()
    }

    fn render_task_row(&self, task: &MergeTask, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let (status_label, tag) = match task.status {
            MergeStatus::Pending => (crate::i18n::t("merge.status.pending"), Tag::secondary()),
            MergeStatus::Running => (crate::i18n::t("merge.status.running"), Tag::secondary()),
            MergeStatus::Completed => (crate::i18n::t("merge.status.completed"), Tag::success()),
            MergeStatus::Failed => (crate::i18n::t("merge.status.failed"), Tag::danger()),
            MergeStatus::Cancelled => (crate::i18n::t("merge.status.cancelled"), Tag::secondary()),
        };
        h_flex()
            .items_center()
            .gap_2()
            .rounded(gpui_kit::px(6.0))
            .border_1()
            .border_color(theme.border)
            .px_3()
            .py_2()
            .text_sm()
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .child(SharedString::from(task.output_name.clone())),
            )
            .child(tag.child(SharedString::from(status_label)))
            .children((task.output_size > 0).then(|| {
                div()
                    .text_xs()
                    .font_family(theme.mono_font_family.clone())
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(format_bytes(task.output_size)))
            }))
            .children((task.status == MergeStatus::Running).then(|| {
                h_flex()
                    .w(gpui_kit::px(112.0))
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .child(
                        Progress::new(ElementId::from(format!("merge-progress-{}", task.id)))
                            .value(task.percent as f32),
                    )
                    .child(
                        div()
                            .w_9()
                            .text_right()
                            .text_xs()
                            .font_family(theme.mono_font_family.clone())
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(format!("{:.0}%", task.percent))),
                    )
            }))
            // 完成后一键定位到产物文件（Tauri 同款 FolderOpen 入口）
            .children(
                (task.status == MergeStatus::Completed && !task.output_path.is_empty()).then(
                    || {
                        let path = task.output_path.clone();
                        Button::new(ElementId::from(format!("merge-open-{}", task.id)))
                            .icon(gpui_kit::assets::IconName::FolderOpen)
                            .ghost()
                            .small()
                            .tooltip(crate::i18n::t("merge.actions.openOutput"))
                            .on_click(move |_, _, _| crate::pages::reveal_in_folder(&path))
                    },
                ),
            )
            .child(
                Button::new(ElementId::from(format!("merge-del-{}", task.id)))
                    .icon(gpui_kit::assets::IconName::Trash)
                    .ghost()
                    .small()
                    .tooltip(crate::i18n::t("merge.actions.remove"))
                    .on_click({
                        let task = task.clone();
                        cx.listener(move |this, _, window, cx| {
                            this.confirm_remove(task.clone(), window, cx);
                        })
                    }),
            )
            .into_any_element()
    }

    fn render_tasks_card(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        // 还没拉过（None）= 首拉加载：出行骨架占位，卡位与标题不变；
        // 拉过但是空 = 没有任务，不渲染任务卡。
        let loading = self.tasks.is_none();
        let tasks = self.tasks.as_deref().unwrap_or(&[]);
        if tasks.is_empty() && !loading {
            return None;
        }
        Some(
            card(cx)
                .child(
                    div()
                        .text_base()
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .child(crate::i18n::t("merge.taskList")),
                )
                .child(if loading {
                    // 行高按 GPUI 真实行算，不照抄 Tauri 的 h-24：行内容高 25
                    // （状态标签最高：文字 15 + 内边距 8 + 边框 2；标题行 23 /
                    // 删除按钮 24 都比它矮）+ 行内上下 p_2 16 + 边框 2 = 43；
                    // 行间距另按真实列表的 gap_2 给 8。
                    crate::pages::skeleton::rows(cx, 5, 43.0, 8.0)
                } else {
                    v_flex()
                        .w_full()
                        .gap_2()
                        .children(tasks.iter().map(|t| self.render_task_row(t, cx)))
                        .into_any_element()
                })
                .into_any_element(),
        )
    }

    /// 页面主体：加载失败时整页换错误态（候选与任务列表同源，一起失败）。
    fn render_body(&self, cx: &mut Context<Self>) -> Vec<gpui_kit::AnyElement> {
        if let Some(msg) = self.failed.clone() {
            return vec![crate::pages::states::error_state(
                cx,
                None,
                crate::i18n::t("common.loadFailed"),
                Some(msg),
                Some((
                    "merge-retry",
                    crate::pages::states::retry_on(cx.entity(), |this, cx| this.load(cx)),
                )),
            )];
        }
        let mut body = vec![self.render_form_card(cx)];
        if let Some(tasks) = self.render_tasks_card(cx) {
            body.push(tasks);
        }
        body
    }

    /// 预览种子（screenshot feature）：灌假候选/假任务/假预检，跳过本地
    /// 读取。`merge-empty` = 无候选且无任务（空下拉 + 无任务卡）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(
        &mut self,
        scene: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let empty = scene == "merge-empty";
        self.tasks = Some(if empty {
            Vec::new()
        } else {
            crate::preview::fixtures::merge_tasks()
        });
        self.candidates = Some(if empty {
            Vec::new()
        } else {
            crate::preview::fixtures::merge_candidates()
        });
        if !empty {
            let candidates = self.candidates.clone().unwrap_or_default();
            let items: Vec<CandidateItem> = candidates.iter().map(CandidateItem::from).collect();
            self.select.update(cx, |select, cx| {
                select.set_items(SearchableVec::new(items), window, cx)
            });
            if let Some(first) = candidates.first().cloned() {
                self.select.update(cx, |select, cx| {
                    select.set_selected_index(
                        Some(gpui_kit::component::IndexPath::new(0)),
                        window,
                        cx,
                    )
                });
                let title = first.series_title.clone();
                self.selected = Some(first);
                self.output_input
                    .update(cx, |state, cx| state.set_value(title, window, cx));
            }
            self.preflight = Some(crate::preview::fixtures::merge_preflight());
        }
        cx.notify();
    }
}

impl gpui_kit::Render for MergeView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .id("merge-page")
            .overflow_y_scroll()
            .child(
                v_flex()
                    .max_w(gpui_kit::px(768.0))
                    .mx_auto()
                    .w_full()
                    .p_6()
                    .gap_4()
                    .children(self.render_body(cx)),
            )
    }
}

/// 预检 warning 的 i18n key → 当前语言文案。
fn merge_warning_text(key: &str, mismatch_episode: Option<u32>) -> String {
    if key == "merge.codecMismatch" {
        crate::i18n::tf(
            key,
            &[("episode", &mismatch_episode.unwrap_or(0).to_string())],
        )
    } else if key.starts_with("merge.") || key.starts_with("error.") {
        crate::i18n::t(key)
    } else {
        key.to_string()
    }
}
