//! 下载队列的单元测试。
//!
//! 以 `#[path]` 挂到 `queue.rs` 下，这样 `use super::*` 能拿到实现里的
//! 私有字段，测试可以直接断言内部状态。

use super::*;

fn task(idx: u32) -> DownloadTask {
    DownloadTask::new("1", "剧", idx, &format!("v{idx}"), "")
}

#[test]
fn enqueue_is_idempotent_per_episode() {
    let q = DownloadQueue::new();
    q.enqueue(task(1));
    q.enqueue(task(1));
    assert_eq!(q.all().len(), 1, "同一集不应重复入队");
}

#[test]
fn mark_running_counts_active() {
    let q = DownloadQueue::new();
    let t = q.enqueue(task(1));
    assert!(q.mark_running(&t.id));
    assert_eq!(q.status().active, 1);
    assert_eq!(q.status().running, 1);
}

#[test]
fn completed_releases_active_slot() {
    let q = DownloadQueue::new();
    let t = q.enqueue(task(1));
    q.mark_running(&t.id);
    q.mark_completed(&t.id, "1.mp4", 100);
    assert_eq!(q.status().active, 0);
    assert_eq!(q.status().completed, 1);
}

#[test]
fn set_limit_is_clamped() {
    let q = DownloadQueue::new();
    q.set_limit(99);
    assert_eq!(q.limit(), 10);
    q.set_limit(0);
    assert_eq!(q.limit(), 1);
}

#[test]
fn pause_all_stops_pending_and_running() {
    let q = DownloadQueue::new();
    let a = q.enqueue(task(1));
    let b = q.enqueue(task(2));
    q.mark_running(&a.id);
    assert_eq!(q.pause_all(), 2);
    assert_eq!(q.status().active, 0);
    assert!(!q.get(&b.id).unwrap().status.is_runnable());
}

#[test]
fn resume_all_requeues_stopped_and_failed() {
    let q = DownloadQueue::new();
    let a = q.enqueue(task(1));
    let b = q.enqueue(task(2));
    q.mark_failed(&a.id, "网络");
    q.mark_running(&b.id);
    q.pause_all();

    assert_eq!(q.resume_all(), 2);
    assert_eq!(q.status().pending, 2);
}

#[test]
fn retry_resets_single_task() {
    let q = DownloadQueue::new();
    let t = q.enqueue(task(1));
    q.mark_failed(&t.id, "断了");
    let back = q.retry(&t.id).unwrap();
    assert_eq!(back.status, TaskStatus::Pending);
    assert!(back.error.is_empty());
}

#[test]
fn remove_deletes_tasks() {
    let q = DownloadQueue::new();
    let a = q.enqueue(task(1));
    let b = q.enqueue(task(2));
    assert_eq!(q.remove(std::slice::from_ref(&a.id)), 1);
    assert_eq!(q.all().len(), 1);
    assert_eq!(q.remove(&[b.id]), 1);
    assert!(q.all().is_empty());
}

#[test]
fn remove_episode_drops_only_that_episode() {
    // 文件被清掉后必须连记录一起摘：`completed_path` 不看文件是否存在，
    // 记录留着就会让播放去开一个不存在的文件，直接「媒体处理失败」。
    let q = DownloadQueue::new();
    let a = q.enqueue(task(1));
    q.mark_completed(&a.id, "a.mp4", 10);
    let b = q.enqueue(task(2));
    q.mark_completed(&b.id, "b.mp4", 20);

    assert_eq!(q.remove_episode("1", 1), 1);
    assert!(q.completed_path("1", 1).is_none(), "第1集不该再有成品路径");
    assert_eq!(
        q.completed_path("1", 2).as_deref(),
        Some("b.mp4"),
        "别的集不该被动到"
    );
    assert_eq!(q.remove_episode("1", 99), 0, "不存在的集应删不到");
}

#[test]
fn restore_requeues_interrupted_tasks() {
    let mut t = task(1);
    t.status = TaskStatus::Running;
    let q = DownloadQueue::restore(vec![t], 5);
    assert_eq!(q.status().pending, 1);
    assert_eq!(q.limit(), 5);
}

#[test]
fn finishing_a_task_after_pause_all_does_not_underflow_active() {
    // pause_all 会把 active 直接清零，而在途任务随后才回来报告收尾。
    // 计数器是 usize，硬减会下溢 panic。
    let q = DownloadQueue::new();
    let a = q.enqueue(task(1));
    q.mark_running(&a.id);
    q.pause_all();
    assert_eq!(q.status().active, 0);
    q.mark_stopped(&a.id);
    assert_eq!(q.status().active, 0);
}
