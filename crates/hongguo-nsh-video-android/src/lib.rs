#![cfg(target_os = "android")]

mod bridge;
mod controller;
mod convert;
mod ffi;
mod frames;
mod reader;
mod render;
mod surface;
mod worker;

pub use ffi::{create, destroy, detached, event, frame, latest_state, seek, set_state, viewport};
pub use frames::{FrameLease, SurfaceFrames, VideoError};
pub use reader::{reader_close, reader_open, reader_read, reader_size};
