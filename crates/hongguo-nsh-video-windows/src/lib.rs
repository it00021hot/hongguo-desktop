#![cfg(target_os = "windows")]

mod component;
mod controller;
mod decoder;
mod direct;
mod draw;
mod gpu_worker;
mod output;
mod presentation;
mod render;
mod seek_gate;
mod shared;
mod state;

pub use component::register;
pub use decoder::{DecoderEvent, VideoDecoder};
pub use draw::{RawFrame, drawing_device};
pub use output::DisplayedFrame;
pub use render::{CompletedFrame, GpuRenderer};
pub use shared::{FrameLease, SharedFrames, VideoError};
