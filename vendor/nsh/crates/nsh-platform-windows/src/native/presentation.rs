/// A native component's canvas was submitted to DXGI. This is not photon time.
#[derive(Clone, Copy, Debug)]
pub struct NativePresentation {
    /// CPU time after the window's successful Present/Present1 call.
    pub submitted_at: std::time::Instant,
    /// DXGI's count for this submission, if available.
    pub present_count: Option<u32>,
    /// Raw DXGI history. PresentCount can refer to an earlier submitted frame;
    /// SyncQPCTime is a vblank reference, not this frame's scanout timestamp.
    pub statistics: Option<windows::Win32::Graphics::Dxgi::DXGI_FRAME_STATISTICS>,
}
