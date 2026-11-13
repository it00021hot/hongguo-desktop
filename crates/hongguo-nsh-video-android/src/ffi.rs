use crate::{
    bridge::{Bridge, error},
    controller::Shared,
};
use hongguo_nsh_video_api::{Seek, VideoEvent, VideoProps};
use jni::{
    JNIEnv,
    objects::{JObject, JString},
    sys::{jint, jlong},
};
use ndk::native_window::NativeWindow;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicI64, Ordering},
    },
};

static NEXT: AtomicI64 = AtomicI64::new(1);
static PLAYERS: OnceLock<Mutex<HashMap<jlong, Arc<Shared>>>> = OnceLock::new();
fn players() -> &'static Mutex<HashMap<jlong, Arc<Shared>>> {
    PLAYERS.get_or_init(Mutex::default)
}
pub(crate) fn player(handle: jlong) -> Option<Arc<Shared>> {
    players()
        .lock()
        .expect("Android video registry")
        .get(&handle)
        .cloned()
}

pub fn create(env: &mut JNIEnv<'_>, owner: JObject<'_>) -> Result<jlong, String> {
    let bridge = Bridge::new(
        Arc::new(env.get_java_vm().map_err(error)?),
        env.new_global_ref(owner).map_err(error)?,
    );
    let controller = Shared::spawn(bridge)?;
    let handle = NEXT.fetch_add(1, Ordering::Relaxed);
    players()
        .lock()
        .expect("Android video registry")
        .insert(handle, controller);
    Ok(handle)
}
pub fn set_state(env: &mut JNIEnv<'_>, handle: jlong, json: JString<'_>) -> Result<(), String> {
    let Some(player) = player(handle) else {
        return Ok(());
    };
    let json: String = env.get_string(&json).map_err(error)?.into();
    player.props(serde_json::from_str::<VideoProps>(&json).map_err(error)?)
}
/// Completion callbacks must observe current controls, not a render worker's older snapshot.
pub fn latest_state(handle: jlong, generation: jlong) -> Result<Option<String>, String> {
    let Some(player) = player(handle) else {
        return Ok(None);
    };
    let props = {
        let mailbox = player.mailbox.lock().expect("Android video mailbox");
        if player.stop.load(Ordering::Acquire)
            || generation as u64 != player.generation.load(Ordering::Acquire)
        {
            return Ok(None);
        }
        mailbox.props.clone()
    };
    serde_json::to_string(&props).map(Some).map_err(error)
}
pub fn seek(env: &mut JNIEnv<'_>, handle: jlong, json: JString<'_>) -> Result<(), String> {
    let Some(player) = player(handle) else {
        return Ok(());
    };
    let json: String = env.get_string(&json).map_err(error)?.into();
    let request: Seek = serde_json::from_str(&json).map_err(error)?;
    let mut mailbox = player.mailbox.lock().expect("Android video mailbox");
    if mailbox
        .props
        .source
        .as_ref()
        .is_some_and(|source| source.id == request.source_id)
    {
        mailbox.seek = Some(request);
    }
    drop(mailbox);
    player.wake();
    Ok(())
}
pub fn viewport(
    env: &mut JNIEnv<'_>,
    handle: jlong,
    surface: JObject<'_>,
    width: jint,
    height: jint,
) -> Result<(), String> {
    let Some(player) = player(handle) else {
        return Ok(());
    };
    let window = if surface.is_null() {
        None
    } else {
        // SAFETY: This JNI local Surface is live for the call. ANativeWindow_fromSurface acquires its own reference.
        Some(
            unsafe { NativeWindow::from_surface(env.get_raw(), surface.as_raw()) }
                .ok_or_else(|| "cannot acquire Android presentation window".to_owned())?,
        )
    };
    let mut mailbox = player.mailbox.lock().expect("Android video mailbox");
    if mailbox.viewport.window != window {
        mailbox.viewport.version = mailbox.viewport.version.wrapping_add(1);
    }
    mailbox.viewport.window = window;
    mailbox.viewport.width = u32::try_from(width).unwrap_or(0);
    mailbox.viewport.height = u32::try_from(height).unwrap_or(0);
    player.revision.fetch_add(1, Ordering::Release);
    drop(mailbox);
    player.wake();
    Ok(())
}
pub fn frame(handle: jlong, generation: jlong) {
    if let Some(player) = player(handle)
        && generation as u64 == player.generation.load(Ordering::Acquire)
    {
        player.frame.store(true, Ordering::Release);
        player.wake();
    }
}
pub fn detached(handle: jlong, token: jlong) {
    if let Some(player) = player(handle) {
        player.detached.store(token as u64, Ordering::Release);
        player.wake();
    }
}
pub fn event(
    env: &mut JNIEnv<'_>,
    handle: jlong,
    generation: jlong,
    json: JString<'_>,
) -> Result<(), String> {
    let Some(player) = player(handle) else {
        return Ok(());
    };
    if generation as u64 != player.generation.load(Ordering::Acquire) {
        return Ok(());
    }
    let json: String = env.get_string(&json).map_err(error)?.into();
    player.event(
        generation as u64,
        serde_json::from_str::<VideoEvent>(&json).map_err(error)?,
    );
    Ok(())
}
pub fn destroy(handle: jlong) {
    let player = players()
        .lock()
        .expect("Android video registry")
        .remove(&handle);
    if let Some(player) = player {
        player.stop.store(true, Ordering::Release);
        player.close_direct();
        player.wake();
    }
}
