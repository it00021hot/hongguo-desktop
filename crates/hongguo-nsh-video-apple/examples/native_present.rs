#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    hongguo_nsh_video_apple::testing::native_present_probe()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("native_present probe requires macOS");
}
