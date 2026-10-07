//! Size-class (bin) geometry, mirroring upstream `page-queue.c` `mi_bin`.
//!
//! The geometry is the ABI-visible contract (`mi_good_size` G2-pins it against
//! the oracle): words 1..=8 get exact byte-multiples-of-8 bins; above that,
//! four bins per power of two (25% worst-case / 12.5% mean internal
//! fragmentation — consecutive bins step by at most 5/4).
//! Bin NUMBERING is internal and need not match upstream — only the
//! size→good_size mapping must (verified by the differential gate).

use crate::types::{BIN_FULL, BIN_HUGE, INTPTR_SIZE, MEDIUM_OBJ_SIZE_MAX, wsize_from_size};

/// Total number of page queues (bins 0..=BIN_HUGE plus the full queue).
pub const BIN_COUNT: usize = BIN_FULL + 1;

/// Direct-table entries: one per small wsize 0..=128 (`MI_PAGES_DIRECT`).
pub const PAGES_DIRECT: usize = crate::types::SMALL_WSIZE_MAX + 1;

/// Map a size to its bin index (`mi_bin`). Sizes above [`MEDIUM_OBJ_SIZE_MAX`]
/// map to [`BIN_HUGE`], the dedicated-segment path.
#[inline]
pub fn bin(size: usize) -> usize {
    let wsize = wsize_from_size(size);
    if wsize <= 1 {
        1
    } else if wsize <= 8 {
        // MI_ALIGN2W (the 64-bit default, G2-verified): round to even word
        // counts so every block ≥ 16 bytes is 16-aligned (max_align_t). Bins
        // 3/5/7 (24/40/56 B) do not exist.
        (wsize + 1) & !1
    } else if size > MEDIUM_OBJ_SIZE_MAX {
        BIN_HUGE
    } else {
        // Four bins per power of two: index by the top bit and the next two.
        let w = wsize - 1;
        // bsr(w).
        //
        // NOTE (2026-09-24, REFUTED): `(w | 1).leading_zeros()`, to let LLVM
        // see a non-zero input and drop the zero-input fallback (`mov $0x7f`
        // before the `bsr`) on the generic path's medium trip, removed the
        // `mov` and added a register copy and the `or`; LLVM keeps the
        // `leading_zeros` form either way (`bsr; xor $0x3f`) because the shift
        // count and the bin index are both derived from it. Opscan `big`
        // +1.00, `aligned` +0.69, `mixed` +0.72. Leave it as written.
        let b = (usize::BITS - 1 - w.leading_zeros()) as usize;
        ((b << 2) + ((w >> (b - 2)) & 0x03)) - 3
    }
}

/// Block size (bytes) served by `bin` (`_mi_bin_size` inverse of [`bin`]).
/// Meaningless for [`BIN_HUGE`]/[`BIN_FULL`].
#[inline]
pub const fn bin_size(bin: usize) -> usize {
    if bin <= 8 {
        return bin * INTPTR_SIZE;
    }
    // Garbage indices (`usize::MAX`) are 0, not `bin + 3` debug overflow or a
    // wrapping shift into a tiny class (OH-rusty_alloc-32). Meaningless for
    // [`BIN_HUGE`]/[`BIN_FULL`] as before; those still take this arm when
    // they fit the formula.
    let Some(t) = bin.checked_add(3) else {
        return 0;
    };
    let b = t >> 2;
    if b < 2 {
        return 0;
    }
    let shift = b - 2;
    match (5 + (t & 3)).checked_shl(shift as u32) {
        Some(w) => w.saturating_mul(INTPTR_SIZE),
        None => 0,
    }
}

/// `mi_good_size`: the size actually allocated for a request â€” the bin's block
/// size for binned sizes, page-rounded for huge ones.
#[inline]
pub fn good_size(size: usize) -> usize {
    if size <= MEDIUM_OBJ_SIZE_MAX {
        bin_size(bin(size))
    } else {
        crate::os::page_align_up(size)
    }
}

/// Is `x` a multiple of `align`, without a `div`?
///
/// `usize::is_multiple_of` on a RUNTIME divisor is a modulo, and a modulo is a
/// `div` — 20-40 cycles, not pipelined. Alignment is a power of two by the C
/// contract, and the aligned-allocation path already relies on that a few lines
/// away (`& !(align - 1)` appears twice in `malloc_aligned_at_slow`), so the
/// test is a mask.
///
/// The power-of-two check is kept rather than assumed, and it makes the
/// function total: a caller that passes a non-power-of-two gets `false`, which
/// is the CONSERVATIVE answer at every call site — each one falls back to the
/// general path rather than taking an in-place or same-bin shortcut. A mask
/// alone would be unsound there (`4 & (3-1) == 0` says "aligned to 3").
///
/// `align == 0` is not a power of two, so this returns `false`. Do **not**
/// lift zero to 1 first: that made every address report as aligned-to-zero
/// (Openheimer `oh_f04_is_aligned_to_is_total`, 2026-09-15) while
/// `malloc_aligned(_, 0)` correctly returns null.
#[inline(always)]
pub fn is_aligned_to(x: usize, align: usize) -> bool {
    // The power-of-two test is spelled out rather than `is_power_of_two()`:
    // that is a popcount, and without `popcnt` in the target features LLVM
    // emitted it as a 20-instruction SWAR bit count inside
    // `realloc_aligned_at` (measured 2026-09-16). `align & (align - 1) == 0`
    // is the same predicate in three instructions, and `align != 0` keeps
    // zero out — `0 & MAX == 0` would otherwise admit it (OH-rusty_alloc-5).
    let m = align.wrapping_sub(1);
    align != 0 && align & m == 0 && x & m == 0
}

/// `(addr + offset)` is aligned to `align`. Overflow of the add is `false`,
/// not a debug panic (OH-rusty_alloc-42 / OH-43).
#[inline]
pub fn is_aligned_at(addr: usize, offset: usize, align: usize) -> bool {
    match addr.checked_add(offset) {
        Some(s) => is_aligned_to(s, align),
        None => false,
    }
}

/// First address `p >= base` such that `(p + offset) % align == 0`.
///
/// `None` when the add overflows. Wrapping here would hand out a wild
/// interior pointer; the callers refuse with null instead (OH-rusty_alloc-6).
#[inline]
pub(crate) fn aligned_at_from(base: usize, offset: usize, align: usize) -> Option<usize> {
    debug_assert!(align.is_power_of_two() && align > 0);
    let sum = base.checked_add(offset)?.checked_add(align - 1)?;
    let rounded = sum & !(align - 1);
    rounded.checked_sub(offset)
}

/// `ceil(2^32 / odd)` for the four odd parts a bin size can have, indexed by
/// `odd >> 1`: 1 -> 0, 3 -> 1, 5 -> 2, 7 -> 3.
///
/// Derived, never transcribed. The `odd == 1` entry is exactly `2^32`, so the
/// same multiply-high returns `m` unchanged and needs no special case — which
/// is what makes the lookup branchless.
const RECIP32: [u64; 4] = [recip32(1), recip32(3), recip32(5), recip32(7)];

const fn recip32(d: u64) -> u64 {
    (1u64 << 32).div_ceil(d)
}

/// `n / bsize` for any `bsize` that is a bin's block size, WITHOUT a `div`.
///
/// A division by a runtime value is a real `div` — 20-40 cycles and not
/// pipelined. This removes it using a fact about this allocator's bin
/// geometry: **every bin size is `odd << k` with `odd` in {1, 3, 5, 7}**.
/// Bins 1..=8 are `bin * 8`, and above that `bin_size` is
/// `((5 + m) << (b - 2)) * 8` with `m` in 0..=3, so the odd factor is only
/// ever 5, 3 (from 6), 7, or 1 (from 8). Shift the power of two out and the
/// divisor is one of four constants.
///
/// **Branchless, by table rather than by `match`.** A four-arm match was the
/// first form, and it cost a three-way compare tree plus THREE `movabs; mul;
/// shr` magic multiplies — one per arm, so nine of those instructions were
/// dead on any given call, in both `free_general` and `usable_size_slow`.
/// Indexing `RECIP32` by the odd part costs one load and keeps one multiply.
///
/// Unlike the modular-inverse trick ([`exact_div_by_block_size`],
/// `odd_mod_inverse`), this is correct for an arbitrary dividend — it does not
/// require `n` to be a multiple of `bsize` — which is what makes it usable on
/// the interior pointers `unalign` and `usable_size` are handed.
///
/// # Bounds
/// The reciprocals are 32-bit, so the identity holds for `n >> k` below 2^30
/// (the tightest arm, `odd == 5`, is exact to 2^30; the others reach further).
/// Every caller is bounded by page geometry — the largest dividend any of them
/// can present is one page payload, `MEDIUM_PAGE_SLICES * SEGMENT_SLICE_SIZE`
/// = 2^19, and `k >= 3` shrinks it further. Debug builds assert it.
#[inline(always)]
pub(crate) fn div_by_block_size(n: usize, bsize: usize) -> usize {
    let k = bsize.trailing_zeros();
    let odd = bsize >> k;
    let m = (n >> k) as u64;
    debug_assert!(
        matches!(odd, 1 | 3 | 5 | 7),
        "bin_size {bsize} has odd part {odd}, outside the {{1,3,5,7}} this relies on"
    );
    debug_assert!(
        m < (1 << 30),
        "div_by_block_size: {n} >> {k} exceeds the bound the 32-bit reciprocals are exact over"
    );
    ((m * RECIP32[(odd >> 1) & 3]) >> 32) as usize
}

/// `n / bsize` when `n` is a KNOWN MULTIPLE of `bsize`.
///
/// Cheaper than [`div_by_block_size`] and strictly less general. Dividing by a
/// constant is a multiply-high plus a shift (`movabs; mul; shr`, and `mul`
/// clobbers `rdx`); dividing an exact multiple is the modular inverse, a
/// single `imul` with no high half and no shift.
///
/// The four inverses are derived at compile time from `odd_mod_inverse`, not
/// transcribed — a mistyped digit here would produce plausible garbage rather
/// than a compile error.
///
/// # Correctness
/// `n` **must** be a multiple of `bsize`. For anything else this returns a
/// meaningless value rather than a rounded-down quotient: the inverse maps
/// non-multiples across the whole 64-bit range. Use [`div_by_block_size`] on
/// interior pointers. Debug builds assert the precondition.
#[inline(always)]
pub(crate) fn exact_div_by_block_size(n: usize, bsize: usize) -> usize {
    const INV3: usize = crate::page::odd_mod_inverse(3);
    const INV5: usize = crate::page::odd_mod_inverse(5);
    const INV7: usize = crate::page::odd_mod_inverse(7);

    let k = bsize.trailing_zeros();
    let odd = bsize >> k;
    debug_assert!(
        bsize != 0 && n.is_multiple_of(bsize),
        "exact_div_by_block_size: {n} is not a multiple of {bsize}"
    );
    let n = n >> k;
    match odd {
        1 => n,
        3 => n.wrapping_mul(INV3),
        5 => n.wrapping_mul(INV5),
        7 => n.wrapping_mul(INV7),
        // Diverges for the same reason `div_by_block_size`'s arm does: a
        // computing fallback lets LLVM merge the arms back into one runtime
        // division.
        _ => unreachable!("block_size {bsize} has odd part {odd}, not in {{1,3,5,7}}"),
    }
}

/// `max(1, 4096 / bin_size(bin))` for every bin — `page_extend`'s batch size,
/// resolved at compile time. **Measured and NOT adopted** — see the refutation
/// at the `let take` line in `page::page_extend`. Kept because the measurement
/// is worth more than the deletion: it is the table anyone will reach for.
///
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned_at_from_refuses_overflow() {
        assert!(aligned_at_from(0x1000, usize::MAX, 8).is_none());
        assert_eq!(aligned_at_from(0x1000, 16, 64), Some(0x1030));
        assert_eq!(bin_size(usize::MAX), 0);
        assert_eq!(bin_size(0), 0);
    }

    #[test]
    fn bin_size_inverts_bin() {
        // For every bin reachable from a size, bin_size must be the largest
        // size mapping to that bin, and good_size must be idempotent.
        for size in 1..=MEDIUM_OBJ_SIZE_MAX {
            let b = bin(size);
            let bs = bin_size(b);
            assert!(bs >= size, "bin_size({b}) = {bs} < size {size}");
            assert_eq!(bin(bs), b, "bin_size({b}) = {bs} maps to bin {}", bin(bs));
            assert_eq!(good_size(good_size(size)), good_size(size));
        }
    }

    #[test]
    fn known_size_classes() {
        // Spot values from the mimalloc paper / types.h (G2 pins the full range
        // against the oracle binary; these catch formula regressions offline).
        //
        // Every entry here must be BIN geometry — pure arithmetic, identical on
        // every target. Sizes above MEDIUM_OBJ_SIZE_MAX are page-rounded and so
        // depend on the runtime page size; they belong in
        // `good_size_above_binned_range_is_page_rounded`, not in this table.
        for (size, good) in [
            (1, 8),
            (8, 8),
            (9, 16),
            (17, 32), // ALIGN2W: no 24-byte bin
            (24, 32),
            (33, 48), // no 40-byte bin
            (56, 64), // no 56-byte bin
            (64, 64),
            (65, 80),
            (72, 80),
            (80, 80),
            (100, 112),
            (128, 128),
            (129, 160),
            (256, 256),
            (257, 320),
            (1024, 1024),
            (1025, 1280),
            (4097, 5120),
            (65536, 65536), // last binned size
        ] {
            // These are pinned against the mimalloc v2.4.5 oracle, which
            // exists only at the shipped geometry. `good_size` bins up to
            // MEDIUM_OBJ_SIZE_MAX and PAGE-ROUNDS above it, so under a
            // different geometry (P2, `docs/plans/small-metal.md`) the
            // largest rows here move out of the binned range and stop being
            // oracle facts. Filtering keeps every row the two geometries
            // share instead of disabling the whole fixture — and the
            // page-rounded half is already covered, as a PROPERTY rather
            // than a literal, by `good_size_above_binned_range_is_page_rounded`
            // below.
            if size > crate::types::MEDIUM_OBJ_SIZE_MAX {
                continue;
            }
            assert_eq!(good_size(size), good, "good_size({size})");
        }
    }

    #[test]
    fn good_size_above_binned_range_is_page_rounded() {
        // Above MEDIUM_OBJ_SIZE_MAX, good_size rounds to whole OS pages, so the
        // expected value is a function of the RUNTIME page size — 4 KiB on
        // x86-64 Linux/Windows, but 16 KiB on Apple Silicon (and on any
        // CONFIG_ARM64_16K_PAGES kernel). Assert the property, never a literal:
        // a hardcoded 4 KiB expectation here fails on aarch64-apple-darwin.
        let ps = crate::os::page_size();
        assert!(
            ps.is_power_of_two() && ps >= 4096,
            "implausible page size {ps}"
        );

        for size in [
            MEDIUM_OBJ_SIZE_MAX + 1,
            MEDIUM_OBJ_SIZE_MAX + ps - 1,
            2 * MEDIUM_OBJ_SIZE_MAX,
            1024 * 1024 + 1,
        ] {
            let g = good_size(size);
            assert_eq!(g, crate::os::page_align_up(size), "good_size({size})");
            assert!(g >= size, "good_size({size}) = {g} < size");
            assert_eq!(g % ps, 0, "good_size({size}) = {g} is not page-aligned");
            assert!(g - size < ps, "good_size({size}) = {g} over-rounded");
            assert_eq!(good_size(g), g, "good_size not idempotent at {g}");
        }
    }

    #[test]
    fn fragmentation_bound() {
        // 4 linear bins per doubling: consecutive bin sizes step by ≤ 5/4, so
        // worst-case waste is 25% (hit just above each bin boundary).
        for size in 65..=MEDIUM_OBJ_SIZE_MAX {
            let g = good_size(size);
            assert!(g - size <= size / 4 + 16, "waste {g}-{size} too large");
        }
    }
}

#[cfg(test)]
mod div_helper_tests {
    use super::*;

    /// `div_by_block_size` IS wired into `unalign` (D11). The identity it rests on
    /// is worth pinning independently of that: if a future bin geometry produced
    /// an odd factor outside {1,3,5,7}, the branchless `RECIP32` lookup would
    /// silently index the wrong reciprocal and return a wrong quotient rather
    /// than diverge — which is exactly what this test exists to prevent.
    /// (Debug builds also assert it at the call.)
    #[test]
    fn every_bin_size_has_odd_part_in_1_3_5_7() {
        for bin in 1..BIN_COUNT {
            let bs = bin_size(bin);
            if bs == 0 {
                continue;
            }
            let odd = bs >> bs.trailing_zeros();
            assert!(
                matches!(odd, 1 | 3 | 5 | 7),
                "bin {bin} size {bs} has odd part {odd}"
            );
        }
    }

    /// And that the helper equals real division for every bin size over a
    /// dividend range that spans whole blocks and interior offsets alike.
    #[test]
    fn div_by_block_size_equals_real_division() {
        for bin in 1..BIN_COUNT {
            let bs = bin_size(bin);
            if bs == 0 {
                continue;
            }
            for n in [0usize, 1, bs - 1, bs, bs + 1, 3 * bs, 3 * bs + 7, 1 << 20] {
                assert_eq!(
                    div_by_block_size(n, bs),
                    n / bs,
                    "div_by_block_size({n}, {bs})"
                );
            }
        }
    }
}

#[cfg(test)]
mod exact_div_tests {
    use super::{bin_size, div_by_block_size, exact_div_by_block_size};

    /// The inverses are derived, so prove the derivation: for every bin size
    /// and every exact multiple, the cheap form must equal real division.
    #[test]
    fn exact_form_matches_real_division_for_every_bin() {
        for bin in 1..=40usize {
            let bs = bin_size(bin);
            if bs == 0 {
                continue;
            }
            for k in [0usize, 1, 2, 3, 17, 255, 4095] {
                let n = k * bs;
                assert_eq!(
                    exact_div_by_block_size(n, bs),
                    n / bs,
                    "bin {bin} bsize {bs} multiple {k}"
                );
            }
        }
    }

    /// The two forms agree wherever the exact one is legal, and the general
    /// one keeps working where it is not — the distinction the doc claims.
    #[test]
    fn general_form_agrees_on_multiples_and_survives_non_multiples() {
        for bin in 1..=40usize {
            let bs = bin_size(bin);
            if bs == 0 {
                continue;
            }
            for k in [1usize, 9, 100] {
                let n = k * bs;
                assert_eq!(div_by_block_size(n, bs), exact_div_by_block_size(n, bs));
            }
            // Interior offsets: only the general form is meaningful here.
            for d in [1usize, bs / 2, bs - 1] {
                if d == 0 || d >= bs {
                    continue;
                }
                let n = 3 * bs + d;
                assert_eq!(div_by_block_size(n, bs), 3, "bin {bin} offset {d}");
            }
        }
    }
}
