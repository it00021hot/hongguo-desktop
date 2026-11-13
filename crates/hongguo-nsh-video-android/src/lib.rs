#![cfg(target_os = "android")]

mod bridge;
mod controller;
mod convert;
mod direct;
mod direct_texture;
mod ffi;
mod frames;
mod pixel_bytes;
mod render;
mod surface;
mod worker;

pub use direct::{direct_clock, direct_close, direct_open, direct_seek};
pub use ffi::{create, destroy, detached, event, frame, latest_state, seek, set_state, viewport};
pub use frames::{FrameLease, SurfaceFrames, VideoError};
