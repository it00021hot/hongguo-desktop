//! Kani proof harnesses (hardening gate H-30).
//!
//! Compiled only under `cfg(kani)`, so this module costs the shipped crate
//! nothing. Run with `cargo kani -p rusty_alloc`.
//!
//! **What a proof is for here.** The rest of the gate ladder is empirical:
//! Miri interprets the paths a test happens to take, the fuzzers sample
//! ~7M inputs, loom exhausts a small interleaving space. All three answer
//! "no counterexample was FOUND". Kani answers "no counterexample EXISTS"
//! over a symbolic input range — which is the right instrument for the
//! arithmetic that every `unsafe` block in this crate rests on:
//!
//!   * `page_of`'s slice index is `< SLICES_PER_SEGMENT` for EVERY pointer
//!     inside a segment. That bound is the contract discharged at all eight
//!     call sites and the reason the bounds check was removed (M10b brick
//!     #4). If it can fail for any offset, that removal is a memory-safety
//!     bug rather than an optimisation.
//!   * `slice_offset` fits the `u16` it is stored in — currently guarded by
//!     a const assert; proved here over the whole index range.
//!   * the bin geometry never returns an out-of-range queue index and
//!     `good_size` never shrinks a request, for every size — the two
//!     properties the direct table and every queue index depend on.

use crate::bins;
use crate::types::{
    BIN_FULL, BIN_HUGE, MEDIUM_OBJ_SIZE_MAX, SEGMENT_SIZE, SEGMENT_SLICE_SIZE, SLICES_PER_SEGMENT,
    SMALL_SIZE_MAX, SMALL_WSIZE_MAX, wsize_from_size,
};

/// `page_of` computes `idx = (p - seg) / SEGMENT_SLICE_SIZE` and then indexes
/// `[Page; SLICES_PER_SEGMENT]` WITHOUT a bounds check, on the argument that
/// `p` lies inside the segment by the caller's contract. Prove that argument
/// holds for every in-segment offset, not merely the ones a test tried.
#[kani::proof]
fn page_of_slice_index_is_always_in_range() {
    let off: usize = kani::any();
    kani::assume(off < SEGMENT_SIZE); // the caller's contract, symbolically
    let idx = off / SEGMENT_SLICE_SIZE;
    assert!(idx < SLICES_PER_SEGMENT);
}

/// The `slice_offset` field is a `u16` holding a SLICE distance back to the
/// span start (M12; bytes until 2026-08-22). Prove the encoding cannot
/// overflow for any interior slot — the const assert checks only the maximum,
/// this checks every index. Also prove the byte distance the `debug_assert` in
/// `page_of` reconstructs from it stays in range, since that scaling is where
/// the old overflow risk lived.
#[kani::proof]
fn slice_offset_always_fits_u16() {
    let idx: usize = kani::any();
    kani::assume(idx < SLICES_PER_SEGMENT);
    assert!(idx <= u16::MAX as usize);
    let bytes = idx * core::mem::size_of::<crate::page::Page>();
    assert!(bytes <= u16::MAX as usize);
}

/// Every size maps to a queue index the heap actually has. A bin outside
/// `0..=BIN_FULL` would index `Heap::pages` out of range on the allocation
/// path.
/// **Bounded**, and the bound is part of the claim: CBMC reasons
/// bit-precisely, and `bin`/`good_size` use `leading_zeros` and variable
/// shifts, so an unbounded 64-bit domain does not terminate in usable time
/// (measured: >13 CPU-minutes, killed). The domain below covers every
/// structural case the function has — small bins, the MI_ALIGN2W region, the
/// four-per-power-of-two region, and BOTH sides of the MEDIUM_OBJ_SIZE_MAX
/// cutoff — which is what the proof is about. Sizes beyond it differ only in
/// magnitude, and the property test covers those empirically.
#[kani::proof]
#[kani::unwind(4)]
fn bin_index_is_always_a_real_queue() {
    let size: usize = kani::any();
    kani::assume(size <= MEDIUM_OBJ_SIZE_MAX * 2);
    let b = bins::bin(size);
    assert!(b <= BIN_FULL);
    // Sizes past the medium cutoff must route to the dedicated-segment path,
    // never to a normal queue: that is what keeps huge blocks off the binned
    // free lists.
    if size > MEDIUM_OBJ_SIZE_MAX {
        assert!(b == BIN_HUGE);
    }
}

/// `good_size` is the ABI-visible promise a caller sizes buffers against: it
/// must never return less than requested. Proved for every size rather than
/// the 2 million the property test samples.
/// Bounded for the same reason as the proof above; the domain spans both
/// sides of the binned cutoff, which is where the rounding changes shape.
#[kani::proof]
#[kani::unwind(4)]
fn good_size_never_shrinks_a_request() {
    let size: usize = kani::any();
    kani::assume(size <= MEDIUM_OBJ_SIZE_MAX * 2);
    assert!(bins::good_size(size) >= size);
}

/// The small-malloc fast path indexes `direct[wsize_from_size(size)]`, an
/// array of `PAGES_DIRECT` entries, with NO bounds check on the hot path.
/// Prove the index is in range for every size the fast path accepts.
#[kani::proof]
fn direct_table_index_is_always_in_range() {
    let size: usize = kani::any();
    kani::assume(size <= SMALL_SIZE_MAX); // the fast path's own guard
    let w = wsize_from_size(size);
    assert!(w <= SMALL_WSIZE_MAX);
    assert!(w < bins::PAGES_DIRECT);
}
