use std::{env, path::PathBuf, process::Command};

const IOS_MINIMUM_VERSION: &str = "16.0";

fn main() {
    println!("cargo:rerun-if-changed=vendor/vvdec");
    let target = env::var("TARGET").expect("Cargo sets TARGET");
    let mut config = cmake::Config::new("vendor/vvdec");
    config
        .profile("Release")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("VVDEC_LIBRARY_ONLY", "ON")
        .define("VVDEC_TOPLEVEL_OUTPUT_DIRS", "OFF")
        .define("VVDEC_ENABLE_UNSTABLE_API", "OFF")
        .define("VVDEC_ENABLE_BITSTREAM_DOWNLOAD", "OFF")
        .define("VVDEC_ENABLE_LINK_TIME_OPT", "OFF");
    if target.contains("msvc") {
        let features = env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default();
        config.static_crt(features.split(',').any(|feature| feature == "crt-static"));
    }
    let ios = target.contains("apple-ios").then(|| ios_toolchain(&target));
    if let Some((sdk, _)) = &ios {
        config
            .define("CMAKE_OSX_SYSROOT", sdk)
            .define("CMAKE_OSX_ARCHITECTURES", "arm64")
            .define("CMAKE_OSX_DEPLOYMENT_TARGET", IOS_MINIMUM_VERSION);
    }
    let output = config.build();
    println!("cargo:rustc-link-search=native={}/lib", output.display());
    println!("cargo:rustc-link-lib=static=vvdec");
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    } else if target.contains("android") {
        android_runtime_search();
        println!("cargo:rustc-link-lib=static=c++_static");
        println!("cargo:rustc-link-lib=static=c++abi");
        println!("cargo:rustc-link-lib=atomic");
        println!("cargo:rustc-link-lib=m");
        println!("cargo:rustc-link-lib=dl");
    } else if !target.contains("msvc") {
        println!("cargo:rustc-link-lib=stdc++");
    }
    let include = output.join("include");
    let mut builder = bindgen::Builder::default()
        .header(include.join("vvdec/vvdec.h").to_string_lossy())
        .clang_arg(format!("-I{}", include.display()))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .allowlist_type("vvdec.*")
        .allowlist_function("vvdec_.*")
        .allowlist_var("VVDEC.*")
        .blocklist_type("vvdecSEIDependentRapIndication")
        .layout_tests(false);
    if let Some((sdk, clang_target)) = ios {
        builder = builder
            .clang_arg(format!("--target={clang_target}"))
            .clang_arg("-isysroot")
            .clang_arg(sdk);
    }
    let bindings = builder
        .generate()
        .expect("VVdeC C headers must generate bindings");
    bindings
        .write_to_file(
            PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR")).join("bindings.rs"),
        )
        .expect("write VVdeC bindings");
}

fn ios_toolchain(target: &str) -> (String, String) {
    let simulator = target.ends_with("-sim");
    let sdk = if simulator {
        "iphonesimulator"
    } else {
        "iphoneos"
    };
    let result = Command::new("xcrun")
        .args(["--sdk", sdk, "--show-sdk-path"])
        .output()
        .expect("iOS VVdeC requires Xcode xcrun");
    assert!(
        result.status.success(),
        "xcrun could not locate {sdk}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let path = String::from_utf8(result.stdout)
        .expect("Xcode SDK path must be UTF-8")
        .trim()
        .to_owned();
    assert!(
        PathBuf::from(&path).is_dir(),
        "iOS SDK does not exist: {path}"
    );
    (
        path,
        format!(
            "arm64-apple-ios{IOS_MINIMUM_VERSION}{}",
            if simulator { "-simulator" } else { "" }
        ),
    )
}

fn android_runtime_search() {
    for name in ["NDK_HOME", "ANDROID_NDK_ROOT", "ANDROID_NDK_HOME"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let ndk = ["NDK_HOME", "ANDROID_NDK_ROOT", "ANDROID_NDK_HOME"]
        .into_iter()
        .find_map(env::var_os)
        .map(PathBuf::from)
        .expect("Android VVdeC requires NDK_HOME or ANDROID_NDK_ROOT");
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
    for library in ["libc++_static.a", "libc++abi.a"] {
        assert!(
            directory.join(library).is_file(),
            "NDK C++ runtime missing: {}",
            directory.join(library).display()
        );
    }
    let runtime =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR")).join("cxx-runtime");
    std::fs::create_dir_all(&runtime).expect("create private Android C++ runtime directory");
    for library in ["libc++_static.a", "libc++abi.a"] {
        let source = directory.join(library);
        println!("cargo:rerun-if-changed={}", source.display());
        std::fs::copy(source, runtime.join(library)).expect("copy target NDK C++ runtime archive");
    }
    // A broad -L of the NDK root would shadow API-specific libc.so with libc.a.
    println!("cargo:rustc-link-search=native={}", runtime.display());
}
