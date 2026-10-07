//! Free-slice pool for platforms where memory can never return to the host
//! (F2 of docs/plans/segment-tax.md).
//!
//! On wasm, `prim::free` is a no-op and linear memory only grows, so the
//! allocator's only recycling is what it keeps for itself. v1.1.5's
//! adopt-on-free arenas recycled at SEGMENT_SIZE (32 MiB) granularity, which
//! left the segment tax standing: every huge reservation was rounded up to
//! whole 32 MiB chunks, and a 33 MiB block permanently cost 64 MiB. The
//! rounding existed because every segment base had to be SEGMENT_SIZE-aligned
//! for `segment_of`'s pointer mask — and once wasm's `segment_of` resolves
//! through the slice-granular base table instead (`segment_map::base_of`),
//! that constraint is gone and this pool can hand memory around at 64 KiB
//! slices.
//!
//! The pool is a bitmap over the wasm address space: one bit per
//! `SEGMENT_SLICE_SIZE` slice, set = the slice is free and pool-owned.
//! `alloc_run` is first-fit; freeing sets bits, so adjacent ranges coalesce
//! by construction. wasm32 addresses are < 4 GiB, so the whole map is
//! 65,536 bits = 8 KiB.
//!
//! Bookkeeping only: nothing here dereferences the memory it tracks, which
//! is also why the module compiles and unit-tests on every target even
//! though only the wasm segment paths are wired to it. Single-threaded by
//! platform (wasm32 without the threads proposal — the standing assumption
//! of `prim/wasm.rs`); the atomics are for `static` soundness, not for
//! concurrency, and are `Relaxed` throughout.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::types::SEGMENT_SLICE_SIZE;

/// Derived, not written down: this was a hardcoded `16` with a const assert
/// pinning it to a 64 KiB slice, which made the slice size unchangeable
/// (P2 of `docs/plans/small-metal.md` — it was one of exactly two lines in the
/// crate that refused a different geometry).
const SLICE_SHIFT: usize = SEGMENT_SLICE_SIZE.trailing_zeros() as usize;
const _: () = assert!(1 << SLICE_SHIFT == SEGMENT_SLICE_SIZE);
/// 4 GiB of address space in slice-sized steps.
const SLOTS: usize = 1 << (32 - SLICE_SHIFT);
/// Bits per pool word. `u32`, not `u64`: a bitmap's width is a free choice
/// and 32-bit RISC-V / Xtensa have no 64-bit atomic (P3 of
/// `docs/plans/small-metal.md`).
const WORD_BITS: usize = u32::BITS as usize;
const WORDS: usize = SLOTS / WORD_BITS;

static FREE: [AtomicU32; WORDS] = [const { AtomicU32::new(0) }; WORDS];

#[inline]
fn bit(idx: usize) -> (usize, u32) {
    (idx / WORD_BITS, 1u32 << (idx % WORD_BITS))
}

/// Return `[base, base + size)` to the pool. `false` (and no state change)
/// when the range is not slice-granular or not addressable by the map —
/// the caller falls through to `os::free`, exactly as adoption does.
///
/// Slice 0 is never accepted: address 0 is the null page and the module's
/// own data lives in the low slices, so a zero base is a caller bug, and
/// refusing it keeps `alloc_run`'s `base == 0` distinct from "no run".
pub fn free_range(base: usize, size: usize) -> bool {
    if base < SEGMENT_SLICE_SIZE
        || size == 0
        || !base.is_multiple_of(SEGMENT_SLICE_SIZE)
        || !size.is_multiple_of(SEGMENT_SLICE_SIZE)
    {
        return false;
    }
    let start = base >> SLICE_SHIFT;
    let n = size >> SLICE_SHIFT;
    let Some(end) = start.checked_add(n) else {
        return false;
    };
    if end > SLOTS {
        return false;
    }
    for idx in start..end {
        let (w, b) = bit(idx);
        let prev = FREE[w].fetch_or(b, Ordering::Relaxed);
        debug_assert_eq!(prev & b, 0, "slice pool: double free of slice {idx}");
    }
    true
}

/// First-fit run of `slices` free slices. Returns the run's base address and
/// clears its bits, or `None` — the caller reserves fresh memory instead.
pub fn alloc_run(slices: usize) -> Option<usize> {
    if slices == 0 || slices > SLOTS {
        return None;
    }
    let mut run = 0usize;
    let mut idx = 0usize;
    while idx < SLOTS {
        let (w, _) = bit(idx);
        let word = FREE[w].load(Ordering::Relaxed);
        if word == 0 && idx.is_multiple_of(WORD_BITS) {
            // Whole word empty: skip it. Resetting the run is correct, not
            // merely convenient — a run cannot cross a zero word.
            run = 0;
            idx += WORD_BITS;
            continue;
        }
        if word & (1 << (idx % WORD_BITS)) != 0 {
            run += 1;
            if run == slices {
                let start = idx + 1 - slices;
                for j in start..=idx {
                    let (jw, jb) = bit(j);
                    FREE[jw].fetch_and(!jb, Ordering::Relaxed);
                }
                return Some(start << SLICE_SHIFT);
            }
        } else {
            run = 0;
        }
        idx += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One lock, and a drain discipline: `alloc_run` is GLOBAL first-fit, so
    /// a test that leaves bits set hands the lowest-address run to whichever
    /// test allocates next — the same cross-test aliasing the adoption tests
    /// hit through VirtualAlloc adjacency. Every test therefore runs under
    /// the lock and exits with its window drained (all its freed slices
    /// allocated back out).
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Bytes for `n` slices. These tests are about the POOL's arithmetic —
    /// runs, first fit, coalescing, word boundaries — all of which is counted
    /// in slices, so they are written in slices. They used to be written in
    /// MiB, which silently assumed a 64 KiB slice and inverted the moment
    /// `SEGMENT_SLICE_SIZE` moved (P2, `docs/plans/small-metal.md`: at an
    /// 8 KiB slice "1 MiB = 16 slices" became 128, and three tests failed on
    /// arithmetic that had nothing to do with what they test).
    const fn sl(n: usize) -> usize {
        n * SEGMENT_SLICE_SIZE
    }

    #[test]
    fn round_trips_and_coalesces() {
        let _g = lock();
        let base = sl(4096);
        assert!(free_range(base, sl(32)));
        assert!(free_range(base + sl(32), sl(16))); // adjacent: coalesces by construction
        // 48 slices, spanning the two freed ranges as one run.
        assert_eq!(alloc_run(48), Some(base), "coalesced run");
        // Pool drained: the same run is not served twice.
        assert!(alloc_run(1).is_none());
        assert!(free_range(base, sl(48)));
        assert_eq!(alloc_run(48), Some(base));
    }

    #[test]
    fn first_fit_skips_too_small_holes() {
        let _g = lock();
        let base = sl(8192);
        assert!(free_range(base, sl(16)));
        assert!(free_range(base + sl(128), sl(64))); // disjoint
        assert_eq!(
            alloc_run(32),
            Some(base + sl(128)),
            "a 32-slice run must skip the 16-slice hole"
        );
        // The small hole is intact; drain everything on the way out.
        assert_eq!(alloc_run(16), Some(base));
        assert_eq!(alloc_run(32), Some(base + sl(160)));
        assert!(alloc_run(1).is_none());
    }

    #[test]
    fn runs_cross_word_boundaries() {
        let _g = lock();
        // Slice index 1000..1100 straddles the u64 word boundary at 1024.
        let base = sl(1000);
        assert!(free_range(base, sl(100)));
        assert_eq!(alloc_run(100), Some(base));
        assert!(alloc_run(1).is_none());
    }

    /// Every rejection, written in SLICES.
    ///
    /// This test was the last byte-denominated one in the module, and it failed
    /// exactly the way P2's did: `MIB + 4096` was "misaligned" only while a
    /// slice was 8 KiB, and became slice-ALIGNED the moment the small profile
    /// went to 4 KiB. It then quietly *succeeded* in freeing two ranges it is
    /// supposed to refuse, left their bits set in a pool the module doc calls
    /// GLOBAL first-fit, and took down the other three tests instead of itself.
    /// Offsets of `+1` and `SLOTS`-relative bases are misaligned and out of
    /// range at every slice size there will ever be.
    #[test]
    fn rejects_what_it_cannot_track() {
        let _g = lock();
        assert!(!free_range(0, sl(16)), "slice 0 must be refused");
        assert!(!free_range(sl(16), 0), "empty range");
        assert!(!free_range(sl(256) + 1, sl(16)), "misaligned base");
        assert!(!free_range(sl(256), sl(16) + 1), "ragged size");
        assert!(
            !free_range(sl(SLOTS - 1), sl(2)),
            "a run ending past SLOTS is unaddressable"
        );
        assert!(alloc_run(0).is_none());
        assert!(alloc_run(SLOTS + 1).is_none());
        // The pool must be untouched: every call above was a refusal, and a
        // refusal that set a bit would strand it for whichever test runs next.
        assert!(alloc_run(1).is_none(), "a refusal leaves the pool empty");
    }
}
