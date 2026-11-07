//! 增长中的本地成片。一次响应只给出已经落盘的一段，这段响应结束不等于整片结束。
//! 下一次从上次终点接着读；写完、失败或取消都会把等在后面的读取叫醒。

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};
use std::time::Duration;

use hongguo_stream::range::{ProtocolResponse, RangeSpec, parse_range};

use crate::local::{content_type, local_play_url};

const GROW_HEADER: &str = "x-hongguo-grow";
const GROW_CHUNK: u64 = 4 * 1024 * 1024;
const WAIT_SLICE: Duration = Duration::from_secs(3600);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Open,
    Done,
    Failed,
    Cancelled,
}

struct State {
    committed: u64,
    phase: Phase,
    message: String,
}

struct Slot {
    state: Mutex<State>,
    cv: Condvar,
}

static SLOTS: LazyLock<Mutex<HashMap<PathBuf, Arc<Slot>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn map_lock() -> MutexGuard<'static, HashMap<PathBuf, Arc<Slot>>> {
    SLOTS.lock().unwrap_or_else(|p| p.into_inner())
}

fn lookup(path: &Path) -> Option<Arc<Slot>> {
    map_lock().get(path).cloned()
}

fn lock_state(slot: &Slot) -> MutexGuard<'_, State> {
    slot.state.lock().unwrap_or_else(|p| p.into_inner())
}

/// 登记一条还会继续长大的成片。扩展名不在播放白名单里直接拒绝。已有未完成登记时返回错误。
pub fn open(path: &Path) -> Result<(), String> {
    let text = path.to_string_lossy();
    if local_play_url(&text).is_none() {
        return Err("只允许播放视频文件".into());
    }
    let mut map = map_lock();
    if map
        .get(path)
        .is_some_and(|slot| lock_state(slot).phase == Phase::Open)
    {
        return Err("这一集正在转码".into());
    }
    map.insert(
        path.to_path_buf(),
        Arc::new(Slot {
            state: Mutex::new(State {
                committed: 0,
                phase: Phase::Open,
                message: String::new(),
            }),
            cv: Condvar::new(),
        }),
    );
    Ok(())
}

/// 已经可以安全读取的字节数。只能往前走。
pub fn commit(path: &Path, len: u64) {
    let Some(slot) = lookup(path) else {
        return;
    };
    let mut state = lock_state(&slot);
    if state.phase != Phase::Open || len <= state.committed {
        return;
    }
    state.committed = len;
    slot.cv.notify_all();
}

pub fn finish(path: &Path) {
    close(path, Phase::Done, String::new());
}

pub fn fail(path: &Path, message: impl Into<String>) {
    close(path, Phase::Failed, message.into());
}

pub fn cancel(path: &Path) {
    close(path, Phase::Cancelled, "已取消".into());
}

/// 还有写入在进行。完成后的整文件按普通本地响应供给。
pub fn is_incomplete(path: &Path) -> bool {
    lookup(path).is_some_and(|slot| lock_state(&slot).phase == Phase::Open)
}

pub(crate) fn serve_if_registered(
    path: &Path,
    range_header: Option<&str>,
) -> Option<Result<ProtocolResponse, String>> {
    let slot = lookup(path)?;
    let phase = lock_state(&slot).phase;
    if phase == Phase::Done && range_header.is_none() {
        return None;
    }
    Some(serve_live(path, range_header))
}

fn serve_live(path: &Path, range_header: Option<&str>) -> Result<ProtocolResponse, String> {
    let spec = parse_range(range_header, u64::MAX);
    let start = match spec {
        RangeSpec::Full => 0,
        RangeSpec::Open { start } | RangeSpec::Closed { start, .. } => start,
        RangeSpec::Unsatisfiable => 0,
    };
    let closed_end = match spec {
        RangeSpec::Closed { end, .. } => Some(end),
        _ => None,
    };
    let (committed, phase, message) = wait_until(path, start)?;
    if start >= committed {
        return Ok(terminal(phase, &message));
    }
    let mut take = committed - start;
    if let Some(end) = closed_end {
        take = take.min(end.saturating_sub(start).saturating_add(1));
    }
    take = take.min(GROW_CHUNK);
    let bytes = read_file(path, start, take)?;
    let end = start + bytes.len() as u64;
    let reaches = end == committed;
    let signal = match phase {
        Phase::Done if reaches => "done",
        Phase::Cancelled if reaches => "cancelled",
        Phase::Failed if reaches => "failed",
        _ => "open",
    };
    let total = if phase == Phase::Done {
        committed.to_string()
    } else {
        "*".into()
    };
    let last = end.saturating_sub(1);
    Ok((
        206,
        vec![
            ("Content-Type".into(), content_type(path).into()),
            ("Accept-Ranges".into(), "bytes".into()),
            (
                "Content-Range".into(),
                format!("bytes {start}-{last}/{total}"),
            ),
            ("Content-Length".into(), bytes.len().to_string()),
            (GROW_HEADER.into(), signal.into()),
        ],
        bytes,
    ))
}

fn wait_until(path: &Path, start: u64) -> Result<(u64, Phase, String), String> {
    let Some(slot) = lookup(path) else {
        return Err("增长中的成片已结束".into());
    };
    let mut state = lock_state(&slot);
    loop {
        if start < state.committed || state.phase != Phase::Open {
            return Ok((state.committed, state.phase, state.message.clone()));
        }
        let (next, timeout) = slot
            .cv
            .wait_timeout(state, WAIT_SLICE)
            .unwrap_or_else(|p| p.into_inner());
        state = next;
        if timeout.timed_out() && state.phase == Phase::Open && start >= state.committed {
            continue;
        }
    }
}

fn terminal(phase: Phase, message: &str) -> ProtocolResponse {
    let (status, signal) = match phase {
        Phase::Done => (204, "done"),
        Phase::Cancelled => (409, "cancelled"),
        Phase::Failed | Phase::Open => (500, "failed"),
    };
    (
        status,
        vec![
            ("Content-Type".into(), "text/plain".into()),
            (GROW_HEADER.into(), signal.into()),
        ],
        message.as_bytes().to_vec(),
    )
}

fn read_file(path: &Path, start: u64, len: u64) -> Result<Vec<u8>, String> {
    let mut file = File::open(path).map_err(|e| format!("打开文件失败: {e}"))?;
    file.seek(SeekFrom::Start(start))
        .map_err(|e| format!("定位失败: {e}"))?;
    let mut buf = vec![0u8; len as usize];
    file.read_exact(&mut buf)
        .map_err(|e| format!("读取文件失败: {e}"))?;
    Ok(buf)
}

fn close(path: &Path, phase: Phase, message: String) {
    let Some(slot) = lookup(path) else {
        return;
    };
    let mut state = lock_state(&slot);
    if state.phase == Phase::Open {
        state.phase = phase;
        state.message = message;
    }
    slot.cv.notify_all();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;

    use crate::local::serve;

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/offline-qa-unit")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("clip.mp4")
    }

    fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn encoded(path: &Path) -> String {
        let url = local_play_url(path.to_str().unwrap()).unwrap();
        url.rsplit('/').next().unwrap().to_string()
    }

    #[test]
    fn the_first_chunk_is_readable_before_later_bytes_exist() {
        let path = scratch("append");
        std::fs::write(&path, b"prefix-bytes").unwrap();
        open(&path).unwrap();
        commit(&path, 12);
        let (ready_tx, ready_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel();
        let path2 = path.clone();
        let reader = thread::spawn(move || {
            let first = serve(&encoded(&path2), Some("bytes=0-")).unwrap();
            ready_tx.send(()).unwrap();
            go_rx.recv().unwrap();
            let second = serve(&encoded(&path2), Some("bytes=12-")).unwrap();
            (first, second)
        });
        ready_rx.recv().unwrap();
        assert!(is_incomplete(&path));
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        use std::io::Write;
        file.write_all(b"-more").unwrap();
        commit(&path, 17);
        finish(&path);
        go_tx.send(()).unwrap();
        let (first, second) = reader.join().unwrap();
        assert_eq!(first.0, 206);
        assert_eq!(first.2, b"prefix-bytes");
        assert_eq!(header(&first.1, GROW_HEADER), Some("open"));
        assert_eq!(second.2, b"-more");
        assert_eq!(header(&second.1, GROW_HEADER), Some("done"));
        assert!(!is_incomplete(&path));
        let whole = serve(&encoded(&path), None).unwrap();
        assert_eq!(whole.0, 200);
        assert_eq!(whole.2, b"prefix-bytes-more");
        assert!(header(&whole.1, GROW_HEADER).is_none());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn cancel_wakes_a_reader_waiting_past_the_prefix() {
        let path = scratch("cancel");
        std::fs::write(&path, b"prefix").unwrap();
        open(&path).unwrap();
        commit(&path, 6);
        let (ready_tx, ready_rx) = mpsc::channel();
        let path2 = path.clone();
        let reader = thread::spawn(move || {
            ready_tx.send(()).unwrap();
            serve(&encoded(&path2), Some("bytes=6-")).unwrap()
        });
        ready_rx.recv().unwrap();
        cancel(&path);
        let response = reader.join().unwrap();
        assert_eq!(response.0, 409);
        assert_eq!(header(&response.1, GROW_HEADER), Some("cancelled"));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn failure_wakes_a_reader_waiting_past_the_prefix() {
        let path = scratch("fail");
        std::fs::write(&path, b"prefix").unwrap();
        open(&path).unwrap();
        commit(&path, 6);
        let (ready_tx, ready_rx) = mpsc::channel();
        let path2 = path.clone();
        let reader = thread::spawn(move || {
            ready_tx.send(()).unwrap();
            serve(&encoded(&path2), Some("bytes=6-")).unwrap()
        });
        ready_rx.recv().unwrap();
        fail(&path, "转码失败");
        let response = reader.join().unwrap();
        assert_eq!(response.0, 500);
        assert_eq!(response.2, "转码失败".as_bytes());
        assert_eq!(header(&response.1, GROW_HEADER), Some("failed"));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_plain_file_without_a_session_does_not_wait() {
        let path = scratch("plain");
        std::fs::write(&path, b"whole-file").unwrap();
        let response = serve(&encoded(&path), None).unwrap();
        assert_eq!(response.0, 200);
        assert_eq!(response.2, b"whole-file");
        assert!(header(&response.1, GROW_HEADER).is_none());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_non_video_path_cannot_be_registered() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/offline-qa-unit")
            .join(format!("secret-{}.txt", std::process::id()));
        let err = open(&path).unwrap_err();
        assert!(err.contains("视频"), "实际 {err}");
        assert!(!is_incomplete(&path));
    }
}
