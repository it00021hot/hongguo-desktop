#![cfg(target_os = "windows")]

mod component;
mod controller;
mod decoder;
mod draw;
mod gpu_worker;
mod output;
mod render;
mod shared;
mod state;

pub use component::register;
pub use decoder::{DecoderEvent, VideoDecoder};
pub use draw::{RawFrame, drawing_device};
pub use output::DisplayedFrame;
pub use render::{CompletedFrame, GpuRenderer};
pub use shared::{FrameLease, SharedFrames, VideoError};
