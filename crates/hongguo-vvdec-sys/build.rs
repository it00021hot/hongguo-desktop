use std::{env, path::PathBuf};

#[path = "../../scripts/native-cxx-build.rs"]
mod native_cxx;
#[cfg(not(feature = "source-build"))]
#[path = "../../scripts/native-sdk-build.rs"]
mod native_sdk;
#[cfg(feature = "source-build")]
mod source_build;

fn main() {
    let target = env::var("TARGET").expect("Cargo sets TARGET");
    #[cfg(feature = "source-build")]
    source_build::build(&target);
    #[cfg(not(feature = "source-build"))]
    use_sdk(&target);
    native_cxx::link(&target);
}

#[cfg(not(feature = "source-build"))]
fn use_sdk(target: &str) {
    let directory =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets manifest directory"));
    let root = native_sdk::require_sdk(&directory, target).join("vvdec-3.2.1");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    std::fs::copy(root.join("bindings.rs"), output.join("bindings.rs"))
        .expect("copy verified VVdeC bindings");
    println!(
        "cargo:rustc-link-search=native={}",
        root.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=vvdec");
}
