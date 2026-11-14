//! Development-only local-file D3D11VA / D3D12VA same-canvas comparison.
#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    match hongguo_nsh_video_windows::audit::run_cli() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("decoder audit failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("decoder_audit requires Windows and its actual GPU/display.");
}
