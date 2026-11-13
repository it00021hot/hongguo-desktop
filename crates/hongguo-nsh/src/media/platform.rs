use super::{cancellation::Cancellation, hls_source};
use hongguo_protocol::grow::{self, GrowthState};
use std::path::Path;

#[derive(Clone, Copy)]
enum GrowingPolicy {
    Hls,
    NativeRead,
    AwaitCompleteFile,
}

/// Apple periodic time observers need advancing media time to confirm a seek.
/// Other native backends have not established a need for this workaround.
pub(crate) fn paused_seek_needs_progress_drive() -> bool {
    cfg!(any(target_os = "macos", target_os = "ios"))
}

fn native_growing_policy() -> GrowingPolicy {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        GrowingPolicy::Hls
    } else if cfg!(target_os = "android") {
        GrowingPolicy::NativeRead
    } else {
        GrowingPolicy::AwaitCompleteFile
    }
}

pub(super) async fn wait_for_playable(path: &Path) -> Result<(), String> {
    if matches!(
        native_growing_policy(),
        GrowingPolicy::Hls | GrowingPolicy::NativeRead
    ) || grow::snapshot(path).is_none()
    {
        return Ok(());
    }
    await_complete(path).await
}

async fn await_complete(path: &Path) -> Result<(), String> {
    let path = path.to_owned();
    let guard = Cancellation::default();
    let cancelled = guard.flag();
    let result = hongguo_service::runtime::spawn_blocking(move || {
        loop {
            let snapshot = hls_source::state(&path).map_err(|error| error.to_string())?;
            if snapshot.state == GrowthState::Done {
                return Ok(());
            }
            grow::wait_for_change(&path, snapshot.committed_bytes, || {
                cancelled.load(std::sync::atomic::Ordering::Acquire)
            })?;
        }
    })
    .await
    .map_err(|error| error.to_string())?;
    drop(guard);
    result
}
