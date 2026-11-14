use std::{env, path::PathBuf};

pub fn link(target: &str) {
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    } else if target.contains("android") {
        android_search();
        for library in ["c++_static", "c++abi"] {
            println!("cargo:rustc-link-lib=static={library}");
        }
        for library in ["atomic", "m", "dl"] {
            println!("cargo:rustc-link-lib={library}");
        }
    } else if !target.contains("msvc") {
        println!("cargo:rustc-link-lib=stdc++");
    }
}

fn android_search() {
    for name in ["NDK_HOME", "ANDROID_NDK_ROOT", "ANDROID_NDK_HOME"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let ndk = ["NDK_HOME", "ANDROID_NDK_ROOT", "ANDROID_NDK_HOME"]
        .into_iter()
        .find_map(env::var_os)
        .map(PathBuf::from)
        .expect("Android native codecs require the standard NDK_HOME or ANDROID_NDK_ROOT");
    let host = match std::env::consts::OS {
        "macos" => "darwin-x86_64",
        "linux" => "linux-x86_64",
        "windows" => "windows-x86_64",
        other => panic!("Unsupported Android NDK build host: {other}"),
    };
    let directory = ndk
        .join("toolchains/llvm/prebuilt")
        .join(host)
        .join("sysroot/usr/lib/aarch64-linux-android");
    let runtime =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR")).join("cxx-runtime");
    std::fs::create_dir_all(&runtime).expect("create private target C++ runtime directory");
    for library in ["libc++_static.a", "libc++abi.a"] {
        let source = directory.join(library);
        assert!(
            source.is_file(),
            "NDK C++ runtime missing: {}",
            source.display()
        );
        println!("cargo:rerun-if-changed={}", source.display());
        std::fs::copy(&source, runtime.join(library)).expect("copy target NDK C++ runtime");
    }
    // A broad NDK library path would shadow the API-specific libc.so with libc.a.
    println!("cargo:rustc-link-search=native={}", runtime.display());
}
