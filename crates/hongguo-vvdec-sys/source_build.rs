use std::{env, path::PathBuf, process::Command};

const MACOS_INTEL_MINIMUM_VERSION: &str = "10.15";
const MACOS_ARM_MINIMUM_VERSION: &str = "11.0";
const IOS_MINIMUM_VERSION: &str = "16.0";

pub fn build(target: &str) {
    println!("cargo:rerun-if-changed=vendor/vvdec");
    println!("cargo:rerun-if-changed=source_build.rs");
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
    if target.ends_with("apple-darwin") {
        config.define(
            "CMAKE_OSX_DEPLOYMENT_TARGET",
            macos_deployment_target(target),
        );
    }
    let ios = target.contains("apple-ios").then(|| ios_toolchain(target));
    if let Some((sdk, _)) = &ios {
        config
            .define("CMAKE_OSX_SYSROOT", sdk)
            .define("CMAKE_OSX_ARCHITECTURES", "arm64")
            .define("CMAKE_OSX_DEPLOYMENT_TARGET", IOS_MINIMUM_VERSION);
    }
    let output = config.build();
    println!("cargo:rustc-link-search=native={}/lib", output.display());
    println!("cargo:rustc-link-lib=static=vvdec");
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

fn macos_deployment_target(target: &str) -> String {
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    let floor = if target.starts_with("x86_64-") {
        MACOS_INTEL_MINIMUM_VERSION
    } else {
        MACOS_ARM_MINIMUM_VERSION
    };
    let requested = env::var_os("MACOSX_DEPLOYMENT_TARGET")
        .map(|value| {
            value
                .into_string()
                .expect("macOS deployment target must be UTF-8")
        })
        .unwrap_or_else(|| floor.to_owned());
    if deployment_version(&requested) < deployment_version(floor) {
        floor.to_owned()
    } else {
        requested
    }
}

fn deployment_version(value: &str) -> (u16, u16, u16) {
    let parts = value
        .split('.')
        .map(|part| {
            part.parse::<u16>()
                .expect("macOS deployment target must contain numeric version components")
        })
        .collect::<Vec<_>>();
    assert!(
        (1..=3).contains(&parts.len()),
        "invalid macOS deployment target"
    );
    (
        parts[0],
        parts.get(1).copied().unwrap_or(0),
        parts.get(2).copied().unwrap_or(0),
    )
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
