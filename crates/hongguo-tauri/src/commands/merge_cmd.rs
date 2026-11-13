//! 合并任务：无损流复制，或统一编码为 HEVC 后合并。
//!
//! 启动编排（占坑/登记/进度/落库/事件）在 service 的 `merge_service::start`；这里只做查询与转调。

use tauri::{AppHandle, State};

use hongguo_common::model::{MergeCandidate, MergeMode, MergePreflight, MergeTask};
use hongguo_common::{AppError, AppResult};
use hongguo_service::app_state::AppState;
use hongguo_service::merge_service::{candidates, prepare, remove_task};

/// 全部合并任务。
#[tauri::command]
pub fn get_merge_tasks(state: State<'_, AppState>) -> AppResult<Vec<MergeTask>> {
    state.store.merge_tasks()
}

/// 可合并的剧列表（有已下载分集的那些）。按下载队列聚合而非剧集档案：档案被「移除记录」软删除后本地文件仍在，那部剧依然合得起来；一部集都没下过的剧则不该占着下拉的位置。
#[tauri::command]
pub fn get_merge_candidates(state: State<'_, AppState>) -> Vec<MergeCandidate> {
    candidates(&state)
}

/// 删除一条合并任务记录（只删记录，不删已产出的文件）。记录若还在 running，这里会**先取消后台线程再删**——只删记录的话线程照样转码吃 CPU，跑完还会把这条记录写回存储（用户刚删掉的东西又冒出来）；取消是协作式的，在集与集之间停下。
#[tauri::command]
pub fn delete_merge_task(state: State<'_, AppState>, id: String) -> AppResult<()> {
    remove_task(&state, &id)
}

/// 在文件管理器中打开某条合并任务的产物所在目录（合并完最顺手的下一步就是拿走成品）。
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

/// 启动合并。返回 running 任务快照；进度与结果经 `merge-*` 事件推送。重活由 service（`merge_service::start`）派发到共用任务运行时的阻塞池——同步 command 在主线程直接转码会让整个窗口停摆。
/// 合并本身失败不返回 Err：任务记录已经带上失败原因与 i18n key、事件也发过了，返回 Err 只会让前端多弹一个 toast。
#[tauri::command]
pub async fn merge_series(
    state: State<'_, AppState>,
    series_id: String,
    output_name: String,
    mode: MergeMode,
) -> AppResult<MergeTask> {
    hongguo_service::merge_service::start::merge_series(
        state.inner(),
        &series_id,
        &output_name,
        mode,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn merge_series_is_async_so_the_main_thread_is_not_blocked() {
        let src = include_str!("merge_cmd.rs");
        let sig = src
            .lines()
            .find(|l| l.contains("fn merge_series("))
            .expect("应能找到 merge_series 的签名");
        assert!(
            sig.contains("pub async fn"),
            "merge_series 必须是 async。实际: {sig}"
        );
    }
}
