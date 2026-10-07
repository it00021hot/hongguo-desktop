//! Global segment map (mirrors upstream `segment-map.c`): one bit per 32 MiB
//! address window, answering "does this pointer lie in memory we own?" —
//! `mi_is_in_heap_region`, and the `debug_checks` foreign-free guard.
//!
//! 48-bit user VA / 32 MiB windows → 2²³ bits = 1 MiB of BSS (zero-init,
//! touched sparsely). Addresses above 2⁴⁸ (LA57) are conservatively reported
//! as NOT ours — a false negative for `is_in_heap_region`, never a false
//! positive.

// Three representations, and each compiles the other two out rather than
// leaving them as dead weight:
//   wasm          -> the slice-granular `base_table` (segments are not
//                    SEGMENT_SIZE-aligned there, so `segment_of` cannot mask)
//   small profile -> the exact `range_table` (the bitmap is sized by ADDRESS
//                    SPACE, which a chip cannot afford at any geometry)
//   otherwise     -> the window bitmap
#[cfg(all(not(ra_small_profile), not(all(target_arch = "wasm32", not(miri)))))]
use core::sync::atomic::{AtomicU32, Ordering};

use crate::segment::Segment;
use crate::types::SEGMENT_SIZE;

/// Addressable bits the map has to span.
///
/// 48 is the x86-64 / aarch64 user-VA limit. On a 32-bit target the whole
/// address space is 32 bits, and spanning 48 would size the table for memory
/// that cannot exist — 65,536x too large. Derived rather than written down
/// (P2 of `docs/plans/small-metal.md`).
#[cfg(all(not(ra_small_profile), not(all(target_arch = "wasm32", not(miri)))))]
const ADDR_BITS: usize = if usize::BITS == 64 {
    48
} else {
    usize::BITS as usize
};

/// `log2(SEGMENT_SIZE)`. This was a hardcoded `25` with a const assert pinning
/// it; deriving it is what lets the geometry move at all — it and
/// `slice_pool::SLICE_SHIFT` were the only two lines in the crate that refused
/// a different `SEGMENT_SIZE`.
const WINDOW_SHIFT: usize = SEGMENT_SIZE.trailing_zeros() as usize;
#[cfg(all(not(ra_small_profile), not(all(target_arch = "wasm32", not(miri)))))]
const MAP_BITS: usize = 1 << (ADDR_BITS - WINDOW_SHIFT);
#[cfg(all(not(ra_small_profile), not(all(target_arch = "wasm32", not(miri)))))]
const MAP_WORDS: usize = MAP_BITS / WORD_BITS;

const _: () = assert!(1 << WINDOW_SHIFT == SEGMENT_SIZE);

/// Bits per map word. `u32`, not `u64`: the width is a free choice for a
/// bitmap and 32-bit RISC-V / Xtensa have no 64-bit atomic (P3 of
/// `docs/plans/small-metal.md`).
#[cfg(all(not(ra_small_profile), not(all(target_arch = "wasm32", not(miri)))))]
const WORD_BITS: usize = u32::BITS as usize;

#[cfg(all(not(ra_small_profile), not(all(target_arch = "wasm32", not(miri)))))]
static MAP: [AtomicU32; MAP_WORDS] = [const { AtomicU32::new(0) }; MAP_WORDS];

/// Small profile: an EXACT range table instead of a window bitmap.
///
/// The bitmap's size is a function of the ADDRESS SPACE, not of the memory
/// actually owned — 1 MiB of BSS at 48 bits, and *worse* as `SEGMENT_SIZE`
/// shrinks (a 64 KiB segment would want 2^32 bits). On a part with 512 KiB of
/// SRAM that is not a tuning problem, it is a disqualifier, and it is
/// independent of §2.1's geometry: parameterising the segment size alone makes
/// this table bigger, not smaller.
///
/// A chip owns a handful of segments carved from one region, so the exact set
/// fits in a fixed array and `contains` is a short scan. Exactness matters:
/// `contains` backs the `debug_checks` foreign-pointer guard, where a false
/// negative aborts a legitimate free — so a truncated bitmap (which would
/// merely under-report) is NOT an acceptable substitute here, even though the
/// module's own doc permits false negatives for `is_in_heap_region`.
#[cfg(all(ra_small_profile, not(all(target_arch = "wasm32", not(miri)))))]
mod range_table {
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// Live ranges tracked at once — 1 KiB of BSS, against the 1 MiB the
    /// bitmap costs. Each range is a whole segment or huge reservation, so at
    /// a 64 KiB segment this spans 4 MiB of registered memory, comfortably
    /// past any chip this profile targets (Janus firmwares declare 64–220 KiB
    /// heaps).
    ///
    /// **A smaller segment multiplies every per-segment structure.** Going
    /// 32 MiB → 64 KiB is 512x more live segments for the same bytes managed,
    /// and this table was the first thing to notice: sized at 32 it overflowed
    /// during the host battery and the allocator began aborting legitimate
    /// frees. That is a property of the geometry, not of this table, and it is
    /// the reason the overflow behaviour below matters more than the number.
    const MAX_RANGES: usize = 64;

    static BASE: [AtomicUsize; MAX_RANGES] = [const { AtomicUsize::new(0) }; MAX_RANGES];
    static END: [AtomicUsize; MAX_RANGES] = [const { AtomicUsize::new(0) }; MAX_RANGES];
    static LOCK: AtomicBool = AtomicBool::new(false);

    /// Set once the table has ever been full, and never cleared.
    ///
    /// **Which way to fail is not symmetric here, and the module doc's
    /// "false negative, never a false positive" is the wrong rule for this
    /// consumer.** `contains` backs two callers: `is_in_heap_region`, a
    /// diagnostic where a false positive merely misleads; and the
    /// `debug_checks` foreign-pointer guard, where a false NEGATIVE aborts a
    /// legitimate free. Dropping a range silently — the first version of this
    /// table — produced exactly that: three integration suites aborting inside
    /// `free` on pointers the allocator had certainly returned.
    ///
    /// So once membership can no longer be decided, `contains` degrades
    /// PERMISSIVELY: the guard stops catching foreign pointers rather than
    /// rejecting good ones, and the loss of the diagnostic is readable from
    /// [`overflowed`] rather than inferred from a crash.
    static OVERFLOWED: AtomicBool = AtomicBool::new(false);

    struct Guard;
    impl Guard {
        fn acquire() -> Self {
            while LOCK
                .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                core::hint::spin_loop();
            }
            Self
        }
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            LOCK.store(false, Ordering::Release);
        }
    }

    /// Record `[base, base+size)`. On a full table, latch [`OVERFLOWED`] so
    /// membership degrades permissively rather than silently wrong.
    pub(super) fn set(base: usize, size: usize) {
        let _g = Guard::acquire();
        for i in 0..MAX_RANGES {
            if END[i].load(Ordering::Relaxed) == 0 {
                BASE[i].store(base, Ordering::Relaxed);
                // A range that wraps the address space cannot be recorded;
                // latch the same fail-closed flag a full table does.
                let Some(end) = base.checked_add(size.max(1)) else {
                    OVERFLOWED.store(true, Ordering::Release);
                    return;
                };
                END[i].store(end, Ordering::Release);
                return;
            }
        }
        // Deliberately NOT a `debug_assert`: this is reachable in a VALID
        // configuration — the host battery under this profile manages far more
        // segments than any chip does — and an assertion should mean
        // "impossible", not "expected when you test it off-target". The latch
        // is the signal; [`overflowed`] is how a test reads it.
        OVERFLOWED.store(true, Ordering::Release);
    }

    /// Whether the table has ever been full, i.e. whether `contains` has
    /// stopped being exact. A chip-shaped workload must never set this; the
    /// host battery does, which is what the two are for.
    pub(super) fn overflowed() -> bool {
        OVERFLOWED.load(Ordering::Acquire)
    }

    /// Forget `[base, base+size)`. Matches on the base, so an unregister that
    /// was never registered is a no-op.
    pub(super) fn clear(base: usize, size: usize) {
        let end = base.saturating_add(size.max(1));
        let _g = Guard::acquire();
        for i in 0..MAX_RANGES {
            if BASE[i].load(Ordering::Relaxed) == base && END[i].load(Ordering::Relaxed) == end {
                END[i].store(0, Ordering::Release);
                BASE[i].store(0, Ordering::Relaxed);
                return;
            }
        }
    }

    /// Exact membership while the table has held every range; permissive once
    /// it has not (see [`OVERFLOWED`]). Lock-free: a racing
    /// register/unregister can flip the answer, which is the same best-effort
    /// the bitmap gives.
    pub(super) fn contains(addr: usize) -> bool {
        if OVERFLOWED.load(Ordering::Acquire) {
            return true;
        }
        (0..MAX_RANGES).any(|i| {
            let e = END[i].load(Ordering::Acquire);
            e != 0 && addr >= BASE[i].load(Ordering::Relaxed) && addr < e
        })
    }
}

/// wasm: a slice-granular BASE table instead of the window bitmap.
///
/// On wasm, segments are 64 KiB-slice-aligned rather than
/// SEGMENT_SIZE-aligned (F2, docs/plans/segment-tax.md): requiring 32 MiB
/// bases on a platform whose memory can never be returned made every ragged
/// reservation strand its tail forever — the segment tax. Slice-aligned
/// bases mean `segment_of` can no longer mask; it asks this table instead.
/// One entry per 64 KiB slice of the 4 GiB address space (256 KiB of BSS,
/// wasm-only), holding `(base >> 16) + 1` so zero stays "no segment". Two
/// segments may share a 32 MiB window here, which is also why the window
/// bitmap above is not maintained on wasm — clearing a window on one
/// segment's free would lie about its neighbour.
#[cfg(all(target_arch = "wasm32", not(miri)))]
mod base_table {
    use core::sync::atomic::{AtomicU32, Ordering};

    const SLICE_SHIFT: usize = 16;
    const _: () = assert!(1 << SLICE_SHIFT == crate::types::SEGMENT_SLICE_SIZE);
    const SLOTS: usize = 1 << (32 - SLICE_SHIFT);
    static BASE: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];

    fn slots(base: usize, size: usize) -> core::ops::Range<usize> {
        let start = base >> SLICE_SHIFT;
        let end = base
            .saturating_add(size.max(1))
            .div_ceil(1 << SLICE_SHIFT)
            .min(SLOTS);
        start.min(SLOTS)..end
    }

    pub(super) fn set(base: usize, size: usize) {
        let entry = ((base >> SLICE_SHIFT) + 1) as u32;
        for i in slots(base, size) {
            BASE[i].store(entry, Ordering::Relaxed);
        }
    }

    pub(super) fn clear(base: usize, size: usize) {
        for i in slots(base, size) {
            BASE[i].store(0, Ordering::Relaxed);
        }
    }

    pub(super) fn get(addr: usize) -> usize {
        let i = addr >> SLICE_SHIFT;
        if i >= SLOTS {
            return 0;
        }
        match BASE[i].load(Ordering::Relaxed) {
            0 => 0,
            e => ((e - 1) as usize) << SLICE_SHIFT,
        }
    }
}

/// The segment base covering `addr`, or 0. wasm-only: this is what
/// `segment_of` resolves through instead of the pointer mask.
#[cfg(all(target_arch = "wasm32", not(miri)))]
#[inline]
pub fn base_of(addr: usize) -> usize {
    base_table::get(addr)
}

#[cfg(all(not(ra_small_profile), not(all(target_arch = "wasm32", not(miri)))))]
#[inline]
fn locate(addr: usize) -> Option<(usize, u32)> {
    let idx = addr >> WINDOW_SHIFT;
    if idx >= MAP_BITS {
        return None;
    }
    Some((idx / WORD_BITS, 1u32 << (idx % WORD_BITS)))
}

/// Register a segment's windows (Normal: one; Huge: every window the
/// reservation spans).
pub fn register(seg: *mut Segment) {
    register_range(seg.addr(), SEGMENT_SIZE);
}

/// Walk every representable window of `[base, base+size)`. Stops at the
/// first address the bitmap cannot name (above `ADDR_BITS`) and on
/// `checked_add` overflow: a range ending within one window of the top of
/// the address space used to wrap `a` back to 0 and walk the whole map
/// (OH-rusty_alloc-30). Reachable from a caller-supplied `manage_os_memory`
/// range, so bounded here as well as there.
#[cfg(all(not(ra_small_profile), not(all(target_arch = "wasm32", not(miri)))))]
fn walk_windows(base: usize, size: usize, mut op: impl FnMut(usize, u32)) {
    let mut a = base;
    let end = base.saturating_add(size.max(1));
    while a < end {
        match locate(a) {
            Some((w, bit)) => op(w, bit),
            None => break,
        }
        match a.checked_add(SEGMENT_SIZE) {
            Some(n) => a = n,
            None => break,
        }
    }
}

/// Register every 32 MiB window overlapped by `[base, base+size)`
/// (on wasm: every 64 KiB slice, in the base table).
pub fn register_range(base: usize, size: usize) {
    // One region: membership is the region's own bounds (`prim::fixed::
    // region_contains`), so there is nothing to record and the small
    // profile's range table -- two 256-byte arrays of `.bss`, a lock, and a
    // scan on every segment allocation and release -- folds away.
    if crate::ONE_REGION {
        return;
    }
    #[cfg(all(target_arch = "wasm32", not(miri)))]
    {
        base_table::set(base, size);
    }
    #[cfg(all(ra_small_profile, not(all(target_arch = "wasm32", not(miri)))))]
    {
        range_table::set(base, size);
    }
    #[cfg(all(not(ra_small_profile), not(all(target_arch = "wasm32", not(miri)))))]
    {
        walk_windows(base, size, |w, bit| {
            MAP[w].fetch_or(bit, Ordering::Release);
        });
    }
}

/// Unregister a segment's single window.
pub fn unregister(seg: *mut Segment) {
    unregister_range(seg.addr(), SEGMENT_SIZE);
}

/// Unregister every window of `[base, base+size)`.
pub fn unregister_range(base: usize, size: usize) {
    if crate::ONE_REGION {
        return;
    }
    #[cfg(all(target_arch = "wasm32", not(miri)))]
    {
        base_table::clear(base, size);
    }
    #[cfg(all(ra_small_profile, not(all(target_arch = "wasm32", not(miri)))))]
    {
        range_table::clear(base, size);
    }
    #[cfg(all(not(ra_small_profile), not(all(target_arch = "wasm32", not(miri)))))]
    {
        walk_windows(base, size, |w, bit| {
            MAP[w].fetch_and(!bit, Ordering::Release);
        });
    }
}

/// Whether `p` lies inside a registered window (`mi_is_in_heap_region`).
/// Best-effort by design: a racing segment release can flip the answer, so
/// this is a diagnostic, not a safety oracle.
pub fn contains(p: *const u8) -> bool {
    if crate::ONE_REGION {
        return crate::prim::fixed::region_contains(p.addr());
    }
    #[cfg(all(target_arch = "wasm32", not(miri)))]
    {
        base_table::get(p.addr()) != 0
    }
    #[cfg(all(ra_small_profile, not(all(target_arch = "wasm32", not(miri)))))]
    {
        range_table::contains(p.addr())
    }
    #[cfg(all(not(ra_small_profile), not(all(target_arch = "wasm32", not(miri)))))]
    {
        match locate(p.addr()) {
            Some((w, bit)) => MAP[w].load(Ordering::Acquire) & bit != 0,
            None => false,
        }
    }
}

/// Whether the small profile's range table has stopped being exact.
///
/// Always `false` on the bitmap and base-table representations, which cannot
/// overflow. P2 of `docs/plans/small-metal.md` uses it to tell a chip-shaped
/// workload (must stay exact) from the host battery (legitimately does not).
#[must_use]
pub fn range_table_overflowed() -> bool {
    if crate::ONE_REGION {
        return false; // two compares cannot overflow
    }
    #[cfg(all(ra_small_profile, not(all(target_arch = "wasm32", not(miri)))))]
    {
        range_table::overflowed()
    }
    #[cfg(not(all(ra_small_profile, not(all(target_arch = "wasm32", not(miri))))))]
    {
        false
    }
}
