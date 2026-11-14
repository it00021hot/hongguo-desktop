use crate::bridge::Bridge;
use hongguo_native_clock::{Clock, Control, NativeClock, NativePlayer, Position};
use jni::{
    JNIEnv, JavaVM,
    objects::{GlobalRef, JObject, JValue},
};
use std::sync::Arc;

pub(crate) fn open(bridge: Bridge, url: String) -> Result<Arc<dyn Clock>, String> {
    NativeClock::open(move || {
        let track = bridge.native_audio(&url)?;
        Ok(Player {
            vm: bridge.vm,
            track,
        })
    })
    .map(|clock| clock as Arc<dyn Clock>)
}

struct Player {
    vm: Arc<JavaVM>,
    track: GlobalRef,
}

impl Player {
    fn call<T>(
        &self,
        action: impl FnOnce(&mut JNIEnv<'_>, &JObject<'_>) -> jni::errors::Result<T>,
    ) -> Result<T, String> {
        let mut env = self
            .vm
            .attach_current_thread()
            .map_err(|failure| failure.to_string())?;
        action(&mut env, self.track.as_obj()).map_err(|failure| error(&mut env, failure))
    }
}

impl NativePlayer for Player {
    fn position(&mut self) -> Result<Position, String> {
        self.call(|env, track| {
            let seconds = env.call_method(track, "positionSeconds", "()D", &[])?.d()?;
            let seeking = env.get_field(track, "seeking", "Z")?.z()?;
            Ok(Position {
                seconds,
                ready: !seeking,
                seeking,
                buffering: env.get_field(track, "buffering", "Z")?.z()?,
                ended: env.get_field(track, "ended", "Z")?.z()?,
            })
        })
    }

    fn control(&mut self, control: Control) -> Result<(), String> {
        self.call(|env, track| {
            env.call_method(
                track,
                "control",
                "(ZFZD)V",
                &[
                    JValue::Bool(control.playing.into()),
                    JValue::Float(control.volume),
                    JValue::Bool(control.muted.into()),
                    JValue::Double(control.rate),
                ],
            )?;
            Ok(())
        })
    }

    fn seek(&mut self, seconds: f64) -> Result<(), String> {
        self.call(|env, track| {
            env.call_method(track, "seek", "(D)V", &[JValue::Double(seconds)])?;
            Ok(())
        })
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Err(error) =
            self.call(|env, track| env.call_method(track, "close", "()V", &[]).map(|_| ()))
        {
            log::debug!("[Android video] closing native audio: {error}");
        }
    }
}

pub(crate) fn error(env: &mut JNIEnv<'_>, failure: jni::errors::Error) -> String {
    let cause = env
        .exception_occurred()
        .ok()
        .filter(|exception| !exception.is_null());
    if let Err(clear) = env.exception_clear() {
        return format!("native Android audio: {failure}; clearing Java exception failed: {clear}");
    }
    let detail = cause.and_then(|cause| {
        env.call_method(cause, "toString", "()Ljava/lang/String;", &[])
            .ok()
            .and_then(|value| value.l().ok())
            .and_then(|value| {
                env.get_string(&jni::objects::JString::from(value))
                    .ok()
                    .map(String::from)
            })
    });
    if let Err(clear) = env.exception_clear() {
        return format!(
            "native Android audio: {failure}; clearing diagnostic exception failed: {clear}"
        );
    }
    format!(
        "native Android audio: {failure}{}",
        detail.map_or_else(String::new, |detail| format!(" ({detail})"))
    )
}
