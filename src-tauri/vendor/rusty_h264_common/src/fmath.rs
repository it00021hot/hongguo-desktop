//! Floating-point math that works with and without `std`.
//!
//! `f64::sqrt`, `powf`, `ln`, `log2`, `exp2`, `floor`, `round` and friends are
//! inherent methods that only exist with `std` (they call the platform libm).
//! Without `std` this module supplies the same names as an extension trait
//! backed by the pure-Rust [`libm`] crate, so call sites stay `x.sqrt()` and
//! compile on both. With `std` the inherent methods win method resolution and
//! nothing changes — the shipped bytes are the platform's, as before.
//!
//! Enable the `libm` feature with `std` too when a host and a chip must make
//! **bit-identical** decisions: the platform libm is not guaranteed to agree
//! with itself across machines, `libm` is deterministic. `rusty_flac` made the
//! same call for the same reason.
//!
//! Import `use crate::fmath::F64Ext as _;` (and `F32Ext`) in a file that uses
//! these methods; the import is harmless with `std`.

#![allow(clippy::wrong_self_convention)]

/// `f64` methods that need a libm.
pub trait F64Ext {
    /// Square root.
    fn sqrt(self) -> f64;
    /// `self ^ n`.
    fn powf(self, n: f64) -> f64;
    /// `self ^ n` for an integer `n`.
    fn powi(self, n: i32) -> f64;
    /// Natural logarithm.
    fn ln(self) -> f64;
    /// Base-2 logarithm.
    fn log2(self) -> f64;
    /// Base-10 logarithm.
    fn log10(self) -> f64;
    /// `e ^ self`.
    fn exp(self) -> f64;
    /// `2 ^ self`.
    fn exp2(self) -> f64;
    /// Largest integer ≤ self.
    fn floor(self) -> f64;
    /// Smallest integer ≥ self.
    fn ceil(self) -> f64;
    /// Nearest integer, ties away from zero.
    fn round(self) -> f64;
    /// Nearest integer, ties to even.
    fn round_ties_even(self) -> f64;
    /// Integer part toward zero.
    fn trunc(self) -> f64;
    /// Fractional part.
    fn fract(self) -> f64;
    /// `self * a + b` with a single rounding.
    fn mul_add(self, a: f64, b: f64) -> f64;
    /// `sqrt(self² + other²)`.
    fn hypot(self, other: f64) -> f64;
}

/// `f32` methods that need a libm.
pub trait F32Ext {
    /// Square root.
    fn sqrt(self) -> f32;
    /// `self ^ n`.
    fn powf(self, n: f32) -> f32;
    /// `self ^ n` for an integer `n`.
    fn powi(self, n: i32) -> f32;
    /// Natural logarithm.
    fn ln(self) -> f32;
    /// Base-2 logarithm.
    fn log2(self) -> f32;
    /// `e ^ self`.
    fn exp(self) -> f32;
    /// `2 ^ self`.
    fn exp2(self) -> f32;
    /// Largest integer ≤ self.
    fn floor(self) -> f32;
    /// Smallest integer ≥ self.
    fn ceil(self) -> f32;
    /// Nearest integer, ties away from zero.
    fn round(self) -> f32;
    /// Integer part toward zero.
    fn trunc(self) -> f32;
    /// `self * a + b` with a single rounding.
    fn mul_add(self, a: f32, b: f32) -> f32;
}

#[cfg(not(feature = "std"))]
impl F64Ext for f64 {
    fn sqrt(self) -> f64 {
        libm::sqrt(self)
    }
    fn powf(self, n: f64) -> f64 {
        libm::pow(self, n)
    }
    fn powi(self, n: i32) -> f64 {
        libm::pow(self, f64::from(n))
    }
    fn ln(self) -> f64 {
        libm::log(self)
    }
    fn log2(self) -> f64 {
        libm::log2(self)
    }
    fn log10(self) -> f64 {
        libm::log10(self)
    }
    fn exp(self) -> f64 {
        libm::exp(self)
    }
    fn exp2(self) -> f64 {
        libm::exp2(self)
    }
    fn floor(self) -> f64 {
        libm::floor(self)
    }
    fn ceil(self) -> f64 {
        libm::ceil(self)
    }
    fn round(self) -> f64 {
        libm::round(self)
    }
    fn round_ties_even(self) -> f64 {
        libm::rint(self)
    }
    fn trunc(self) -> f64 {
        libm::trunc(self)
    }
    fn fract(self) -> f64 {
        self - libm::trunc(self)
    }
    fn mul_add(self, a: f64, b: f64) -> f64 {
        libm::fma(self, a, b)
    }
    fn hypot(self, other: f64) -> f64 {
        libm::hypot(self, other)
    }
}

#[cfg(not(feature = "std"))]
impl F32Ext for f32 {
    fn sqrt(self) -> f32 {
        libm::sqrtf(self)
    }
    fn powf(self, n: f32) -> f32 {
        libm::powf(self, n)
    }
    fn powi(self, n: i32) -> f32 {
        libm::powf(self, n as f32)
    }
    fn ln(self) -> f32 {
        libm::logf(self)
    }
    fn log2(self) -> f32 {
        libm::log2f(self)
    }
    fn exp(self) -> f32 {
        libm::expf(self)
    }
    fn exp2(self) -> f32 {
        libm::exp2f(self)
    }
    fn floor(self) -> f32 {
        libm::floorf(self)
    }
    fn ceil(self) -> f32 {
        libm::ceilf(self)
    }
    fn round(self) -> f32 {
        libm::roundf(self)
    }
    fn trunc(self) -> f32 {
        libm::truncf(self)
    }
    fn mul_add(self, a: f32, b: f32) -> f32 {
        libm::fmaf(self, a, b)
    }
}

// With `std` the inherent methods exist and win method resolution; these
// impls only make the trait usable as a bound. They forward to the inherent
// methods (or `libm` when asked for determinism).
#[cfg(feature = "std")]
impl F64Ext for f64 {
    #[cfg(feature = "libm")]
    fn sqrt(self) -> f64 {
        libm::sqrt(self)
    }
    #[cfg(not(feature = "libm"))]
    fn sqrt(self) -> f64 {
        f64::sqrt(self)
    }
    fn powf(self, n: f64) -> f64 {
        f64::powf(self, n)
    }
    fn powi(self, n: i32) -> f64 {
        f64::powi(self, n)
    }
    fn ln(self) -> f64 {
        f64::ln(self)
    }
    fn log2(self) -> f64 {
        f64::log2(self)
    }
    fn log10(self) -> f64 {
        f64::log10(self)
    }
    fn exp(self) -> f64 {
        f64::exp(self)
    }
    fn exp2(self) -> f64 {
        f64::exp2(self)
    }
    fn floor(self) -> f64 {
        f64::floor(self)
    }
    fn ceil(self) -> f64 {
        f64::ceil(self)
    }
    fn round(self) -> f64 {
        f64::round(self)
    }
    fn round_ties_even(self) -> f64 {
        f64::round_ties_even(self)
    }
    fn trunc(self) -> f64 {
        f64::trunc(self)
    }
    fn fract(self) -> f64 {
        f64::fract(self)
    }
    fn mul_add(self, a: f64, b: f64) -> f64 {
        f64::mul_add(self, a, b)
    }
    fn hypot(self, other: f64) -> f64 {
        f64::hypot(self, other)
    }
}

#[cfg(feature = "std")]
impl F32Ext for f32 {
    fn sqrt(self) -> f32 {
        f32::sqrt(self)
    }
    fn powf(self, n: f32) -> f32 {
        f32::powf(self, n)
    }
    fn powi(self, n: i32) -> f32 {
        f32::powi(self, n)
    }
    fn ln(self) -> f32 {
        f32::ln(self)
    }
    fn log2(self) -> f32 {
        f32::log2(self)
    }
    fn exp(self) -> f32 {
        f32::exp(self)
    }
    fn exp2(self) -> f32 {
        f32::exp2(self)
    }
    fn floor(self) -> f32 {
        f32::floor(self)
    }
    fn ceil(self) -> f32 {
        f32::ceil(self)
    }
    fn round(self) -> f32 {
        f32::round(self)
    }
    fn trunc(self) -> f32 {
        f32::trunc(self)
    }
    fn mul_add(self, a: f32, b: f32) -> f32 {
        f32::mul_add(self, a, b)
    }
}

// ---------------------------------------------------------------------------
// Free functions: the ONE routing point for every float call on the coding
// path. `x.sqrt()` resolves to the inherent (platform-libm) method under `std`
// no matter what the traits above say, so a host with `libm` enabled was still
// reading the platform libm at every site but `sqrt` — and the promise the
// feature makes (a host and a chip make the same float-derived decisions bit
// for bit) held for `sqrt` alone. These route to the pure-Rust `libm` whenever
// the feature is on, with or without `std`, and to the inherent method
// otherwise (byte-identical to before). Coding-path call sites use these.
// ---------------------------------------------------------------------------

macro_rules! route1 {
    ($(#[$doc:meta])* $name:ident, $lm:path, $inh:path) => {
        $(#[$doc])*
        #[cfg(feature = "libm")]
        #[inline]
        pub fn $name(x: f64) -> f64 {
            $lm(x)
        }
        $(#[$doc])*
        #[cfg(not(feature = "libm"))]
        #[inline]
        pub fn $name(x: f64) -> f64 {
            $inh(x)
        }
    };
}

route1!(
    /// Square root.
    sqrt, libm::sqrt, f64::sqrt
);
route1!(
    /// Base-2 logarithm.
    log2, libm::log2, f64::log2
);
route1!(
    /// Natural logarithm.
    ln, libm::log, f64::ln
);
route1!(
    /// `2 ^ x`.
    exp2, libm::exp2, f64::exp2
);
route1!(
    /// `e ^ x`.
    exp, libm::exp, f64::exp
);
route1!(
    /// Nearest integer, ties away from zero.
    round, libm::round, f64::round
);
route1!(
    /// Largest integer ≤ x.
    floor, libm::floor, f64::floor
);
route1!(
    /// Smallest integer ≥ x.
    ceil, libm::ceil, f64::ceil
);

/// `x ^ n`.
#[cfg(feature = "libm")]
#[inline]
pub fn powf(x: f64, n: f64) -> f64 {
    libm::pow(x, n)
}
/// `x ^ n`.
#[cfg(not(feature = "libm"))]
#[inline]
pub fn powf(x: f64, n: f64) -> f64 {
    f64::powf(x, n)
}

/// `x ^ n` for an integer `n`.
#[cfg(feature = "libm")]
#[inline]
pub fn powi(x: f64, n: i32) -> f64 {
    libm::pow(x, f64::from(n))
}
/// `x ^ n` for an integer `n`.
#[cfg(not(feature = "libm"))]
#[inline]
pub fn powi(x: f64, n: i32) -> f64 {
    f64::powi(x, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trait_agrees_with_the_platform_where_both_exist() {
        for x in [0.25f64, 1.0, 2.0, 10.5, 1234.5678] {
            assert!((F64Ext::sqrt(x) - x.sqrt()).abs() < 1e-12);
            assert!((F64Ext::log2(x) - x.log2()).abs() < 1e-12);
            assert_eq!(F64Ext::floor(x), x.floor());
            assert_eq!(F64Ext::round_ties_even(x), x.round_ties_even());
        }
        assert_eq!(F64Ext::powf(2.0, 10.0), 1024.0);
        assert_eq!(F32Ext::exp2(3.0f32), 8.0);
    }
}
