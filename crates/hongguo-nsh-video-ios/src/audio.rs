//! Playback audio is an application-owned main-thread lease, shared by all pixel
//! components. The final pause/end/drop restores the category that preceded them.
use dispatch2::DispatchQueue;
use hongguo_nsh_video_apple::PlayerController;
use objc2::{
    MainThreadMarker, msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Bool},
};
use objc2_foundation::NSString;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

#[link(name = "AVFAudio", kind = "framework")]
unsafe extern "C" {
    static AVAudioSessionCategoryPlayback: &'static NSString;
    static AVAudioSessionModeSpokenAudio: &'static NSString;
}

#[derive(Clone)]
struct Category {
    name: Retained<NSString>,
    mode: Retained<NSString>,
    options: usize,
}
#[derive(Default)]
struct Sessions {
    owners: BTreeMap<u64, String>,
    previous: Option<Category>,
}
thread_local! { static SESSIONS: RefCell<Sessions> = RefCell::default(); }
static NEXT_KEY: AtomicU64 = AtomicU64::new(1);

pub(super) type ControlSlot = Arc<Mutex<Weak<PlayerController>>>;
pub(super) struct Lease {
    key: u64,
}

impl Lease {
    pub fn new() -> Self {
        Self {
            key: NEXT_KEY.fetch_add(1, Ordering::Relaxed),
        }
    }
    pub fn key(&self) -> u64 {
        self.key
    }

    pub fn play(&self, source: &str) -> Result<(), String> {
        main_thread()?;
        SESSIONS.with(|sessions| {
            let mut sessions = sessions.borrow_mut();
            if sessions
                .owners
                .get(&self.key)
                .is_some_and(|current| current == source)
            {
                return Ok(());
            }
            if sessions.owners.is_empty() {
                let session = shared()?;
                let before = sessions
                    .previous
                    .clone()
                    .unwrap_or_else(|| category(&session));
                // SAFETY: AVFAudio's public exported category/mode constants. This
                // exactly preserves the previous NSH AudioSession::Playback mapping.
                let wanted = unsafe {
                    Category {
                        name: Retained::retain(
                            std::ptr::from_ref(AVAudioSessionCategoryPlayback).cast_mut(),
                        )
                        .expect("system category constant"),
                        mode: Retained::retain(
                            std::ptr::from_ref(AVAudioSessionModeSpokenAudio).cast_mut(),
                        )
                        .expect("system mode constant"),
                        options: 0,
                    }
                };
                set_category(&session, &wanted)?;
                if let Err(error) = set_active(&session, true) {
                    if let Err(restore) = set_category(&session, &before) {
                        sessions.previous = Some(before);
                        return Err(format!(
                            "{error}; restoring previous audio category also failed: {restore}"
                        ));
                    }
                    return Err(error);
                }
                sessions.previous = Some(before);
            }
            sessions.owners.insert(self.key, source.to_owned());
            Ok(())
        })
    }

    pub fn release(&self) -> Result<(), String> {
        release(self.key, None)
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Err(error) = self.release() {
            log::warn!("could not restore playback audio session on drop: {error}");
        }
    }
}

/// Terminal events arrive on the render worker. Only the UIKit queue touches the
/// session; stale source ends and ends superseded by a real seek/play cannot release it.
pub(super) fn terminal(key: u64, source: String, controller: ControlSlot) {
    DispatchQueue::main().exec_async(move || {
        let control = controller
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .upgrade();
        if control
            .as_ref()
            .is_some_and(|control| control.source_is_playing(&source))
        {
            return;
        }
        if let Err(error) = release(key, Some(&source)) {
            log::warn!("could not restore playback audio session after native end/error: {error}");
        }
    });
}

fn release(key: u64, source: Option<&str>) -> Result<(), String> {
    main_thread()?;
    SESSIONS.with(|sessions| {
        let mut sessions = sessions.borrow_mut();
        if source.is_some_and(|source| {
            sessions
                .owners
                .get(&key)
                .is_none_or(|current| current != source)
        }) {
            return Ok(());
        }
        sessions.owners.remove(&key);
        if !sessions.owners.is_empty() {
            return Ok(());
        }
        let Some(before) = sessions.previous.clone() else {
            return Ok(());
        };
        let session = shared()?;
        let inactive = set_active(&session, false);
        let restored = set_category(&session, &before);
        match (inactive, restored) {
            (Ok(()), Ok(())) => {
                sessions.previous = None;
                Ok(())
            }
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    })
}

fn main_thread() -> Result<(), String> {
    MainThreadMarker::new()
        .map(|_| ())
        .ok_or_else(|| "audio session must be changed on the UIKit thread".into())
}

fn shared() -> Result<Retained<AnyObject>, String> {
    let class = AnyClass::get(c"AVAudioSession").ok_or("AVAudioSession is unavailable")?;
    // SAFETY: Public singleton getter on the linked AVFAudio framework class.
    Ok(unsafe { msg_send![class, sharedInstance] })
}

fn category(session: &AnyObject) -> Category {
    // SAFETY: Read-only public getters on this retained AVAudioSession.
    unsafe {
        Category {
            name: msg_send![session, category],
            mode: msg_send![session, mode],
            options: msg_send![session, categoryOptions],
        }
    }
}

fn set_category(session: &AnyObject, category: &Category) -> Result<(), String> {
    let mut error: *mut AnyObject = std::ptr::null_mut();
    // SAFETY: The category/mode strings are retained, options are the SDK's NSUInteger bitmask, and NSError** points to a local.
    let result: Bool = unsafe {
        msg_send![session, setCategory: &*category.name, mode: &*category.mode, options: category.options, error: &mut error]
    };
    answer(result, error, "cannot set playback audio category")
}

fn set_active(session: &AnyObject, active: bool) -> Result<(), String> {
    const NOTIFY_OTHERS_ON_DEACTIVATION: usize = 1;
    let mut error: *mut AnyObject = std::ptr::null_mut();
    // SAFETY: These are the public APIs and option value in AVAudioSessionTypes.h,
    // invoked only on the owning UIKit thread.
    let result: Bool = unsafe {
        msg_send![session, setActive: Bool::new(active),
        withOptions: if active { 0usize } else { NOTIFY_OTHERS_ON_DEACTIVATION }, error: &mut error]
    };
    answer(
        result,
        error,
        "cannot activate/deactivate playback audio session",
    )
}

fn answer(result: Bool, error: *mut AnyObject, fallback: &str) -> Result<(), String> {
    if result.as_bool() {
        return Ok(());
    }
    // SAFETY: The immediately preceding synchronous API initializes NSError** to
    // either null or a live autoreleased NSError on this same UIKit stack.
    let Some(error) = (unsafe { error.as_ref() }) else {
        return Err(fallback.into());
    };
    // SAFETY: A non-null NSError** from the immediately preceding synchronous public API.
    let detail: Retained<NSString> = unsafe { msg_send![error, localizedDescription] };
    Err(detail.to_string())
}
