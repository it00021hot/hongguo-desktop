fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").expect("target OS");
    if target != "macos" && target != "ios" {
        return;
    }
    println!("cargo:rerun-if-changed=src/output.m");
    println!("cargo:rerun-if-changed=../hongguo-audio-api/native/output.h");
    cc::Build::new()
        .file("src/output.m")
        .include("../hongguo-audio-api/native")
        .flag("-fobjc-arc")
        .std("c11")
        .compile("hongguo_audio_apple");
    for framework in ["AudioToolbox", "CoreAudio", "Foundation"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
    if target == "ios" {
        println!("cargo:rustc-link-lib=framework=AVFAudio");
    }
}
