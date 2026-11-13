#![cfg(target_os = "android")]
mod pixel;

use nsh_android_runtime::{
    Root,
    jni::{
        JNIEnv,
        objects::{JClass, JString},
    },
};

nsh_android_runtime::ui_sets!(vec![nsh_base::registry(), hongguo_nsh::registry()]);
nsh_android_runtime::main!(Root::of::<hongguo_nsh::App>().app_id("com.hongguo.nsh"));

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hongguo_nsh_MainActivity_initialize(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    path: JString<'_>,
) {
    let result = env
        .get_string(&path)
        .map(String::from)
        .map_err(|error| error.to_string())
        .and_then(|path| {
            hongguo_nsh::configure_mobile_data_root(std::path::Path::new(&path))
                .map_err(|error| error.to_string())
        });
    if let Err(error) = result {
        let _ = env.throw_new("java/lang/IllegalStateException", error);
    }
}
