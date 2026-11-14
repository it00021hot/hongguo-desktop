use sha2::{Digest, Sha256};
use std::{
    env,
    path::{Component, PathBuf},
};

const VERSION: &str = "9.0.2";
const SOURCE_SHA256: &str = "8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e";

fn main() {
    println!("cargo:rerun-if-env-changed=HONGGUO_FFMPEG_PREFIX");
    let target = env::var("TARGET").expect("Cargo TARGET");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let prefix = env::var_os("HONGGUO_FFMPEG_PREFIX")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            root.join("../../target/native-media")
                .join(&target)
                .join(format!("ffmpeg-{VERSION}"))
        });
    let manifest = prefix.join("manifest.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let data = std::fs::read(&manifest).unwrap_or_else(|error| {
        panic!("Pinned FFmpeg missing at {}: {error}. Run node scripts/native-media.mjs build {target}", prefix.display())
    });
    let metadata: serde_json::Value =
        serde_json::from_slice(&data).expect("native media manifest JSON");
    assert_eq!(
        metadata["schemaVersion"].as_u64(),
        Some(1),
        "FFmpeg manifest schema mismatch"
    );
    assert_eq!(
        metadata["source"]["version"].as_str(),
        Some(VERSION),
        "FFmpeg version mismatch"
    );
    assert_eq!(
        metadata["source"]["sha256"].as_str(),
        Some(SOURCE_SHA256),
        "FFmpeg source mismatch"
    );
    assert_eq!(
        metadata["target"]["triple"].as_str(),
        Some(target.as_str()),
        "FFmpeg target mismatch"
    );
    let actual_prefix = prefix.canonicalize().expect("native media prefix");
    for artifact in metadata["artifacts"]
        .as_array()
        .expect("native media artifact list")
    {
        let path = artifact["path"]
            .as_str()
            .expect("native media artifact path");
        let relative = std::path::Path::new(path);
        assert!(
            relative
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
            "native media path escapes its prefix"
        );
        let full = prefix
            .join(relative)
            .canonicalize()
            .expect("native media artifact");
        assert!(
            full.starts_with(&actual_prefix),
            "native media symlink escapes its prefix"
        );
        let bytes = std::fs::read(full).expect("read native media artifact");
        let hash = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(
            artifact["sha256"].as_str(),
            Some(hash.as_str()),
            "native media artifact changed: {path}"
        );
    }
    let mut build = cc::Build::new();
    build.include(prefix.join("include")).include("native");
    for source in ["native/video.c", "native/audio.c"] {
        println!("cargo:rerun-if-changed={source}");
        build.file(source);
    }
    println!("cargo:rerun-if-changed=native/codec.h");
    build.std("c11").warnings(true).compile("hongguo_codec");
    println!(
        "cargo:rustc-link-search=native={}",
        prefix.join("lib").display()
    );
    let mobile = target.contains("ios") || target.contains("android");
    for library in ["avfilter", "swresample", "avcodec", "avutil"] {
        println!(
            "cargo:rustc-link-lib={}={library}",
            if mobile { "static:-bundle" } else { "dylib" }
        );
    }
    if target.contains("apple") {
        for framework in [
            "VideoToolbox",
            "CoreMedia",
            "CoreVideo",
            "CoreFoundation",
            "AudioToolbox",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
        println!("cargo:rustc-link-lib=z");
        println!("cargo:rustc-link-lib=iconv");
    } else if target.contains("android") {
        for library in ["m", "log", "android", "mediandk"] {
            println!("cargo:rustc-link-lib={library}");
        }
    }
}
