#![cfg(any(target_os = "macos", target_os = "ios"))]

mod control;
mod controller;
#[cfg(feature = "test-support")]
mod diagnostics;
mod display_clock;
mod frame;
mod native;
mod output;
mod render;
mod surface;
mod worker;

pub use controller::PlayerController;
#[cfg(feature = "test-support")]
pub use diagnostics::PresentationObservation;
pub use display_clock::DisplayClock;
use frame::ImportedFrame;
use output::AudioOutput;
pub use output::OutputError;
pub use surface::MetalSurface;

#[cfg(all(target_os = "macos", feature = "test-support"))]
pub mod testing;
