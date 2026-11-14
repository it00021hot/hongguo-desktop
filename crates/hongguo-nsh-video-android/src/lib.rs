#![cfg(target_os = "android")]

mod bridge;
mod controller;
mod decoder_surface;
mod direct;
mod direct_texture;
mod ffi;
mod gl_state;
mod output_color;
mod pixel_bytes;
mod raw_yuv;
#[path = "native_renderer.rs"]
mod render;
mod surface;
mod surface_geometry;
mod worker;

pub use direct::{
    direct_close, direct_control, direct_is_owner, direct_open, direct_presented, direct_seek,
    direct_status,
};
pub use ffi::{create, destroy, event, frame_tick, latest_state, seek, set_state, viewport};
