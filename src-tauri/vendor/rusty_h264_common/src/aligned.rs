//! A 16-byte-aligned heap byte buffer for the openh264 spatial asm kernels.
//!
//! openh264's deblock / MC / intra-prediction asm load aligned 16-byte row chunks
//! (`movdqa`), which a plain `Vec<u8>` (alignment 1) cannot satisfy. [`AlignedBytes`]
//! is backed by a `Vec<u128>` (alignment 16) and viewed as `[u8]` through `bytemuck`
//! — guaranteed 16-aligned, no `unsafe`. It `Deref`s to `[u8]`, so it is a drop-in for
//! the reconstruction / reference plane buffers (`FrameEncoder` rec planes, `RefFrame`).

#[allow(unused_imports)]
use alloc::vec;
use alloc::vec::Vec;

use core::ops::{Deref, DerefMut};

/// A heap byte buffer guaranteed to start on a 16-byte boundary.
#[derive(Clone, Default)]
pub struct AlignedBytes {
    words: Vec<u128>,
    len: usize,
}

impl AlignedBytes {
    /// A zero-filled, 16-byte-aligned buffer of `len` bytes.
    pub fn zeroed(len: usize) -> Self {
        Self {
            words: vec![0u128; (len + 15) / 16],
            len,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl core::fmt::Debug for AlignedBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "AlignedBytes({} bytes)", self.len)
    }
}

impl Deref for AlignedBytes {
    type Target = [u8];
    #[inline]
    fn deref(&self) -> &[u8] {
        &bytemuck::cast_slice(&self.words)[..self.len]
    }
}

impl DerefMut for AlignedBytes {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut bytemuck::cast_slice_mut(&mut self.words)[..self.len]
    }
}
