//! 运行中合并任务的占用与取消，两者都挂在「输出名」上（同一剧换输出名产出不同文件，允许并行）。
//! 占用：同一输出名同时只能有一条合并在跑；取消：删掉「合并中」记录必须真的把线程停掉，
//! 否则线程照样转码、跑完还会把记录再写回去。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// 一次合并的占用凭证。Drop 时自动释放，
/// 合并线程 panic 或提前返回都不会漏掉占用。
#[derive(Debug)]
pub struct RunningMerge {
    key: String,
    /// 取消标志。删除任务时会被置位，合并线程在集与集之间检查。
    cancelled: Arc<AtomicBool>,
}

impl RunningMerge {
    /// 是否已被要求取消。
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

impl Drop for RunningMerge {
    fn drop(&mut self) {
        // 顺手把取消标志置上：panic 或提前 return 时这里是最后一道「别再往下写」保险。
        if let Some(flag) = lock_running().remove(&self.key) {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

/// 运行中合并任务表：`输出名 → 取消标志`（`HashMap::new()` 不是 const，用 `OnceLock` 延迟建表）。
fn running() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static RUNNING: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    RUNNING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 拿表锁，中毒则恢复：持锁内只做整键插入/删除，中毒只说明某条合并线程 panic 过
///（其 Drop 已清掉登记项），表本身完好。调用方有跑在主线程上的同步 command，
/// 这里若跟着 panic 会 abort 全进程——后台线程的崩溃不允许升级成应用闪退。
fn lock_running() -> std::sync::MutexGuard<'static, HashMap<String, Arc<AtomicBool>>> {
    running().lock().unwrap_or_else(|poisoned| {
        log::warn!("[Merge] 运行中任务表锁曾中毒（某条合并线程 panic 过），已恢复");
        poisoned.into_inner()
    })
}

/// 尝试占用一个输出名。已被占用时返回 `None`。
pub fn try_acquire(output_name: &str) -> Option<RunningMerge> {
    let mut guard = lock_running();
    if guard.contains_key(output_name) {
        return None;
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    guard.insert(output_name.to_string(), cancelled.clone());
    drop(guard);
    Some(RunningMerge {
        key: output_name.to_string(),
        cancelled,
    })
}

/// 取消某个输出名上正在跑的合并，返回是否确实取消了一条。
/// 记录已删但线程已自己结束时就是 `false`，这不算错误——调用方只是在清理。
pub fn cancel(output_name: &str) -> bool {
    let flag = lock_running().get(output_name).cloned();
    match flag {
        Some(flag) => {
            flag.store(true, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个用例用独立的名字，避免与别的用例（乃至并行执行）撞车。
    fn unique(tag: &str) -> String {
        format!("{tag}-{}", std::process::id())
    }

    #[test]
    fn the_same_output_name_cannot_be_taken_twice() {
        let name = unique("dup");
        let first = try_acquire(&name).expect("第一次应拿到占用");
        assert!(try_acquire(&name).is_none());
        drop(first);
        assert!(try_acquire(&name).is_some());
    }

    #[test]
    fn different_output_names_run_in_parallel() {
        let a = unique("a");
        let b = unique("b");
        let _ga = try_acquire(&a).expect("a 应拿到占用");
        let _gb = try_acquire(&b).expect("不同输出名应允许并行");
    }

    #[test]
    fn the_slot_is_released_even_if_the_holder_is_forgotten() {
        let name = unique("release");
        {
            let _held = try_acquire(&name).expect("应拿到占用");
            assert!(cancel(&name), "占着的时候应能取消");
        }
        assert!(try_acquire(&name).is_some(), "释放后应能重新占用");
    }

    #[test]
    fn cancelling_marks_the_holder_so_it_can_stop_early() {
        let name = unique("cancel");
        let held = try_acquire(&name).expect("应拿到占用");
        assert!(!held.is_cancelled());
        assert!(cancel(&name));
        assert!(held.is_cancelled(), "取消后占用方应看到标志");
    }

    #[test]
    fn cancelling_something_that_is_not_running_is_not_an_error() {
        assert!(!cancel(&unique("never-started")));
    }
}
