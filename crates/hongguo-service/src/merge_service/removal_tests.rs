use super::*;
use hongguo_common::model::{MergeMode, MergeStatus, MergeTask};

#[test]
fn removing_a_running_task_cancels_its_producer_before_removing_the_record() {
    let state = AppState::default();
    let mut task = MergeTask::new("offline", "剧", "", MergeMode::Quick);
    task.output_name = format!("remove-running-{}", task.id);
    task.status = MergeStatus::Running;
    state.store.upsert_merge_task(&task).unwrap();
    let producer = guard::try_acquire(&task.output_name).unwrap();
    remove_task(&state, &task.id).unwrap();
    assert!(
        producer.is_cancelled(),
        "删除记录必须同时取消仍运行的合并，避免后台完成后重新写回"
    );
    assert!(state.store.merge_tasks().unwrap().is_empty());
}

#[test]
fn removing_pending_and_completed_records_does_not_cancel_other_outputs() {
    let state = AppState::default();
    let mut pending = MergeTask::new("offline", "剧", "", MergeMode::Quick);
    pending.output_name = format!("remove-pending-{}", pending.id);
    let mut completed = MergeTask::new("offline", "剧", "", MergeMode::Compat);
    completed.output_name = pending.output_name.clone();
    completed.mark_completed("offline.mp4", 12);
    let unrelated = guard::try_acquire(&pending.output_name).unwrap();
    state.store.upsert_merge_task(&pending).unwrap();
    state.store.upsert_merge_task(&completed).unwrap();
    remove_task(&state, &pending.id).unwrap();
    remove_task(&state, &completed.id).unwrap();
    assert!(state.store.merge_tasks().unwrap().is_empty());
    assert!(!unrelated.is_cancelled());
}

#[test]
fn removing_an_unknown_record_propagates_not_found_and_preserves_active_work() {
    let state = AppState::default();
    let mut task = MergeTask::new("offline", "剧", "", MergeMode::Quick);
    task.output_name = format!("remove-missing-{}", task.id);
    state.store.upsert_merge_task(&task).unwrap();
    let producer = guard::try_acquire(&task.output_name).unwrap();
    assert!(matches!(
        remove_task(&state, "missing-task"),
        Err(AppError::NotFound(_))
    ));
    assert!(!producer.is_cancelled());
    assert_eq!(state.store.merge_tasks().unwrap().len(), 1);
}
