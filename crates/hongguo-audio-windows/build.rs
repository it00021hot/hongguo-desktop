fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=src/output.c");
    println!("cargo:rerun-if-changed=src/watcher.c");
    println!("cargo:rerun-if-changed=src/watcher.h");
    println!("cargo:rerun-if-changed=src/guids.c");
    println!("cargo:rerun-if-changed=../hongguo-audio-api/native/output.h");
    cc::Build::new()
        .file("src/output.c")
        .file("src/watcher.c")
        .file("src/guids.c")
        .include("../hongguo-audio-api/native")
        .std("c11")
        .compile("hongguo_audio_windows");
    for library in ["ole32", "uuid", "avrt"] {
        println!("cargo:rustc-link-lib={library}");
    }
}
