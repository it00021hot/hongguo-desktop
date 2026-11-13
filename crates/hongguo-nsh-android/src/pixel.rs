//! JNI exports remain in the final cdylib; the platform crate owns implementation.
use hongguo_nsh_video_android as video;
use nsh_android_runtime::jni::{
    JNIEnv,
    objects::{JClass, JObject, JString},
    sys::{jboolean, jint, jlong, jstring},
};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn boundary<T: Default>(
    env: &mut JNIEnv<'_>,
    call: impl FnOnce(&mut JNIEnv<'_>) -> Result<T, String>,
) -> T {
    match catch_unwind(AssertUnwindSafe(|| call(env))) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            let _ = env.throw_new("java/lang/IllegalArgumentException", error);
            T::default()
        }
        Err(_) => {
            let _ = env.throw_new(
                "java/lang/IllegalStateException",
                "Native video controller failed",
            );
            T::default()
        }
    }
}

fn io_boundary<T: Default>(
    env: &mut JNIEnv<'_>,
    call: impl FnOnce(&mut JNIEnv<'_>) -> Result<T, String>,
) -> T {
    match catch_unwind(AssertUnwindSafe(|| call(env))) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            let _ = env.throw_new("java/io/IOException", error);
            T::default()
        }
        Err(_) => {
            let _ = env.throw_new("java/io/IOException", "Native media session failed");
            T::default()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_PixelDirect_nativeOpen(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    generation: jlong,
    url: JString<'_>,
) {
    io_boundary(&mut env, |env| {
        let url: String = env
            .get_string(&url)
            .map_err(|error| error.to_string())?
            .into();
        video::direct_open(handle, generation, &url)
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_PixelDirect_nativeClock(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    generation: jlong,
    position: jlong,
    seeking: jboolean,
) {
    io_boundary(&mut env, |_| {
        video::direct_clock(handle, generation, position, seeking != 0);
        Ok(())
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_PixelDirect_nativeSeek(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    generation: jlong,
    position: jlong,
) -> jlong {
    io_boundary(&mut env, |_| {
        video::direct_seek(handle, generation, position)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_PixelDirect_nativeClose(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    generation: jlong,
) {
    io_boundary(&mut env, |_| {
        video::direct_close(handle, generation);
        Ok(())
    });
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_PixelVideoComponent_nativeCreate(
    mut env: JNIEnv<'_>,
    owner: JObject<'_>,
) -> jlong {
    boundary(&mut env, |env| video::create(env, owner))
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_PixelVideoComponent_nativeSetState(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    json: JString<'_>,
) {
    boundary(&mut env, |env| video::set_state(env, handle, json));
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_PixelVideoComponent_nativeLatestState(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    generation: jlong,
) -> jstring {
    boundary(&mut env, |env| {
        video::latest_state(handle, generation)?
            .map(|state| {
                env.new_string(state)
                    .map(JString::into_raw)
                    .map_err(|error| error.to_string())
            })
            .transpose()
            .map(|state| state.unwrap_or(std::ptr::null_mut()))
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_PixelVideoComponent_nativeSeek(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    json: JString<'_>,
) {
    boundary(&mut env, |env| video::seek(env, handle, json));
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_PixelVideoComponent_nativeViewport(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    surface: JObject<'_>,
    width: jint,
    height: jint,
) {
    boundary(&mut env, |env| {
        video::viewport(env, handle, surface, width, height)
    });
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_PixelVideoComponent_nativeFrame(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    generation: jlong,
) {
    boundary(&mut env, |_| {
        video::frame(handle, generation);
        Ok(())
    });
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_PixelVideoComponent_nativeDetached(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
) {
    boundary(&mut env, |_| {
        video::detached(handle, token);
        Ok(())
    });
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_PixelVideoComponent_nativeEvent(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    generation: jlong,
    json: JString<'_>,
) {
    boundary(&mut env, |env| video::event(env, handle, generation, json));
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_PixelVideoComponent_nativeDestroy(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
) {
    boundary(&mut env, |_| {
        video::destroy(handle);
        Ok(())
    });
}
