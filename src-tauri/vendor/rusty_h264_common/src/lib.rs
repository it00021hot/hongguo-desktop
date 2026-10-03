//! Shared primitives for the `rusty_h264` pure-Rust H.264 codec.
//!
//! # Global allocator
//!
//! The `global-alloc` feature (on by **default**) installs [`rusty_alloc`] as the
//! process-wide allocator so measured and shipped routes share one allocator.
//!
//! `#[global_allocator]` is process-wide (exactly one per program). Downstream
//! crates that declare their own allocator should depend on this crate with
//! `default-features = false` (then re-enable `asm` / other features as needed).
//!
//! This crate is the foundation both the encoder and decoder sit on. It is
//! `#![forbid(unsafe_code)]`: the bit-twiddling core of an H.264 codec is
//! exactly where memory-safety bugs hide in the C implementations, so we keep
//! it provably safe.
//!
//! Modules mirror the concerns shared across `codec/common` in Cisco's
//! openh264:
//! - [`bit_writer`] / [`bit_reader`] — MSB-first bit packing + Exp-Golomb.
//! - [`nal`] — NAL units, Annex-B framing, RBSP emulation prevention.
//! - [`types`] — shared enums and the raw YUV frame container.
//!
//! The shipped build is `#![forbid(unsafe_code)]`. The `profile` feature (a
//! measurement-only dev build, never shipped) relaxes this to unlock the `rdtsc`
//! timer in [`prof`]; that is the *only* unsafe in the crate and only under `profile`.
#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(not(feature = "profile"), forbid(unsafe_code))]

extern crate alloc;
#[cfg(test)]
extern crate std;
#[cfg(all(not(feature = "std"), not(feature = "libm")))]
compile_error!(
    "rusty_h264-common without `std` needs the `libm` feature for its floating-point math"
);

// ---------------------------------------------------------------------------
// `no_std` shims. Every `std` use left in this crate is a diagnostic: an
// environment knob (`RS_H264_*` / `RFF_*`), a stderr census, the profiler.
// Without `std` there is no environment and no stderr, so a knob reads as
// unset and a print is a no-op: the shipped defaults, which is what a chip
// runs. Defined here, before the modules, so they are in textual scope.
// ---------------------------------------------------------------------------

/// Read an environment knob. `None` without `std` (no environment).
#[cfg(feature = "std")]
#[doc(hidden)]
pub fn knob(name: &str) -> Option<alloc::string::String> {
    std::env::var(name).ok()
}
/// Read an environment knob. `None` without `std` (no environment).
#[cfg(not(feature = "std"))]
#[doc(hidden)]
pub fn knob(_name: &str) -> Option<alloc::string::String> {
    None
}

/// A knob evaluated once and cached (`OnceLock` under `std`); evaluated per
/// call without `std`, where `knob` is always `None` and the expression
/// folds to its default.
#[doc(hidden)]
#[macro_export]
macro_rules! cached_knob {
    ($ty:ty, $init:expr) => {{
        #[cfg(feature = "std")]
        {
            static V: ::std::sync::OnceLock<$ty> = ::std::sync::OnceLock::new();
            *V.get_or_init(|| $init)
        }
        #[cfg(not(feature = "std"))]
        {
            // No environment: `knob` yields `None`, so `$init` folds to its
            // own default. Evaluated per call; it is a few Option combinators.
            $init
        }
    }};
    ($ty:ty, $default:expr, $init:expr) => {{
        let _ = $default;
        $crate::cached_knob!($ty, $init)
    }};
}

#[cfg(not(feature = "std"))]
macro_rules! eprintln {
    ($($t:tt)*) => {{
        let _ = ::core::format_args!($($t)*);
    }};
}
#[cfg(not(feature = "std"))]
#[allow(unused_macros)]
macro_rules! println {
    ($($t:tt)*) => {{
        let _ = ::core::format_args!($($t)*);
    }};
}

/// Process-wide allocator (see the crate docs).
///
/// PATCHED (hongguo-downloader): the `#[global_allocator]` install is removed.
/// rusty_alloc 1.1.6 (the version pinned here via rusty_alloc-api "=1.1.6") has
/// concurrency bugs that abort the whole process under multi-threaded
/// allocation -- our `cargo test` crashed intermittently with
/// "memory allocation of 22906492240 bytes failed". Upstream's own changelog
/// confirms a string of such fixes after 1.1.6 (see rusty_alloc 2.2.2:
/// "stress_mt aborted on 4 CPUs", cross-thread double free). We run the
/// standard Rust allocator instead; re-enable this block only together with a
/// fixed rusty_alloc.
#[cfg(any())]
#[global_allocator]
static ALLOC: rusty_alloc_api::RustyAlloc = rusty_alloc_api::RustyAlloc;

/// Per-twin CALL census (scalar vs kernel share), re-exported so consumers can
/// read it without depending on the accel crate directly. Counters are live only
/// under the `census` feature; otherwise every method is a no-op.
#[cfg(accel)]
pub use rusty_h264_accel::census;

pub mod aligned;
pub mod arms;
pub mod bit_reader;
pub mod bit_writer;
pub mod cabac_tables;
pub mod cavlc;
pub mod deblock;

/// Whether the vendored SIMD kernels are compiled in (`asm` feature on x86-64).
/// Exposed so benchmarks can state which path they measured — a harness that
/// silently falls back to the scalar twin reports numbers that look like a
/// regression in the fast path.
pub const ACCEL: bool = cfg!(accel);
pub mod inter;
pub mod nal;
pub mod predict;
/// 64-bit atomics where the target has them, the portable fallback where not.
#[doc(hidden)]
pub mod atomic {
    #[cfg(target_has_atomic = "64")]
    pub use core::sync::atomic::AtomicU64;
    #[cfg(not(target_has_atomic = "64"))]
    pub use portable_atomic::AtomicU64;
}

/// `OnceLock` on both sides of the ladder: `std::sync::OnceLock` with `std`,
/// a `once_cell::race::OnceBox` (atomics + one heap box per cell) without.
/// Same `new` / `get_or_init` shape, so a lazily built table or a cached knob
/// in a `static` reads identically on the host and on a chip.
#[doc(hidden)]
pub mod once {
    #[cfg(feature = "std")]
    pub use std::sync::OnceLock;

    #[cfg(not(feature = "std"))]
    pub struct OnceLock<T>(once_cell::race::OnceBox<T>);

    #[cfg(not(feature = "std"))]
    impl<T> OnceLock<T> {
        /// An empty cell.
        pub const fn new() -> Self {
            OnceLock(once_cell::race::OnceBox::new())
        }
        /// The value, initialising it with `f` on first use.
        pub fn get_or_init(&self, f: impl FnOnce() -> T) -> &T {
            self.0.get_or_init(|| alloc::boxed::Box::new(f()))
        }
        /// The value if initialised.
        pub fn get(&self) -> Option<&T> {
            self.0.get()
        }
        /// Set the value if the cell is empty; otherwise hand it back.
        pub fn set(&self, v: T) -> Result<(), T> {
            self.0.set(alloc::boxed::Box::new(v)).map_err(|b| *b)
        }
    }

    #[cfg(not(feature = "std"))]
    impl<T> Default for OnceLock<T> {
        fn default() -> Self {
            Self::new()
        }
    }
}

pub mod fmath;
pub mod prof;
pub mod transform;
pub mod types;

pub use bit_reader::BitReader;
pub use bit_writer::BitWriter;
pub use nal::{NalUnit, NalUnitType};
pub use types::{ChromaFormat, Profile, YuvFrame, YuvPlanes};
