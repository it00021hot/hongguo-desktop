fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("android") {
        return;
    }
    println!("cargo:rerun-if-changed=src/output.c");
    println!("cargo:rerun-if-changed=../hongguo-audio-api/native/output.h");
    cc::Build::new()
        .file("src/output.c")
        .include("../hongguo-audio-api/native")
        .std("c11")
        .compile("hongguo_audio_android");
    println!("cargo:rustc-link-lib=aaudio");
}
