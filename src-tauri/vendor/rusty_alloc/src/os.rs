//! OS memory layer over [`crate::prim`] (mirrors upstream `src/os.c`).
//!
//! Adds to prim: cached configuration, page-size rounding, alignment
//! validation, and the purge policy (decommit vs reset — `mi_option_purge_decommits`,
//! wired to the options table in M7; until then callers pass the policy).

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::prim::{self, PrimError};

/// A block of OS memory returned by [`alloc_aligned`].
#[derive(Debug, Clone, Copy)]
pub struct OsBlock {
    /// Base address — aligned as requested; also the free handle.
    pub ptr: *mut u8,
    /// Rounded (page-multiple) size actually reserved.
    pub size: usize,
    /// Backed by large pages.
    pub is_large: bool,
    /// Contents guaranteed zero.
    pub is_zero: bool,
}

static PAGE_SIZE: AtomicUsize = AtomicUsize::new(0);
static ALLOC_GRANULARITY: AtomicUsize = AtomicUsize::new(0);
static LARGE_PAGE_SIZE: AtomicUsize = AtomicUsize::new(usize::MAX); // MAX = uninit (0 is a valid value)

fn config_init() {
    let cfg = prim::mem_init();
    // Benign race: idempotent stores of identical values.
    PAGE_SIZE.store(cfg.page_size, Ordering::Relaxed);
    ALLOC_GRANULARITY.store(cfg.alloc_granularity, Ordering::Relaxed);
    LARGE_PAGE_SIZE.store(cfg.large_page_size, Ordering::Relaxed);
}

/// OS page size (cached).
pub fn page_size() -> usize {
    let v = PAGE_SIZE.load(Ordering::Relaxed);
    if v != 0 {
        return v;
    }
    config_init();
    PAGE_SIZE.load(Ordering::Relaxed)
}

/// Reservation granularity (64 KiB on Windows, page size elsewhere; cached).
pub fn alloc_granularity() -> usize {
    let v = ALLOC_GRANULARITY.load(Ordering::Relaxed);
    if v != 0 {
        return v;
    }
    config_init();
    ALLOC_GRANULARITY.load(Ordering::Relaxed)
}

/// Large-page size, 0 when unavailable (cached).
pub fn large_page_size() -> usize {
    let v = LARGE_PAGE_SIZE.load(Ordering::Relaxed);
    if v != usize::MAX {
        return v;
    }
    config_init();
    LARGE_PAGE_SIZE.load(Ordering::Relaxed)
}

/// Round `size` up to a whole number of OS pages (never 0).
pub fn page_align_up(size: usize) -> usize {
    let ps = page_size();
    // `div_ceil(ps) * ps` is a division by a RUNTIME value — a real `div`, and
    // this function is called from `purge`, `alloc_aligned` and the reservation
    // paths, so LLVM emitted one into each: 26 divide instructions across the
    // binary from this one line. Rounding up to a power of two is an add and a
    // mask instead.
    //
    // The mask is built from the page size's TRAILING ZEROS rather than from
    // `ps - 1` directly, which makes the power-of-two property structural
    // instead of assumed. `ps` is read from an atomic cache of an OS value, so
    // nothing in the type system says it is a power of two — an earlier
    // version asserted it and Kani (correctly) refused the proof, because
    // `ps - 1` underflows at `ps == 0` and the mask is only a round-up for a
    // power of two. `1 << (tz & 63)` is a power of two for EVERY input,
    // including 0 (`trailing_zeros` is 64 there, masked to 0, giving mask 0 and
    // a returned `size.max(1)`), so the function is total and
    // `good_size_never_shrinks_a_request` verifies again.
    //
    // On any real platform `1 << ps.trailing_zeros() == ps`, so this is the
    // same round-up it always was, for one `tzcnt` and one `shl`.
    let mask = (1usize << (ps.trailing_zeros() & 63)) - 1;
    let base = size.max(1);
    // Checked add: wrapping here is a silent shrink (OH-rusty_alloc-10).
    // `usize::MAX + 4095` wraps to a small page-aligned value in release, so
    // `good_size(usize::MAX)` would promise 0. Unrepresentable round-up keeps
    // `base` — never shrinks, never panics. "Never 0": `size == 0` uses base 1.
    match base.checked_add(mask) {
        Some(s) => s & !mask,
        None => base,
    }
}

/// Reserve (and optionally commit) an aligned block of OS memory.
///
/// `alignment` must be a power of two ≥ page size; `size` is rounded up to
/// pages. This is what segments (M3) and arenas (M6) sit on. A non-power-of-two
/// (or zero) alignment is `Err`, not `assert!` (OH-rusty_alloc-28).
pub fn alloc_aligned(
    size: usize,
    alignment: usize,
    commit: bool,
    allow_large: bool,
) -> Result<OsBlock, PrimError> {
    // EINVAL: hostile / mistaken callers hit the `Result`, not a panic.
    if !alignment.is_power_of_two() {
        return Err(22);
    }
    let alignment = alignment.max(page_size());
    let size = page_align_up(size);
    // Unrepresentable `size + alignment` is Err, not `size + try_alignment`
    // debug overflow in the prim aligned-reserve dance (OH-rusty_alloc-29).
    if size.checked_add(alignment).is_none() {
        return Err(12);
    }
    // On a platform whose `free` cannot return memory (wasm), a freed block's
    // only afterlife is adoption as SEGMENT_SIZE arena chunks (see [`free`]).
    // A segment-aligned block with a ragged size would leave a sub-chunk tail
    // with no such afterlife — up to SEGMENT_SIZE-page_size lost per HUGE
    // alloc/free cycle, which is an unbounded leak, not an overhead. Rounding
    // the size up makes every adoptable block exactly chunk-granular: the
    // overshoot is committed linear memory (wasm has no lazy commit), but it
    // is bounded by one chunk per LIVE huge block and fully recycled on free.
    // `alignment >= SEGMENT_SIZE` is precisely the segment/huge reservation
    // paths; descriptor-sized allocations keep their page granularity.
    let size = if !prim::FREE_RETURNS_MEMORY && alignment >= crate::types::SEGMENT_SIZE {
        size.checked_next_multiple_of(crate::types::SEGMENT_SIZE)
            .ok_or(12u32)?
    } else {
        size
    };
    // SAFETY: size is page-multiple and > 0, alignment a power of two; the
    // returned mapping is owned by the OsBlock we hand out.
    let a = unsafe { prim::alloc(size, alignment, commit, allow_large)? };
    // `% alignment` by a RUNTIME divisor is a real `div`. Debug-only, but
    // `debug_checks` runs the whole datasweep corpus, so it is worth the mask
    // — same substitution as the five release sites (D7).
    // On a fixed region alignment is measured from the region's base, which
    // is where its segments stride from (`crate::REGION_STRIDES`). Computed
    // outside the assert so the check has no side effect to be skipped.
    let from_origin = if crate::REGION_STRIDES {
        (a.ptr as usize).wrapping_sub(crate::prim::fixed::stride_base())
    } else {
        a.ptr as usize
    };
    debug_assert!(
        crate::bins::is_aligned_to(from_origin, alignment),
        "prim returned misaligned block"
    );
    Ok(OsBlock {
        ptr: a.ptr,
        size,
        is_large: a.is_large,
        is_zero: a.is_zero,
    })
}

/// Whether `[ptr, ptr+size)` is reserved or committed by the OS.
pub fn range_is_reserved(ptr: *const u8, size: usize) -> bool {
    prim::range_is_reserved(ptr, size)
}

/// Release a block from [`alloc_aligned`].
///
/// # Safety
/// `block` must come from [`alloc_aligned`], be unfreed, and have no live
/// references into it.
pub unsafe fn free(block: OsBlock) -> Result<(), PrimError> {
    // Where the prim cannot return memory to the host (wasm: linear memory
    // never shrinks), dropping the block would leave it mapped but
    // unreachable — the allocator would simply forget it. Adopt it as arena
    // chunks instead, which makes the arena the free list the no-op prim
    // `free` relies on. Compiled out entirely where `free` works.
    if !prim::FREE_RETURNS_MEMORY && crate::arena::adopt_os_block(block.ptr, block.size).is_some() {
        return Ok(());
    }
    // Slice-granular fallback (F2): blocks adoption cannot take — heap
    // descriptors are page-granular, and on wasm a page IS a slice — recycle
    // through the slice pool instead of hitting the no-op prim free. This
    // closes the residual descriptor leak recorded in wasm-recycling.md.
    #[cfg(all(target_arch = "wasm32", not(miri)))]
    if crate::slice_pool::free_range(block.ptr.expose_provenance(), block.size) {
        return Ok(());
    }
    // SAFETY: forwarded contract (whole mapping base + size).
    unsafe { prim::free(block.ptr, block.size) }
}

/// Commit a page-aligned sub-range of a reserved block. Returns known-zero.
///
/// # Safety
/// Range must lie within a live block from [`alloc_aligned`], page-aligned.
pub unsafe fn commit(ptr: *mut u8, size: usize) -> Result<bool, PrimError> {
    // SAFETY: forwarded contract.
    unsafe { prim::commit(ptr, page_align_up(size)) }
}

/// Decommit a page-aligned sub-range. Returns whether recommit is required.
///
/// # Safety
/// As [`commit`]; contents are lost.
pub unsafe fn decommit(ptr: *mut u8, size: usize) -> Result<bool, PrimError> {
    // SAFETY: forwarded contract.
    unsafe { prim::decommit(ptr, page_align_up(size)) }
}

/// Purge a range per policy: `purge_decommits` → decommit (returns whether
/// recommit is needed); otherwise reset (stays committed, contents undefined).
///
/// # Safety
/// As [`commit`]; contents are lost either way.
pub unsafe fn purge(ptr: *mut u8, size: usize, purge_decommits: bool) -> Result<bool, PrimError> {
    if purge_decommits {
        // SAFETY: forwarded contract.
        unsafe { decommit(ptr, size) }
    } else {
        // SAFETY: forwarded contract.
        unsafe { prim::reset(ptr, page_align_up(size))? };
        Ok(false)
    }
}

/// Toggle no-access protection on a page-aligned range (guard pages).
///
/// # Safety
/// As [`commit`]; caller must not touch a protected range.
pub unsafe fn protect(ptr: *mut u8, size: usize, on: bool) -> Result<(), PrimError> {
    // SAFETY: forwarded contract.
    unsafe { prim::protect(ptr, page_align_up(size), on) }
}
