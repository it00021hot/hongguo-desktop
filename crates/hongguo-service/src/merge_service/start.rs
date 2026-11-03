//! 合并任务的启动编排：占用输出名 → 登记 running 记录并广播 → 后台 blocking 线程跑合并
//!（进度 1% 步进广播 + 落库）→ 结束写回结果并广播。重活派发到共用任务运行时的阻塞池，
//! 直接在调用线程转码会让窗口停摆。

use std::sync::Arc;

use hongguo_common::model::{MergeMode, MergeStatus, MergeTask};
use hongguo_common::{AppError, AppResult};

use super::{compat, guard, progress, quick};
use crate::app_state::AppState;
use crate::events::ServiceEvent;

/// 启动合并，返回 running 任务快照（前端据此刷新列表，后续走事件）。
/// 合并本身失败不返回 Err：任务记录已带失败原因与 i18n key，事件也发过了。
pub fn merge_series(
    state: &AppState,
    series_id: &str,
    output_name: &str,
    mode: MergeMode,
) -> AppResult<MergeTask> {
    // 先占坑再干活：按输出名去重（同一剧换输出名产出的是不同文件，允许并行）；
    // 不去重的话每点一次按钮就多开一条 blocking 任务，抢同一个输出文件、各占一份 CPU。
    let slot = guard::try_acquire(output_name)
        .ok_or_else(|| AppError::Busy(format!("《{output_name}》正在合并中，等这次跑完再试")))?;

    // 剧名优先取任务记录：合并页的候选就是按它聚合的，
    // 档案被「移除记录」软删除后这里不该退化成用户输入的输出名。
    let series_title = state
        .queue()
        .of_series(series_id)
        .first()
        .map(|t| t.series_title.clone())
        .filter(|t| !t.is_empty())
        .or_else(|| {
            state
                .store
                .series_by_id(series_id)
                .ok()
                .flatten()
                .map(|s| s.title.clone())
        })
        .unwrap_or_else(|| output_name.to_string());

    let mut task = MergeTask::new(series_id, &series_title, output_name, mode);
    task.status = MergeStatus::Running;
    // 开跑就落库：进程一崩，重开应用至少还能看到「合到哪了」。
    upsert(state, &task);
    // 合并可能跑好几分钟，先广播「开始了」，UI 立刻能看到 running 状态
    state
        .events()
        .publish(ServiceEvent::MergeTaskAdded(task.clone()));

    let state = state.clone();
    // 集内回调每帧都会来（30fps × 并行路数），按 1% 步进打闸：
    // 事件与落库都只在新百分比跨档时发生，一集几十条封顶。
    let last_step = std::sync::atomic::AtomicI64::new(i64::MIN);
    let on_progress: progress::ProgressSink = {
        let events = state.events().clone();
        let progress_state = state.clone();
        Arc::new(move |done, total, t| {
            let percent = if total == 0 {
                0.0
            } else {
                (done / total as f64 * 100.0).clamp(0.0, 100.0)
            };
            let step = (percent * 100.0).round() as i64;
            if last_step.swap(step, std::sync::atomic::Ordering::Relaxed) == step {
                return;
            }
            let mut snapshot = t.clone();
            snapshot.percent = percent;
            // 参与集数在任务登记时数不出来（要等合并内部盘点输入），每次上报都带上。
            snapshot.episode_count = total;
            events.publish(ServiceEvent::MergeProgress(snapshot.clone()));
            // 进度同步落库：进程崩了重开至少停在「最后跨过的那个百分点」。
            upsert(&progress_state, &snapshot);
        })
    };

    let owned_series = series_id.to_string();
    let owned_output = output_name.to_string();
    let owned_task = task.clone();

    crate::runtime::spawn_blocking(move || {
        // slot 随闭包活到合并结束，Drop 时自动释放占用（中途 panic 也会释放）。
        let slot = slot;
        let result = match mode {
            MergeMode::Quick => quick::quick_merge(
                &state,
                &owned_series,
                &owned_output,
                &owned_task,
                &on_progress,
                &slot,
            ),
            MergeMode::Compat => compat::compat_merge(
                &state,
                &owned_series,
                &owned_output,
                &owned_task,
                &on_progress,
                &slot,
            ),
        };

        // 用户主动取消：记录已在 delete_merge_task 里删掉，再写回去就是「刚删掉又冒出来」。
        if matches!(result, Err(AppError::Cancelled)) {
            log::info!("[Merge] {} 已取消，不再写回记录", owned_output);
            return;
        }

        let mut task = owned_task;
        match result {
            Ok((path, size, count)) => {
                task.mark_completed(&path.to_string_lossy(), size);
                task.episode_count = count;
            }
            Err(e) => {
                // 与下载任务同一约定：记录里存 i18n key（前端会 t() 它），内部细节只进日志。
                log::error!("[Merge] {} 合并失败: {e}", task.output_name);
                task.mark_failed(e.i18n_key());
            }
        }
        task.status = if task.error.is_empty() {
            MergeStatus::Completed
        } else {
            MergeStatus::Failed
        };

        // 覆盖刚才那条 pending/running 记录，这次连同结果一起落库
        upsert(&state, &task);

        state
            .events()
            .publish(if task.status == MergeStatus::Failed {
                ServiceEvent::MergeFailed(task)
            } else {
                ServiceEvent::MergeCompleted(task)
            });
    });

    // 立刻返回 running 任务：前端据此把列表刷新成「合并中」，
    // 后续进度与结果都走事件推送
    Ok(task)
}

/// 把合并任务写进数据库，失败只记日志（记录本身不是产物，为写状态把任务判失败会让用户以为白转了）。
/// 开始时先插一条 running 记录让 UI 立刻看到任务；跑完后再用同一条覆盖它。
fn upsert(state: &AppState, task: &MergeTask) {
    if let Err(e) = state.store.upsert_merge_task(task) {
        log::error!("[Merge] 合并记录落库失败: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 转码工作必须派发到共用运行时的阻塞池，而不是留在 command/主线程里跑。
    ///（假针拼接再匹配：断言字符串自身会出现在本文件里，直接写会永远为真。）
    #[test]
    fn merge_work_runs_on_the_blocking_pool() {
        let needle = ["crate::runtime::spawn", "_blocking(move ||"].concat();
        assert!(
            include_str!("start.rs").contains(&needle),
            "重活必须交给共用运行时的阻塞池，否则仍会占住调用线程"
        );
    }

    #[test]
    fn upsert_replaces_the_same_id_instead_of_appending() {
        let state = AppState::default();
        let mut a = MergeTask::new("1", "剧", "out", MergeMode::Quick);
        a.status = MergeStatus::Running;
        upsert(&state, &a);

        let mut done = a.clone();
        done.mark_completed("out.mp4", 100);
        done.status = MergeStatus::Completed;
        upsert(&state, &done);

        let tasks = state.store.merge_tasks().unwrap();
        assert_eq!(tasks.len(), 1, "同一条任务应被覆盖而不是追加");
        assert_eq!(tasks[0].status, MergeStatus::Completed);
    }
}
