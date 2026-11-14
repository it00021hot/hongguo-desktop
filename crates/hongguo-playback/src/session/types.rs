use hongguo_common::AppResult;
use std::{sync::atomic::AtomicBool, time::Duration};

pub trait ReadAt: Send + Sync + 'static {
    fn length(&self) -> AppResult<u64>;
    fn read_at(
        &self,
        offset: u64,
        length: u64,
        cancelled: &AtomicBool,
        bytes: &mut Vec<u8>,
    ) -> AppResult<()>;
}

#[derive(Clone, Debug)]
pub struct RecoveryLimits {
    pub attempts: u32,
    pub elapsed: Duration,
}

#[derive(Clone, Debug)]
pub struct SessionConfig {
    pub defer_decoder_binding: bool,
    pub start_position_secs: f64,
    pub video_queue_frames: usize,
    pub video_queue_bytes: usize,
    pub video_ahead_secs: f64,
    pub decoder_threads: u32,
    pub rate: f64,
    pub recovery: RecoveryLimits,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            defer_decoder_binding: false,
            start_position_secs: 0.0,
            video_queue_frames: 4,
            video_queue_bytes: 64 * 1024 * 1024,
            video_ahead_secs: 0.75,
            decoder_threads: std::thread::available_parallelism()
                .map_or(1, |count| count.get().min(4) as u32),
            rate: 1.0,
            recovery: RecoveryLimits {
                attempts: 2,
                elapsed: Duration::from_secs(15),
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SessionInfo {
    pub width: u32,
    pub height: u32,
    pub duration_secs: f64,
    pub has_audio: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct SessionPosition {
    pub position_secs: f64,
    pub duration_secs: f64,
    pub ended: bool,
    pub buffering: bool,
    pub video_gap: bool,
}
