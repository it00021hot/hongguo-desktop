#![cfg(any(target_os = "macos", target_os = "ios"))]

mod control;
mod controller;
mod display_clock;
mod frame;
mod native;
mod native_audio;
mod output;
mod render;
mod surface;
mod worker;

pub use controller::PlayerController;
pub use display_clock::DisplayClock;
use frame::ImportedFrame;
use output::AudioOutput;
pub use output::OutputError;
pub use surface::MetalSurface;
