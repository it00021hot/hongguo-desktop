//! 合并任务。
//!
//! 两种模式：快速合并（流复制，不重编码）与兼容合并（转 H.264）。

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::app_state::AppState;
use crate::domain::model::{MergeCandidate, MergeMode, MergePreflight, MergeStatus, MergeTask};
use crate::error::{AppError, AppResult};
use crate::service::download_service::events::names;
use crate::service::merge_service::{
    candidates, compat, guard, prepare, progress, quick, remove_task,
};

/// 全部合并任务。
#[tauri::command]
pub fn get_merge_tasks(state: State<'_, AppState>) -> Vec<MergeTask> {
    state.store.merge_tasks().unwrap_or_default()
}

/// 可合并的剧列表（有已下载分集的那些）。
///
/// 按下载队列聚合而非剧集档案：档案被「移除记录」软删除后本地文件仍在，
/// 那部剧依然合得起来；一部集都没下过的剧则不该占着下拉的位置。
#[tauri::command]
pub fn get_merge_candidates(state: State<'_, AppState>) -> Vec<MergeCandidate> {
    candidates(&state)
}

/// 删除一条合并任务记录（只删记录，不删已产出的文件）。
///
/// 记录若还在 running，这里会**先取消后台线程再删**：只删记录的话，线程照样
/// 转码、照样吃 CPU，跑完还会把这条记录再写回 data.json——用户刚删掉的东西
/// 又冒出来。取消是协作式的，线程会在集与集之间停下（最多再浪费一集的时间）。
#[tauri::command]
pub fn delete_merge_task(state: State<'_, AppState>, id: String) -> AppResult<()> {
    if let Some(task) = state
        .store
        .merge_tasks()?
        .iter()
        .find(|t| t.id == id)
        .cloned()
    {
        if guard::cancel(&task.output_name) {
            log::info!("[Merge] 已请求取消 {}", task.output_name);
        }
    }
    remove_task(&state, &id)
}

/// 在文件管理器中打开某条合并任务的产物所在目录。
///
/// 合并完最顺手的下一步就是拿走成品：让用户自己去下载目录里翻
/// `<下载根>/红果短剧/<剧名>/xxx 合集.mp4`，不如一键定位到它。
#[tauri::command]
pub fn open_merge_output(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;

    let path = state
        .store
        .merge_tasks()?
        .iter()
        .find(|t| t.id == id)
        .map(|t| t.output_path.clone())
        .filter(|p| !p.trim().is_empty())
        .ok_or_else(|| AppError::NotFound(format!("合并任务 {id} 没有产物路径")))?;

    app.opener()
        .reveal_item_in_dir(std::path::Path::new(&path))
        .map_err(|e| AppError::Io(e.to_string()))
}

/// 合并前校验。
#[tauri::command]
pub fn merge_preflight(state: State<'_, AppState>, series_id: String) -> AppResult<MergePreflight> {
    prepare::preflight(&state, &series_id)
}

/// 启动合并。
///
/// 合并是**重活**：兼容合并要把每集转成 H.264，10 集能跑几分钟。
/// 所以这里只在主线程上做「登记 + 派发」，真正的活丢给 blocking 线程池
/// （[`tauri::async_runtime::spawn_blocking`]）——同步 command 跑在主线程上，
/// 直接转码会让整个窗口（包括重绘与事件循环）停摆，表现就是标题栏
/// 「未响应」且界面整体泛白。
///
/// 进度通过 `merge-progress` 事件推送：每完成一集发一次，
/// 收尾再发一次 `merge-completed` / `merge-failed`。
///
/// 合并本身失败不返回 Err：任务记录已经带上失败原因与 i18n key，
/// 事件也发过了，返回 Err 只会让前端多弹一个 toast。
#[tauri::command]
pub async fn merge_series(
    app: AppHandle,
    state: State<'_, AppState>,
    series_id: String,
    output_name: String,
    mode: MergeMode,
) -> AppResult<MergeTask> {
    // 先占坑再干活。少了这一步，用户每点一次「开始合并」就多开一条
    // spawn_blocking，界面上会攒出一串同名「合并中」，而它们在抢同一个
    // 输出文件、还各占一份 CPU。按输出名去重，不按剧：
    // 同一剧换输出名产出的是不同文件，允许并行。
    let slot = guard::try_acquire(&output_name)
        .ok_or_else(|| AppError::Busy(format!("《{output_name}》正在合并中，等这次跑完再试")))?;

    // 剧名优先取任务记录：合并页的候选就是按它聚合的，
    // 档案被「移除记录」软删除后这里不该退化成用户输入的输出名。
    let series_title = state
        .queue()
        .of_series(&series_id)
        .first()
        .map(|t| t.series_title.clone())
        .filter(|t| !t.is_empty())
        .or_else(|| {
            state
                .store
                .series_by_id(&series_id)
                .ok()
                .flatten()
                .map(|s| s.title.clone())
        })
        .unwrap_or_else(|| output_name.clone());

    let mut task = MergeTask::new(&series_id, &series_title, &output_name, mode);
    task.status = MergeStatus::Running;
    // 开跑就落库。跑到结束才落的话，进程一崩这十几分钟的进度就凭空消失，
    // 用户重开应用既看不到「刚才合过」也看不到「合到哪了」。
    upsert_merge_task(&state, &task);
    // 合并可能跑好几分钟，先广播「开始了」，UI 立刻能看到 running 状态
    let _ = app.emit(names::MERGE_TASK_ADDED, &task);

    let emit_app = app.clone();
    let store_app = state.inner().clone();
    // 集内回调每帧都会来（30fps × 并行路数），按 1% 步进打闸：
    // 事件与落库都只在新百分比跨档时发生，一集几十条封顶。
    let last_step = std::sync::atomic::AtomicI64::new(i64::MIN);
    let on_progress: progress::ProgressSink = Arc::new(move |done, total, t| {
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
        // 参与集数在任务登记时数不出来（要等合并内部盘点输入），
        // 每次上报都带上，运行中的快照才不用一直挂着 0
        snapshot.episode_count = total;
        let _ = emit_app.emit(names::MERGE_PROGRESS, &snapshot);
        // 进度同步落库：get_merge_tasks 中途就能看到，进程崩了重开
        // 也至少能停在「最后跨过的那个百分点」
        upsert_merge_task_owned(&store_app, &snapshot);
    });

    let inner = state.inner().clone();
    let owned_series = series_id.clone();
    let owned_output = output_name.clone();
    let owned_task = task.clone();

    tauri::async_runtime::spawn_blocking(move || {
        // slot 随这条闭包一起活到合并结束，Drop 时自动释放占用。
        // 中途 panic 也会释放：用户不必重启就能再点一次。
        let slot = slot;
        let state = inner;
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

        // 用户主动取消：记录已经在 delete_merge_task 里删掉了。
        // 这里再写回去就是「刚删掉又冒出来」，所以直接收工不发事件。
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
                // 与下载任务同一个约定：记录里存 i18n key（前端会 t() 它），
                // 内部细节只进日志。直接把 e.to_string() 存进去等于把后端术语糊给用户。
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
        upsert_merge_task_owned(&state, &task);

        let _ = app.emit(
            if task.status == MergeStatus::Failed {
                names::MERGE_FAILED
            } else {
                names::MERGE_COMPLETED
            },
            &task,
        );
    });

    // 立刻返回 running 任务：前端据此把列表刷新成「合并中」，
    // 后续进度与结果都走事件推送
    Ok(task)
}

/// 把合并任务写进数据库。失败只记日志。
///
/// 记录本身不是产物：为了写一条状态把整个任务判失败，会让用户以为
/// 白转了十几分钟。开始时先插一条 running 记录让 UI 立刻看到任务；
/// 跑完后再用同一条覆盖它。
fn upsert_merge_task(state: &State<'_, AppState>, task: &MergeTask) {
    upsert_merge_task_owned(state.inner(), task);
}

/// [`upsert_merge_task`] 的自有类型版本，供后台线程使用
/// （后台线程拿不到 command 的 `State` 生命周期）。
fn upsert_merge_task_owned(state: &AppState, task: &MergeTask) {
    if let Err(e) = state.store.upsert_merge_task(task) {
        log::error!("[Merge] 合并记录落库失败: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合并必须离开主线程。
    ///
    /// 同步 command 跑在主线程上，兼容合并要逐集转 H.264（10 集要几分钟），
    /// 直接在里面跑会让整个窗口停止响应——这正是之前「卡死」的成因。
    ///
    /// 类型系统在这里帮不上忙（async fn 与 fn 的返回值无法在测试里直接比对），
    /// 所以退一步做源码检查：函数签名里必须有 `async fn`。
    /// 写法够土，但它能在有人把 async 去掉时立刻失败。
    #[test]
    fn merge_series_is_async_so_transcoding_leaves_the_main_thread() {
        let src = include_str!("merge_cmd.rs");
        let sig = src
            .lines()
            .find(|l| l.contains("fn merge_series("))
            .expect("应能找到 merge_series 的签名");
        assert!(
            sig.contains("pub async fn"),
            "merge_series 必须是 async：同步 command 会在主线程上跑转码，窗口直接卡死。实际: {sig}"
        );
    }

    /// 转码工作必须派发到 blocking 线程池，而不是留在 command 里直接跑。
    #[test]
    fn merge_work_runs_on_the_blocking_pool() {
        let src = include_str!("merge_cmd.rs");
        assert!(
            src.contains("spawn_blocking"),
            "重活必须交给 spawn_blocking，否则仍会占住调用线程"
        );
    }

    #[test]
    fn upsert_replaces_the_same_id_instead_of_appending() {
        let state = AppState::default();
        let mut a = MergeTask::new("1", "剧", "out", MergeMode::Quick);
        a.status = MergeStatus::Running;
        upsert_merge_task_owned(&state, &a);

        let mut done = a.clone();
        done.mark_completed("out.mp4", 100);
        done.status = MergeStatus::Completed;
        upsert_merge_task_owned(&state, &done);

        let tasks = state.store.merge_tasks().unwrap();
        assert_eq!(tasks.len(), 1, "同一条任务应被覆盖而不是追加");
        assert_eq!(tasks[0].status, MergeStatus::Completed);
    }
}
