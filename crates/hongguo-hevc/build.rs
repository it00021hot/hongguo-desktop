use std::{env, path::PathBuf};

#[path = "../../scripts/native-cxx-build.rs"]
mod native_cxx;
#[cfg(not(feature = "source-build"))]
#[path = "../../scripts/native-sdk-build.rs"]
mod native_sdk;

fn main() {
    let target = env::var("TARGET").expect("Cargo sets TARGET");
    #[cfg(not(feature = "source-build"))]
    let root = {
        let directory =
            PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets crate root"));
        native_sdk::require_sdk(&directory, &target).join("libde265-1.1.3")
    };
    #[cfg(feature = "source-build")]
    let root = {
        println!("cargo:rerun-if-env-changed=HONGGUO_HEVC_ROOT");
        PathBuf::from(env::var_os("HONGGUO_HEVC_ROOT").expect(
            "source-build requires the native-codec-build.mjs output via HONGGUO_HEVC_ROOT",
        ))
    };
    assert!(
        root.join("include/libde265/de265.h").is_file(),
        "HEVC SDK header missing"
    );
    println!(
        "cargo:rustc-link-search=native={}",
        root.join("lib").display()
    );
    println!(
        "cargo:rustc-link-lib={}de265",
        if target.contains("android") || target.contains("apple-ios") {
            "static:-bundle="
        } else {
            "dylib="
        }
    );
    native_cxx::link(&target);
}
