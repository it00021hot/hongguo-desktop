#[cfg(target_os = "macos")]
#[path = "playback_probe/app.rs"]
mod app;

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args()
        .nth(1)
        .is_some_and(|arg| arg == "--help" || arg == "-h")
    {
        println!("{}", app::USAGE);
        return Ok(());
    }
    app::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("playback_probe requires macOS");
}
