use super::downloaded_file;
use hongguo_common::model::{DownloadTask, TaskStatus};
use hongguo_service::download_service::queue::DownloadQueue;
use std::path::PathBuf;

fn task(id: &str, episode: u32, status: TaskStatus, path: &str) -> DownloadTask {
    let mut task = DownloadTask::new("same-series", "Same series", episode, id, "Episode");
    task.id = id.into();
    task.status = status;
    task.file_path = path.into();
    task
}

#[test]
fn each_completed_episode_exports_its_own_path_even_when_the_series_is_shared() {
    let queue = DownloadQueue::restore(
        vec![
            task("first", 1, TaskStatus::Completed, "/fixture/one.mp4"),
            task("second", 2, TaskStatus::Completed, "/fixture/two.mp4"),
        ],
        1,
    );
    assert_eq!(
        downloaded_file(&queue, "first").unwrap(),
        PathBuf::from("/fixture/one.mp4")
    );
    assert_eq!(
        downloaded_file(&queue, "second").unwrap(),
        PathBuf::from("/fixture/two.mp4")
    );
    assert!(downloaded_file(&queue, "same-series").is_err());
}

#[test]
fn missing_incomplete_and_pathless_tasks_are_not_exportable() {
    assert!(downloaded_file(&DownloadQueue::new(), "missing").is_err());
    let queue = DownloadQueue::restore(
        vec![
            task("partial", 1, TaskStatus::Pending, "/fixture/partial.mp4"),
            task("pathless", 2, TaskStatus::Completed, ""),
        ],
        1,
    );
    assert!(downloaded_file(&queue, "partial").is_err());
    assert!(downloaded_file(&queue, "pathless").is_err());
    assert!(downloaded_file(&queue, "missing").is_err());
}
