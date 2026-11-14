#[cfg(target_os = "windows")]
fn main() -> Result<(), String> {
    hongguo_nsh_video_windows::probe_hevc_hardware()
}

#[cfg(not(target_os = "windows"))]
fn main() {
    panic!("hevc_d3d11_probe requires Windows hardware");
}
