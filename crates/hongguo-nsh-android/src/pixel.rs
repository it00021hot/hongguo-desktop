//! JNI exports remain in the final cdylib; the platform crate owns implementation.
use hongguo_nsh_video_android as video;
use nsh_android_runtime::jni::{
    JNIEnv,
    objects::{JClass, JObject, JString},
    sys::{jboolean, jdouble, jfloat, jint, jlong, jstring},
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
pub extern "system" fn Java_com_hongguo_nsh_PixelDirect_nativeControl(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    generation: jlong,
    playing: jboolean,
    volume: jfloat,
    muted: jboolean,
    rate: jdouble,
    claim_owner: jboolean,
) {
    io_boundary(&mut env, |_| {
        video::direct_control(
            handle,
            generation,
            playing != 0,
            volume,
            muted != 0,
            rate,
            claim_owner != 0,
        )
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_PixelDirect_nativeStatus(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    generation: jlong,
) -> jstring {
    io_boundary(&mut env, |env| {
        env.new_string(video::direct_status(handle, generation)?)
            .map(JString::into_raw)
            .map_err(|error| error.to_string())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_PixelDirect_nativePresented(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    epoch: jlong,
    generation: jlong,
    serial: jlong,
) -> jboolean {
    io_boundary(&mut env, |_| {
        video::direct_presented(handle, epoch, generation, serial).map(u8::from)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_PixelDirect_nativeIsOwner(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    epoch: jlong,
) -> jboolean {
    io_boundary(&mut env, |_| {
        video::direct_is_owner(handle, epoch).map(u8::from)
    })
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
    hdr_peak: jfloat,
    float_output: jboolean,
) -> jlong {
    boundary(&mut env, |env| {
        video::viewport(env, handle, surface, width, height, hdr_peak, float_output)
    })
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
pub extern "system" fn Java_com_hongguo_nsh_PixelVideoComponent_nativeFrameTick(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
) {
    boundary(&mut env, |_| {
        video::frame_tick(handle);
        Ok(())
    });
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
