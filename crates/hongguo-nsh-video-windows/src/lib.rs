#![cfg(target_os = "windows")]

mod component;
mod controller;
mod conversion;
mod device;
mod direct;
mod draw;
mod gpu_worker;
mod hevc;
mod input;
mod native_audio;
mod output;
mod presentation;
mod render;
mod seek_gate;
mod shared;
mod state;

pub use component::register;
pub use draw::{RawFrame, drawing_device};
pub use output::DisplayedFrame;
pub use render::{CompletedFrame, GpuRenderer};
pub use shared::{FrameLease, SharedFrames, VideoError};

#[cfg(feature = "test-support")]
pub use hevc::probe::run as probe_hevc_hardware;
