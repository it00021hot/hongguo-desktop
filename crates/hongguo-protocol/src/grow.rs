//! 增长中的本地成片。一次响应只给出已经落盘的一段，这段响应结束不等于整片结束。
//! 下一次从上次终点接着读；写完、失败或取消都会把等在后面的读取叫醒。

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};
use std::time::Duration;

use hongguo_stream::range::ProtocolResponse;

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
    match std::fs::metadata(path) {
        Ok(file) => close(path, Phase::Done, String::new(), Some(file.len())),
        Err(error) => fail(path, format!("读取已完成视频长度失败: {error}")),
    }
}

pub fn fail(path: &Path, message: impl Into<String>) {
    close(path, Phase::Failed, message.into(), None);
}

pub fn cancel(path: &Path) {
    close(path, Phase::Cancelled, "已取消".into(), None);
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

enum Want {
    Full,
    From(u64),
    Closed { start: u64, end: u64 },
    Suffix(u64),
    Malformed,
}

fn classify_range(header: Option<&str>) -> Want {
    let Some(header) = header else {
        return Want::Full;
    };
    let Some(spec) = header.trim().strip_prefix("bytes=") else {
        return Want::Full;
    };
    if spec.contains(',') {
        return Want::Full;
    }
    let Some((start_s, end_s)) = spec.split_once('-') else {
        return Want::Malformed;
    };
    if start_s.is_empty() {
        return match end_s.parse::<u64>() {
            Ok(0) | Err(_) => Want::Malformed,
            Ok(len) => Want::Suffix(len),
        };
    }
    let Ok(start) = start_s.parse::<u64>() else {
        return Want::Malformed;
    };
    if end_s.is_empty() {
        return Want::From(start);
    }
    let Ok(end) = end_s.parse::<u64>() else {
        return Want::Malformed;
    };
    if start > end {
        Want::Malformed
    } else {
        Want::Closed { start, end }
    }
}

fn serve_live(path: &Path, range_header: Option<&str>) -> Result<ProtocolResponse, String> {
    match classify_range(range_header) {
        Want::Malformed => Ok(unsatisfiable(known_total(path))),
        Want::Suffix(len) => serve_suffix(path, len),
        Want::Full => serve_from(path, 0, None),
        Want::From(start) => serve_from(path, start, None),
        Want::Closed { start, end } => serve_from(path, start, Some(end)),
    }
}

fn known_total(path: &Path) -> Option<u64> {
    let slot = lookup(path)?;
    let state = lock_state(&slot);
    (state.phase == Phase::Done).then_some(state.committed)
}

fn serve_suffix(path: &Path, len: u64) -> Result<ProtocolResponse, String> {
    let (committed, phase, message) = wait_until(path, u64::MAX)?;
    if phase != Phase::Done {
        return Ok(terminal(phase, &message));
    }
    if committed == 0 {
        return Ok(unsatisfiable(Some(0)));
    }
    let start = committed.saturating_sub(len);
    serve_slice(path, start, committed - 1, committed, phase)
}

fn serve_from(
    path: &Path,
    start: u64,
    closed_end: Option<u64>,
) -> Result<ProtocolResponse, String> {
    let (committed, phase, message) = wait_until(path, start)?;
    if start >= committed {
        return Ok(if phase == Phase::Done {
            unsatisfiable(Some(committed))
        } else {
            terminal(phase, &message)
        });
    }
    let end = closed_end
        .unwrap_or(committed.saturating_sub(1))
        .min(committed.saturating_sub(1));
    if start > end {
        return Ok(unsatisfiable((phase == Phase::Done).then_some(committed)));
    }
    serve_slice(path, start, end, committed, phase)
}

fn serve_slice(
    path: &Path,
    start: u64,
    end: u64,
    committed: u64,
    phase: Phase,
) -> Result<ProtocolResponse, String> {
    let mut take = end.saturating_sub(start).saturating_add(1);
    take = take.min(committed.saturating_sub(start));
    take = take.min(GROW_CHUNK);
    let bytes = read_file(path, start, take)?;
    let actual_end = start + bytes.len() as u64;
    let reaches = actual_end == committed;
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
    let last = actual_end.saturating_sub(1);
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

fn unsatisfiable(total: Option<u64>) -> ProtocolResponse {
    let total = total
        .map(|len| len.to_string())
        .unwrap_or_else(|| "*".into());
    (
        416,
        vec![
            ("Content-Range".into(), format!("bytes */{total}")),
            ("Content-Type".into(), "text/plain".into()),
            ("Content-Length".into(), "0".into()),
        ],
        Vec::new(),
    )
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
    let len = usize::try_from(len).map_err(|_| "区间过大".to_string())?;
    let mut file = File::open(path).map_err(|e| format!("打开文件失败: {e}"))?;
    file.seek(SeekFrom::Start(start))
        .map_err(|e| format!("定位失败: {e}"))?;
    let mut buf = vec![0u8; len];
    file.read_exact(&mut buf)
        .map_err(|e| format!("读取文件失败: {e}"))?;
    Ok(buf)
}

fn close(path: &Path, phase: Phase, message: String, final_len: Option<u64>) {
    let Some(slot) = lookup(path) else {
        return;
    };
    let mut state = lock_state(&slot);
    if state.phase == Phase::Open {
        if let Some(len) = final_len {
            state.committed = len;
        }
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
        let base = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.qa-b-scratch")
            });
        let dir = base
            .join("grow-unit")
            .join(format!("{name}-{}", std::process::id()));
        let dir = if dir.is_absolute() {
            dir
        } else {
            std::env::current_dir().unwrap().join(dir)
        };
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
    fn a_finished_file_serves_ranges_without_intermediate_commits() {
        let path = scratch("finished-without-prefix");
        open(&path).unwrap();
        std::fs::write(&path, b"whole-file").unwrap();
        finish(&path);

        let response = serve(&encoded(&path), Some("bytes=0-1")).unwrap();
        assert_eq!(response.0, 206);
        assert_eq!(response.2, b"wh");
        assert_eq!(header(&response.1, "Content-Range"), Some("bytes 0-1/10"));
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

    #[test]
    fn suffix_over_range_and_malformed_ranges_keep_their_status() {
        let path = scratch("range");
        std::fs::write(&path, b"abcdef").unwrap();
        open(&path).unwrap();
        commit(&path, 6);
        finish(&path);
        let suffix = serve(&encoded(&path), Some("bytes=-2")).unwrap();
        assert_eq!(suffix.0, 206);
        assert_eq!(suffix.2, b"ef");
        assert_eq!(header(&suffix.1, "Content-Range"), Some("bytes 4-5/6"));
        let past = serve(&encoded(&path), Some("bytes=10-")).unwrap();
        assert_eq!(past.0, 416);
        assert_eq!(header(&past.1, "Content-Range"), Some("bytes */6"));
        let bad = serve(&encoded(&path), Some("bytes=abc-def")).unwrap();
        assert_eq!(bad.0, 416);
        assert!(header(&bad.1, "Content-Range").is_some());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn empty_and_growing_suffix_do_not_overflow() {
        let path = scratch("empty");
        std::fs::write(&path, b"").unwrap();
        open(&path).unwrap();
        finish(&path);
        let empty = serve(&encoded(&path), Some("bytes=-2")).unwrap();
        assert_eq!(empty.0, 416);
        assert_eq!(header(&empty.1, "Content-Range"), Some("bytes */0"));

        let growing = scratch("suffix-wait");
        std::fs::write(&growing, b"abcdef").unwrap();
        open(&growing).unwrap();
        commit(&growing, 6);
        let path2 = growing.clone();
        let reader = thread::spawn(move || serve(&encoded(&path2), Some("bytes=-2")).unwrap());
        std::thread::sleep(std::time::Duration::from_millis(30));
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&growing)
            .unwrap();
        use std::io::Write;
        file.write_all(b"XYZ").unwrap();
        commit(&growing, 9);
        finish(&growing);
        let response = reader.join().unwrap();
        assert_eq!(response.0, 206);
        assert_eq!(response.2, b"YZ");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        let _ = std::fs::remove_dir_all(growing.parent().unwrap());
    }
}
