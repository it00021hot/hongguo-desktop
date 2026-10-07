//! Segments: 32 MiB-aligned reservations sliced into 64 KiB slices (M3 subset
//! of upstream `segment.c`). The alignment IS the addressing scheme —
//! `ptr → segment` is a mask, `ptr → page` two shifts and a table walk.
//!
//! M3 adds SPAN RECLAMATION: freed page spans return to a per-segment
//! first-fit free list with left/right coalescing, so pages of any size class
//! can reuse the space (M2 leaked slices to their first size class forever).
//!
//! Span invariants (what makes coalescing O(1)):
//! - The carved region `[HEADER_SLICES, next_free_slice)` is partitioned into
//!   spans; every span's FIRST slot has `slice_offset == 0`, and slot
//!   `block_size > 0` ⟺ the span is a live page (0 ⟺ free span).
//! - Every span's LAST slot's `slice_offset` points back to its first slot, so
//!   the left neighbor of any span is found by one follow-back.
//! - Free spans link through `next`/`prev` into `Segment::free_spans`.
//!
//! Eager commit (the oracle's default); decommit/purge of free spans is the
//! purge policy work of M7.

use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::os;
use crate::page::Page;
use crate::prim::PrimError;
use crate::segment_map;
use crate::types::{SEGMENT_SIZE, SEGMENT_SLICE_SIZE, SLICES_PER_SEGMENT};

/// What a segment holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SegmentKind {
    /// Sliced segment holding small/medium/large pages.
    Normal = 0xA110C,
    /// Dedicated reservation for one huge block (> 16 MiB).
    Huge = 0x40BE,
}

/// Segment header — occupies slice 0 of the reservation.
pub struct Segment {
    /// Kind tag (doubles as a magic for `debug_checks`).
    pub kind: SegmentKind,
    /// Total reserved bytes (== SEGMENT_SIZE for Normal; the whole reservation
    /// for Huge — needed to free it).
    pub total_size: usize,
    /// Bump cursor: slices at/after this index have never been carved.
    pub next_free_slice: u32,
    /// Live pages carved from this segment.
    pub used_pages: u32,
    /// Owning thread id (`prim::thread_id`); 0 = abandoned. Gates the
    /// local-vs-remote routing in `alloc::free`.
    pub thread_id: AtomicUsize,
    /// The mapping's never-carved region is known zero (fresh OS memory).
    /// False for recycled arena chunks — bump-fresh spans then are NOT zero.
    pub mem_is_zero: bool,
    /// Any span of this segment was purged (decommitted) at some point. The
    /// segment must be RE-COMMITTED in full before it goes back to an arena:
    /// the next tenant bump-allocates from it and would otherwise touch
    /// decommitted pages (Windows access violation — the M8 defect).
    pub purged_any: bool,
    /// This segment carries a PROT_NONE guard page (guarded allocation). The
    /// protection MUST be lifted before the memory is recycled through an
    /// arena, or the next tenant faults on a page it legitimately owns —
    /// the M8 P0. (OS-released memory is unmapped, so this only bites the
    /// arena path, which is exactly why "arenas off" was the one clean arm.)
    pub guarded: bool,
    /// Next segment in the owning heap's list (reused as the global
    /// abandoned-list link while abandoned).
    pub next: *mut Segment,
    /// Head of the free-span list (slots inside `pages`).
    pub free_spans: *mut Page,
    /// For each slice, the byte offset from THIS SEGMENT'S BASE to the `Page`
    /// that owns it.
    ///
    /// It is the same fact `Page::slice_offset` already carries, in the one
    /// form the free path can use in two instructions instead of five.
    /// Resolving a pointer to its page used to mean: multiply the slice index
    /// by `size_of::<Page>()` — 88, not a power of two, so a real `imul` —
    /// add it to the slot array to reach the slot, load the slot's
    /// `slice_offset`, and subtract it. Four dependent steps to reach a page
    /// whose address is a pure function of the slice index. Indexed by a
    /// `u32` array instead, the whole thing is
    /// `mov off(%seg,%idx,4); lea (%seg,%off)`.
    ///
    /// `free` runs this on every call, so those two instructions are the
    /// second-largest item in it. The table costs 2 KiB in a 32 MiB segment
    /// header that has ~20 KiB spare (0.006% of the segment) and is written
    /// only when a span is carved.
    ///
    /// It duplicates state, which is a real hazard, so `debug_validate_segment`
    /// checks the two agree on every slice of every span it walks.
    pub page_off: [u32; SLICES_PER_SEGMENT],
    /// One metadata slot per slice; a span's slot is its first slice.
    pub pages: [Page; SLICES_PER_SEGMENT],
}

/// Byte offset from a segment's base to its slot array — the base value stored
/// in [`Segment::page_off`].
pub const PAGES_OFFSET: usize = core::mem::offset_of!(Segment, pages);

/// The byte offset a slice owned by the span starting at `start` must record.
#[inline(always)]
pub const fn page_off_for(start: usize) -> u32 {
    (PAGES_OFFSET + start * core::mem::size_of::<Page>()) as u32
}

const _: () = assert!(
    core::mem::size_of::<Segment>() <= SEGMENT_SLICE_SIZE,
    "segment header must fit in slice 0"
);

/// Slices reserved for the header.
pub const HEADER_SLICES: usize = 1;

/// Usable slices in a Normal segment.
pub const USABLE_SLICES: usize = SLICES_PER_SEGMENT - HEADER_SLICES;

// `LARGE_OBJ_SIZE_MAX` is the byte capacity of exactly this carved region —
// the routing constant and the geometry must never drift apart, or malloc
// would either send span-sized requests to dedicated huge segments (the
// segment tax, docs/plans/segment-tax.md) or ask `span_from_segments` for a
// span no segment can hold.
const _: () = assert!(crate::types::LARGE_OBJ_SIZE_MAX == USABLE_SLICES * SEGMENT_SLICE_SIZE);

/// Recover the owning segment from any pointer into it. `with_addr` keeps
/// provenance so the mask trick is miri-clean.
///
/// Two arms on one predicate. Hosted segments are `SEGMENT_SIZE`-aligned and
/// the mask recovers the base from the address. On a fixed region
/// (`crate::REGION_STRIDES`) segments stride from the region's BASE, so the
/// mask is applied to the offset from it: that is what lets a firmware's
/// region be `MAX_ALIGN_SIZE`-aligned rather than segment-aligned, and it
/// returns the gap the linker used to leave in front of the aligned static —
/// 24,148 bytes on the ESP32-S3 that measured it
/// (`docs/plans/finished/region-alignment-dissolve.md`).
#[cfg(not(all(target_arch = "wasm32", not(miri))))]
#[inline]
pub fn segment_of(p: *mut u8) -> *mut Segment {
    if crate::REGION_STRIDES {
        // COST, on the ESP32-S3 (the bench firmware's `dealloc`, which is
        // `free` inlined, objdump of the shipped image, 2026-09-09): the mask
        // is `l32r 0xffff0000; and` — two instructions — and this is
        // `l32r &REGION_BASE; l32i; sub; l32r 0xffff; and; sub`, of which the
        // page-index computation reuses the first `sub`, so the free path
        // grows from 36 to 39 instructions. The board prices that at 9-17 ns
        // per alloc/free pair (595 / 833 / 1011 / 1134 against 578 / 820 /
        // 997 / 1125 ns with `--cfg ra_aligned_region`, which is the mask),
        // for 24,144 bytes of stack returned on the firmware that asked.
        // (The esp LLVM backend materialises `0xffff` through the literal
        // pool rather than as an `extui`; that is one instruction of the
        // three and not worth a special case.)
        // `wrapping_sub`: a pointer below the base is foreign, and the answer
        // for a foreign pointer is garbage here exactly as the mask's is; the
        // `debug_checks` guard in `free` is what refuses those.
        let base = crate::prim::fixed::stride_base();
        let off = p.addr().wrapping_sub(base) & (SEGMENT_SIZE - 1);
        p.with_addr(p.addr().wrapping_sub(off)).cast()
    } else {
        // NOTE (2026-08-21): the GEP form `p.wrapping_sub(p.addr() &
        // (SEGMENT_SIZE - 1))` — proposed because `with_addr` is an integer
        // round-trip LLVM is conservative about — measured +1.00 Ir on EVERY
        // malloc and free (small 59.32 -> 60.32, batch_lifo 59.53 -> 60.53).
        // The round-trip is free here and the subtraction is not. Do not
        // retry.
        p.with_addr(p.addr() & !(SEGMENT_SIZE - 1)).cast()
    }
}

/// wasm: segments are SLICE-aligned, not SEGMENT_SIZE-aligned (F2,
/// docs/plans/segment-tax.md), so the mask cannot recover the base — the
/// slice-granular table in `segment_map` does. One load; this platform's
/// free path is plain Rust (the x86 asm fast path is cfg'd out) and
/// single-threaded, so the table lookup is the whole cost of dissolving the
/// alignment constraint that made every ragged reservation strand its tail.
#[cfg(all(target_arch = "wasm32", not(miri)))]
#[inline]
pub fn segment_of(p: *mut u8) -> *mut Segment {
    let base = crate::segment_map::base_of(p.addr());
    debug_assert!(
        base != 0,
        "segment_of: unmapped wasm address {:#x}",
        p.addr()
    );
    p.with_addr(base).cast()
}

/// Reserve backing memory for a segment of `want` bytes.
///
/// Native: an eagerly committed, SEGMENT_SIZE-aligned OS block, exactly as
/// always (on a fixed region, aligned on the region's own segment strides —
/// `crate::REGION_STRIDES`). wasm: the slice pool first — recycled memory at 64 KiB
/// granularity — then a fresh slice-aligned reservation sized to the SLICE
/// round of `want`, not the chunk round. This is where the segment tax
/// dies: a 33 MiB huge block reserves 33.06 MiB instead of 64 MiB, and on
/// free every slice of it becomes serviceable again.
fn reserve_backing(want: usize) -> Result<(*mut u8, usize, bool), PrimError> {
    #[cfg(all(target_arch = "wasm32", not(miri)))]
    {
        let size = os::page_align_up(want); // wasm page == slice == 64 KiB
        debug_assert!(size.is_multiple_of(SEGMENT_SLICE_SIZE));
        if let Some(addr) = crate::slice_pool::alloc_run(size / SEGMENT_SLICE_SIZE) {
            // Provenance: pool addresses were exposed when their blocks were
            // freed (`expose_provenance` at the free sites).
            return Ok((core::ptr::with_exposed_provenance_mut(addr), size, false));
        }
        let b = os::alloc_aligned(size, SEGMENT_SLICE_SIZE, true, false)?;
        Ok((b.ptr, b.size, b.is_zero))
    }
    #[cfg(not(all(target_arch = "wasm32", not(miri))))]
    {
        let b = os::alloc_aligned(want, SEGMENT_SIZE, true, false)?;
        Ok((b.ptr, b.size, b.is_zero))
    }
}

/// Resolve a block pointer to its page slot (follows `slice_offset` back to
/// the span start).
///
/// # Safety
/// `p` must point into a live segment owned by this allocator.
#[inline]
pub unsafe fn page_of(seg: *mut Segment, p: *mut u8) -> *mut Page {
    // NOTE (2026-08-21): deriving this as `p.addr() & (SEGMENT_SIZE - 1)` —
    // the same value without the subtraction, since `seg` is `p` masked —
    // measured FLAT. LLVM already sees through the mask here.
    let idx = (p.addr() - seg.addr()) / SEGMENT_SLICE_SIZE;
    debug_assert!(
        idx < SLICES_PER_SEGMENT,
        "page_of: slice index out of range"
    );
    // SAFETY: `p` lies inside this 32 MiB segment (caller contract), so
    // `idx < SLICES_PER_SEGMENT` BY CONSTRUCTION — the offset cannot exceed
    // SEGMENT_SIZE and the divisor is SEGMENT_SLICE_SIZE. Indexing the array
    // with `[idx]` instead makes LLVM emit a bounds check it cannot discharge
    // (the bound is a property of the caller's contract, not of the
    // arithmetic), and this is the hottest function in the allocator: two
    // resolutions per free. `add` on the base pointer keeps the same
    // provenance and the same address, with the check kept under
    // `debug_checks`.
    unsafe {
        // ONE scale-4 load and an add. The table records, per slice, the byte
        // offset from this segment's base to the page that owns it, so the
        // slot address, the `imul` by `size_of::<Page>()` and the
        // `slice_offset` follow-back all disappear — see `Segment::page_off`.
        let tab: *const u32 = (&raw const (*seg).page_off).cast();
        let off = *tab.add(idx) as usize;
        // The two representations must agree. This is the check that makes the
        // duplicated state safe to keep, and it is free in release.
        debug_assert!(
            {
                let base: *mut Page = (&raw mut (*seg).pages).cast();
                let slot = base.add(idx);
                let back = (*slot).slice_offset as usize * slot_stride();
                back <= idx * core::mem::size_of::<Page>()
                    && slot.cast::<u8>().sub(back).cast::<Page>()
                        == seg.cast::<u8>().add(off).cast::<Page>()
            },
            "page_of: page_off table disagrees with slice_offset"
        );
        seg.cast::<u8>().add(off).cast::<Page>()
    }
}

/// Byte distance between adjacent slice slots. No longer the unit of
/// [`Page::slice_offset`], which counts slices; this scales it where a byte
/// distance is actually wanted.
#[inline]
pub const fn slot_stride() -> usize {
    core::mem::size_of::<Page>()
}

// A span can start at most SLICES_PER_SEGMENT-1 slots before an interior slot,
// so the slice distance must fit the u16 it is stored in. This had far less
// headroom when the field held bytes: 511 * 88 = 44,968, inside u16 but only
// just — a larger `Page` or segment would have overflowed it silently.
const _: () = assert!(
    SLICES_PER_SEGMENT - 1 <= u16::MAX as usize,
    "slice_offset (slices) must fit in u16"
);

/// Payload start of the page whose slot index is `idx`.
///
/// # Safety
/// `idx` must be a span-start slice of a live segment.
#[inline]
pub unsafe fn page_area(seg: *mut Segment, idx: usize) -> *mut u8 {
    // SAFETY: stays within the segment reservation by the idx contract.
    unsafe { seg.cast::<u8>().add(idx * SEGMENT_SLICE_SIZE) }
}

/// Slot index of a page within its segment.
///
/// # Safety
/// `page` must be a slot inside `seg`'s table.
#[inline]
pub unsafe fn page_index(seg: *mut Segment, page: *mut Page) -> usize {
    // From the SLOT pointer this is `(page - base) / size_of::<Page>()`, and
    // `size_of::<Page>()` is 88 — `8 * 11`, not a power of two — so LLVM emits
    // `movabs $0x2e8ba2e8ba2e8ba3; mul; shr`, a 3-instruction magic multiply
    // with a 10-byte immediate. That is `docs/opps.md` #1, and it appeared at
    // all five call sites.
    //
    // `Page::area` is `seg + idx * SEGMENT_SLICE_SIZE` (written by `span_mark`
    // for every carved span, free or allocated, and by `huge_alloc` for its
    // one slot), and SEGMENT_SLICE_SIZE **is** a power of two. Recovering the
    // index through the payload pointer is therefore a shift.
    //
    // SAFETY: both point into the same header per the contract.
    unsafe {
        let idx = ((*page).area.addr() - seg.addr()) / SEGMENT_SLICE_SIZE;
        debug_assert!(
            {
                let base: *mut Page = (&raw mut (*seg).pages).cast();
                idx == (page.addr() - base.addr()) / core::mem::size_of::<Page>()
            },
            "page_index: Page::area disagrees with the slot-pointer form"
        );
        idx
    }
}

/// Spin until no page of `seg` has a remote free IN FLIGHT.
///
/// **This closes a use-after-free.** `remote_free` claims `XFLAG_FREEING`
/// BEFORE pushing a block onto the owner's delayed list and holds it until it
/// restores `XFLAG_DELAYED` afterwards. The owner, draining that very push,
/// can free the block, retire the page, empty the segment and release it to an
/// arena — where the next tenant `memset`s the whole header, including the
/// `xthread_free` atomic the remote is about to CAS. Miri caught exactly that:
/// an atomic store in `remote_free` racing `huge_alloc`'s header scrub.
///
/// The window is bounded, which is why a barrier suffices rather than an epoch
/// scheme: before the remote sets FREEING it has not pushed yet, so the owner
/// cannot have drained it, so `used > 0` and no retire is possible. Every
/// dangerous instant therefore has FREEING observably set.
///
/// Cost is a 512-slot scan, paid only when a segment is actually released.
///
/// # Safety
/// `seg` must be a live segment header.
unsafe fn wait_no_remote_in_flight(seg: *mut Segment) {
    use crate::page::{XFLAG_FREEING, XMASK};
    // SAFETY: the page table is inside the live header; atomics only.
    unsafe {
        let base: *mut Page = (&raw mut (*seg).pages).cast();
        // Only the CARVED region can hold a page with a remote free in flight.
        // Slots at/after `next_free_slice` were never carved, so their
        // `xthread_free` is still 0 (`& XMASK == XFLAG_NORMAL`, never
        // `XFLAG_FREEING`) — scanning them is guaranteed-idle work. Bounding to
        // `[HEADER_SLICES, next_free_slice)` turns a fixed 512-slot sweep into
        // one proportional to how much of the segment was ever used: a segment
        // that carved 10 slices scans 10, not 512. (A Huge segment sets
        // `next_free_slice = SLICES_PER_SEGMENT`, so it is unaffected — correct,
        // since its one page occupies the whole reservation.)
        let end = (*seg).next_free_slice as usize;
        for i in HEADER_SLICES..end {
            let pg = base.add(i);
            while (*pg)
                .xthread_free
                .load(core::sync::atomic::Ordering::Acquire)
                & XMASK
                == XFLAG_FREEING
            {
                core::hint::spin_loop();
            }
        }
    }
}

/// Reserve a Normal segment — from an arena when one qualifies (respecting a
/// heap's `arena_id` restriction), else eagerly-committed fresh OS memory —
/// and register it in the segment map.
pub fn segment_alloc(arena_id: i32) -> Result<*mut Segment, PrimError> {
    let (ptr_, size, mem_zero) = match crate::arena::chunk_alloc(arena_id) {
        Some((p, zero)) => (p, SEGMENT_SIZE, zero),
        None => {
            if arena_id >= 0 {
                return Err(0); // exclusive-arena heap and its arena is full
            }
            let (p, sz, zero) = reserve_backing(SEGMENT_SIZE)?;
            (p, sz, zero)
        }
    };
    let seg: *mut Segment = ptr_.cast();
    // SAFETY: 32 MiB mapping (fresh or recycled arena chunk); header written
    // in full. A recycled chunk's stale bytes are all overwritten here and
    // page slots are re-initialized by span_mark on every carve.
    unsafe {
        // Recycled chunks carry stale page slots — scrub the header region.
        if !mem_zero {
            core::ptr::write_bytes(seg.cast::<u8>(), 0, core::mem::size_of::<Segment>());
        }
        (*seg).kind = SegmentKind::Normal;
        (*seg).total_size = size;
        (*seg).next_free_slice = HEADER_SLICES as u32;
        (*seg).used_pages = 0;
        (*seg).thread_id = AtomicUsize::new(crate::init::thread_id());
        (*seg).mem_is_zero = mem_zero;
        (*seg).purged_any = false;
        (*seg).guarded = false;
        (*seg).next = ptr::null_mut();
        (*seg).free_spans = ptr::null_mut();
    }
    segment_map::register(seg);
    Ok(seg)
}

/// Release an empty Normal segment (caller has unlinked it from the heap):
/// back to its arena when it came from one, else to the OS.
///
/// # Safety
/// `seg` must be a live Normal segment with `used_pages == 0` and no live
/// blocks or references into it.
pub unsafe fn segment_free(seg: *mut Segment) -> Result<(), PrimError> {
    // SAFETY: seg is live per the contract; this only reads atomics.
    unsafe { wait_no_remote_in_flight(seg) };
    segment_map::unregister(seg);
    // SAFETY: seg is live and empty per the contract.
    unsafe {
        if (*seg).purged_any || (*seg).guarded {
            // Restore full commitment AND accessibility before the memory can
            // be re-tenanted from an arena (see purged_any / guarded).
            let base = seg.cast::<u8>().add(HEADER_SLICES * SEGMENT_SLICE_SIZE);
            let bytes = (*seg).total_size - HEADER_SLICES * SEGMENT_SLICE_SIZE;
            let _ = os::protect(base, bytes, false);
            let _ = os::commit(base, bytes);
            (*seg).purged_any = false;
            (*seg).guarded = false;
        }
    }
    if crate::arena::chunk_free(seg.cast()) {
        return Ok(());
    }
    // wasm: the slice pool is the free list (F2) — `expose_provenance` so a
    // later `reserve_backing` may reconstruct a pointer to this range.
    #[cfg(all(target_arch = "wasm32", not(miri)))]
    // SAFETY: seg is live per the contract; reading total_size.
    unsafe {
        if crate::slice_pool::free_range(seg.cast::<u8>().expose_provenance(), (*seg).total_size) {
            return Ok(());
        }
    }
    // SAFETY: per contract; reconstruct the OsBlock we allocated with.
    unsafe {
        let block = os::OsBlock {
            ptr: seg.cast(),
            size: (*seg).total_size,
            is_large: false,
            is_zero: false,
        };
        os::free(block)
    }
}

/// Write the span markers for a span at `idx` of `len` slices: first slot
/// offset 0, interior+last offsets pointing back.
///
/// Interior slices below `from` are NOT rewritten: the caller knows they
/// already point back to `idx` in both representations. That holds for every
/// slice of every span in a normal segment (this is their only writer, and
/// `debug_validate_segment` checks it), so a span taken from the FRONT of a
/// free span, or a freed span that did not merge into its left neighbour,
/// starts where its interiors already point. Rewriting them was the largest
/// loop in both `span_alloc` and `span_free`, and it stored values that were
/// already there: 31 of 31 interior slices, twice, per 2 MiB allocate-and-free
/// (opscan `huge`, callgrind per instruction). `from == 1` marks everything.
///
/// # Safety
/// `[idx, idx+len)` must lie in the carved region of `seg` under the heap lock;
/// `from >= 1`, and slices `idx+1 .. idx+min(from, len)` must already point
/// back to `idx`.
unsafe fn span_mark(seg: *mut Segment, idx: usize, len: usize, from: usize) {
    debug_assert!(from >= 1, "span_mark: slot 0 is the head, not an interior");
    // SAFETY: caller contract keeps every index in bounds.
    unsafe {
        (*seg).pages[idx].slice_offset = 0;
        (*seg).pages[idx].slice_count = len as u16;
        // Cache the payload start now, while `idx` is an integer — a shift, no
        // division. The refill path then reads `(*p).area` instead of
        // recovering it with `page_index` (see `Page::area`).
        (*seg).pages[idx].area = seg.cast::<u8>().add(idx * SEGMENT_SLICE_SIZE);
        // Interior slices, walked with RUNNING values rather than recomputed
        // indices. `(*seg).pages[idx + j]` costs a multiply by `size_of::<Page>()`
        // — which is 88, not a power of two — and `j * slot_stride()` costs a
        // second; both become adds when the loop carries the slot pointer and
        // the byte offset it is about to store. Every span carved and every
        // span released walks this loop, and a workload that churns spans
        // walks it constantly.
        // Every slice of this span resolves to the span's own slot, so the
        // table entry is one constant written across the run.
        let owner = page_off_for(idx);
        let tab: *mut u32 = (&raw mut (*seg).page_off).cast();
        *tab.wrapping_add(idx) = owner;
        // NOTE (2026-09-24, REFUTED): splitting the owner-table stores into
        // their own `slice::fill` pass, away from the stride-88 `slice_offset`
        // stores, made both loops cheaper (151 -> 92 + 43 instructions for a
        // 32-slice span) and the function DEARER: each loop brings its own
        // unroll setup and scalar remainder. Opscan `huge` +28.00, perl +340,
        // lua -8,327, sqlite -862 allocator Ir — a sign flip by span length,
        // not a win. Same verdict as the fill `huge_alloc` measured flat.
        let base: *mut Page = (&raw mut (*seg).pages).cast();
        let mut slot = base.wrapping_add(idx + from);
        let mut ent = tab.wrapping_add(idx + from);
        let mut j = from;
        while j < len {
            // SLICES back to the span start, not bytes — see Page::slice_offset.
            (*slot).slice_offset = j as u16;
            *ent = owner;
            slot = slot.wrapping_add(1);
            ent = ent.wrapping_add(1);
            j += 1;
        }
    }
}

// ASSESSED AND DECLINED 2026-08-21 — `span_mark_free` writing only the span's
// FIRST and LAST slot instead of every interior one.
//
// The reasoning holds: interior `slice_offset`s are read by exactly one
// function, `page_of`, which is only ever called on a pointer to a LIVE block,
// and coalescing follows back only from a neighbour's LAST slot. Free spans
// get long once they coalesce — ~16 interior slices per call on `sh8bench` —
// so this was the largest line in both `span_alloc` and `span_free`.
//
// It was not landed. `debug_validate_segment` asserts the STRONGER invariant —
// every span, free or live, has every interior slice pointing back — and that
// walk is the net which caught the M8 parallel-gate race that `adopt_segment`
// documents. Landing this meant weakening a validated layout invariant of an
// allocator to buy ~20M Ir on a benchmark whose own noise floor is ~100M: a
// change we could not measure where it matters, paid for in the one property
// this code most needs to keep. If it is ever wanted, it needs the invariant
// re-specified deliberately — free spans validated on bounds, live spans on
// interiors — not weakened as a side effect of an optimisation.

/// Mark `[idx, idx+len)` as a FREE span and push it on the free list.
///
/// # Safety
/// As [`span_mark`]; the span must not be in the free list already.
unsafe fn span_mark_free(seg: *mut Segment, idx: usize, len: usize, from: usize) {
    // SAFETY: caller contract.
    unsafe {
        span_mark(seg, idx, len, from);
        let slot: *mut Page = &raw mut (*seg).pages[idx];
        (*slot).block_size = 0; // free marker
        (*slot).prev = ptr::null_mut();
        (*slot).next = (*seg).free_spans;
        if !(*seg).free_spans.is_null() {
            (*(*seg).free_spans).prev = slot;
        }
        (*seg).free_spans = slot;
    }
}

/// Unlink a free span from the free list.
///
/// # Safety
/// `span` must be a free-span start slot currently linked in `seg`'s list.
unsafe fn span_list_remove(seg: *mut Segment, span: *mut Page) {
    // SAFETY: caller contract.
    unsafe {
        if (*span).prev.is_null() {
            (*seg).free_spans = (*span).next;
        } else {
            (*(*span).prev).next = (*span).next;
        }
        if !(*span).next.is_null() {
            (*(*span).next).prev = (*span).prev;
        }
        (*span).next = ptr::null_mut();
        (*span).prev = ptr::null_mut();
    }
}

/// `debug_checks`: verify the carved region is exactly TILED by spans, every
/// span start is marked, and the free-span list is consistent. Layout
/// corruption shows up here instead of as a wild pointer later.
///
/// # Safety
/// `seg` must be a live segment the caller is already operating on.
pub unsafe fn debug_validate_segment(seg: *mut Segment, where_: &str) {
    #[cfg(feature = "debug_checks")]
    {
        // SAFETY: caller is operating on this live segment already.
        unsafe {
            let end = (*seg).next_free_slice as usize;
            let mut idx = HEADER_SLICES;
            let mut spans = 0usize;
            while idx < end {
                let slot: *mut Page = &raw mut (*seg).pages[idx];
                assert_eq!(
                    (*slot).slice_offset,
                    0,
                    "{where_}: slice {idx} is not a span start (layout not tiled)"
                );
                let len = (*slot).slice_count as usize;
                assert!(
                    len > 0 && idx + len <= end,
                    "{where_}: slice {idx} has bad slice_count {len} (end {end})"
                );
                // Interior slices must point back to this start.
                for j in 1..len {
                    assert_eq!(
                        (*seg).pages[idx + j].slice_offset as usize,
                        j,
                        "{where_}: slice {} lost its back-pointer",
                        idx + j
                    );
                }
                spans += 1;
                assert!(
                    spans <= SLICES_PER_SEGMENT,
                    "{where_}: span walk did not terminate"
                );
                idx += len;
            }
            assert_eq!(idx, end, "{where_}: spans do not tile the carved region");
            // Free-span list: every node is a free span start inside the region.
            let mut s = (*seg).free_spans;
            let mut n = 0usize;
            while !s.is_null() {
                let i = page_index(seg, s);
                assert!(
                    i >= HEADER_SLICES && i < end,
                    "{where_}: free span {i} out of region"
                );
                assert_eq!((*s).slice_offset, 0, "{where_}: free span {i} not a start");
                assert_eq!((*s).block_size, 0, "{where_}: free span {i} marked live");
                n += 1;
                assert!(
                    n <= SLICES_PER_SEGMENT,
                    "{where_}: free-span list is cyclic"
                );
                s = (*s).next;
            }
        }
    }
    #[cfg(not(feature = "debug_checks"))]
    {
        let _ = (seg, where_);
    }
}

/// Allocate a span of `slices` from the segment: first-fit over reclaimed
/// spans, else bump. Returns (span-start slot, span-is-fresh-zero) or null.
///
/// # Safety
/// `seg` must be a live Normal segment under the heap lock.
pub unsafe fn span_alloc(seg: *mut Segment, slices: usize) -> (*mut Page, bool) {
    // SAFETY: caller contract — seg is live under the owner.
    unsafe { debug_validate_segment(seg, "span_alloc:enter") };
    // SAFETY: heap lock held; list and indices maintained by the invariants
    // in the module docs.
    unsafe {
        // First fit over reclaimed spans (recycled memory — NOT zero).
        let mut s = (*seg).free_spans;
        while !s.is_null() {
            let len = (*s).slice_count as usize;
            if len >= slices {
                span_list_remove(seg, s);
                let idx = page_index(seg, s);
                // Re-commit BEFORE splitting so both halves are backed; the
                // remainder inherits the (now cleared) purged state.
                span_recommit(seg, idx, len);
                if len > slices {
                    // The remainder starts at a new slice: re-mark all of it.
                    span_mark_free(seg, idx + slices, len - slices, 1);
                }
                // The FRONT of a free span: its interiors already point here.
                span_mark(seg, idx, slices, slices);
                (*seg).used_pages += 1;
                return (s, false);
            }
            s = (*s).next;
        }
        // Bump (never-carved region — zero iff the mapping came fresh from
        // the OS; recycled arena chunks are dirty).
        let idx = (*seg).next_free_slice as usize;
        if idx + slices > SLICES_PER_SEGMENT {
            return (ptr::null_mut(), false);
        }
        (*seg).next_free_slice = (idx + slices) as u32;
        (*seg).used_pages += 1;
        let start: *mut Page = &raw mut (*seg).pages[idx];
        // Never-carved slices: mark all of them.
        span_mark(seg, idx, slices, 1);
        (start, (*seg).mem_is_zero)
    }
}

/// Return a page's span to the segment, coalescing with free neighbors.
/// Freed spans NEVER rejoin the bump frontier: `[next_free_slice, …)` is the
/// virgin-zero region and bump allocations report `fresh = true` — giving
/// recycled slices back to it would let dirty memory skip zalloc's memset
/// (caught by the spans G1c gate, 2026-08-05).
/// The page must already be unlinked from heap queues.
///
/// Returns whether the freed span was purged back to the OS.
///
/// # Safety
/// `page` must be a live span-start of `seg` with no live blocks, under the
/// heap lock.
pub unsafe fn span_free(seg: *mut Segment, page: *mut Page) -> bool {
    // SAFETY: caller contract — seg is live under the owner.
    unsafe { debug_validate_segment(seg, "span_free:enter") };
    // SAFETY: heap lock held; boundary reads follow the span invariants.
    unsafe {
        let mut idx = page_index(seg, page);
        let mut len = (*page).slice_count as usize;
        // Where the freed span sat before any merge: its interiors already
        // point back to `freed_idx` (see `span_mark`).
        let (freed_idx, freed_len) = (idx, len);
        (*seg).used_pages -= 1;
        // Scrub page state so a stale slot can't masquerade as live.
        (*page).block_size = 0;
        (*page).free = ptr::null_mut();
        (*page).local_free = ptr::null_mut();
        (*page).used = 0;
        (*page).capacity = 0;
        (*page).reserved = 0;
        // `blockmap`: the map pointer is page state like the rest — leave it
        // and the next tenant of this slot inherits a map it never carved.
        #[cfg(feature = "blockmap")]
        {
            (*page).payload = ptr::null_mut();
        }
        (*page).flags.store(0, Ordering::Relaxed);
        (*page).free_is_zero = false;

        // Merge right: the slot at idx+len (if carved) is a span START.
        let right = idx + len;
        if right < (*seg).next_free_slice as usize {
            let rslot: *mut Page = &raw mut (*seg).pages[right];
            debug_assert_eq!((*rslot).slice_offset, 0, "right neighbor not a span start");
            if (*rslot).block_size == 0 {
                span_list_remove(seg, rslot);
                len += (*rslot).slice_count as usize;
            }
        }
        // Merge left: follow the left slot back to its span start.
        if idx > HEADER_SLICES {
            let lslot_idx = idx - 1;
            // slice_offset counts SLICES, so the span start is a subtract.
            // This was `bytes / slot_stride()` — a `movabs; mul; shr` magic
            // multiply by 88, `docs/opps.md` #1, now gone.
            let lstart_idx = lslot_idx - (*seg).pages[lslot_idx].slice_offset as usize;
            let lstart: *mut Page = &raw mut (*seg).pages[lstart_idx];
            if (*lstart).block_size == 0 {
                span_list_remove(seg, lstart);
                len += idx - lstart_idx;
                idx = lstart_idx;
            }
        }
        // Re-mark only what does not already point to `idx`: with no left
        // merge the freed span itself starts here and only an absorbed right
        // neighbour moves; after a left merge the left span is already right
        // and everything from the freed span on moves.
        let from = if idx == freed_idx {
            freed_len
        } else {
            freed_idx - idx
        };
        span_mark_free(seg, idx, len, from);
        // PURGE the coalesced free span (the RSS lever): return its pages to
        // the OS. Only worthwhile for multi-slice spans — a syscall costs
        // more than the pages a single 64 KiB slice returns. The span is
        // marked `purged` so reuse re-commits it; skipping that recommit is
        // an access violation on Windows (MEM_DECOMMIT), which is how this
        // was caught (2026-08-05).
        // Length FIRST: it is in a register, the option is a table load, a
        // bounds test and a sentinel test — and most freed spans are short, so
        // most frees never need the option at all.
        if len >= crate::types::MEDIUM_PAGE_SLICES && crate::options::get(15) >= 0 {
            let area = page_area(seg, idx);
            let bytes = len * SEGMENT_SLICE_SIZE;
            let decommits = crate::options::is_enabled(5); // purge_decommits
            if os::purge(area, bytes, decommits).is_ok() {
                (*seg).pages[idx].purged = true;
                (*seg).purged_any = true;
                return true;
            }
        }
        false
    }
}

/// Purge every FREE span of `seg` — used when a dying thread abandons it.
///
/// An abandoned segment is orphaned until some other thread happens to adopt
/// it, and until then it holds every page it ever touched RESIDENT. Measured:
/// 25 orphans accumulated across 8 waves of thread churn and a full 2048-block
/// allocation burst adopted only 4 of them. At 32 MiB each, that is the RSS
/// tail FFAI saw — max 403 MB against mimalloc's 134, with a 4.4x run-to-run
/// spread because whether anything adopts is pure scheduling.
///
/// Deliberately NOT gated on `purge_delay`. That option governs a LIVE heap's
/// own free spans, where the pages are likely to be reused shortly and a
/// syscall would be wasted. An abandoned segment has no owner to reuse them —
/// holding them costs memory for an unbounded time and buys nothing. Upstream
/// draws the same distinction with `abandoned_page_purge`.
///
/// Each purged span is marked so reuse re-commits it (skipping that is an
/// access violation on Windows), exactly as `span_free` does.
///
/// # Safety
/// `seg` must be a live Normal segment whose free-span list is stable — i.e.
/// the caller still owns it, which is true right up to the abandon publish.
pub unsafe fn purge_free_spans(seg: *mut Segment) {
    // SAFETY: caller still owns the segment; free spans have no live blocks.
    unsafe {
        let decommits = crate::options::is_enabled(5); // purge_decommits
        let mut s = (*seg).free_spans;
        while !s.is_null() {
            let next = (*s).next;
            if !(*s).purged {
                let idx = page_index(seg, s);
                let len = (*s).slice_count as usize;
                if len > 0 {
                    let area = page_area(seg, idx);
                    let bytes = len * SEGMENT_SLICE_SIZE;
                    if os::purge(area, bytes, decommits).is_ok() {
                        (*s).purged = true;
                        (*seg).purged_any = true;
                    }
                }
            }
            s = next;
        }
    }
}

/// Re-commit a span that was purged while free (no-op otherwise).
///
/// # Safety
/// `[idx, idx+len)` is a span of `seg` being handed to a caller.
unsafe fn span_recommit(seg: *mut Segment, idx: usize, len: usize) {
    // SAFETY: caller contract; range lies inside the segment reservation.
    unsafe {
        if !(*seg).pages[idx].purged {
            return;
        }
        (*seg).pages[idx].purged = false;
        let area = page_area(seg, idx);
        let _ = os::commit(area, len * SEGMENT_SLICE_SIZE);
    }
}

/// Allocate a dedicated huge segment for one block of `size` bytes, placed so
/// `(block + offset) % align == 0` (align ≤ SEGMENT_SIZE/2). Returns
/// (segment, block ptr).
pub fn huge_alloc(
    size: usize,
    align: usize,
    offset: usize,
    arena_id: i32,
) -> Result<(*mut Segment, *mut u8), PrimError> {
    // A garbage alignment is EINVAL, not a `debug_assert!`: this is a safe
    // `pub fn`, and the mask arithmetic below is only sound for a power of
    // two. Cold path (dedicated segment), so the two compares are free.
    if !align.is_power_of_two() || align > SEGMENT_SIZE / 2 {
        return Err(22);
    }
    let header = SEGMENT_SLICE_SIZE;
    // Worst-case room for placing the block within the reservation. Hosted,
    // the area is slice-aligned, so only larger alignments — or offsets that
    // shift the placement off the natural boundary — need slack. On a fixed
    // region it is only `REGION_ALIGN`-aligned in absolute terms, because
    // segments stride from a `MAX_ALIGN_SIZE`-aligned base
    // (`crate::REGION_STRIDES`), so anything coarser needs the slack there.
    // `is_multiple_of` on a RUNTIME `align` is a modulo, i.e. a `div`.
    // `bins::is_aligned_to` is the mask, and its conservative direction is the
    // safe one here: a `false` makes this reserve the extra slack it would have
    // reserved for a misaligned offset anyway.
    let natural = if crate::REGION_STRIDES {
        crate::prim::fixed::REGION_ALIGN
    } else {
        SEGMENT_SLICE_SIZE
    };
    let extra = if align > natural || !crate::bins::is_aligned_to(offset, align) {
        align
    } else {
        0
    };
    // An unsatisfiable reservation (`malloc(usize::MAX)`) is an error, not a
    // `header + size` debug overflow or a wrapped small reservation.
    let Some(raw) = header.checked_add(size).and_then(|s| s.checked_add(extra)) else {
        return Err(0);
    };
    let want = os::page_align_up(raw);
    if want < raw {
        return Err(0);
    }
    // Huge blocks recycle through arenas too (contiguous chunks) — without
    // this, every huge alloc/free cycle is an OS round-trip (the Tier-A
    // malloc-large gate measured 3–4× slower before this path).
    //
    // `arena_id` is the OWNING HEAP's, exactly as `segment_alloc` above uses
    // it. It used to be a hardcoded `-1`, which meant an exclusive-arena heap
    // — the whole point of which is that its memory comes from ONE region —
    // silently took its huge blocks from the default arena or straight from
    // the OS. Upstream passes `heap->arena_id` here
    // (`mi_segment_huge_page_alloc`, oracle segment.c:1671/1683); we did not.
    // See `tests/heaps.rs::exclusive_arena_confines_huge_allocations`.
    let chunks = want.div_ceil(SEGMENT_SIZE);
    let Some(chunk_bytes) = chunks.checked_mul(SEGMENT_SIZE) else {
        return Err(0);
    };
    let (bptr, total, mem_zero) = match crate::arena::chunk_alloc_n(arena_id, chunks) {
        Some((p, zero)) => (p, chunk_bytes, zero),
        None => {
            if arena_id >= 0 {
                return Err(0); // exclusive-arena heap and its arena is full
            }
            let (p, sz, zero) = reserve_backing(want)?;
            (p, sz, zero)
        }
    };
    let seg: *mut Segment = bptr.cast();
    // SAFETY: header region fully written below; recycled chunks scrubbed.
    unsafe {
        if !mem_zero {
            core::ptr::write_bytes(seg.cast::<u8>(), 0, core::mem::size_of::<Segment>());
        }
    }
    let b = os::OsBlock {
        ptr: bptr,
        size: total,
        is_large: false,
        is_zero: mem_zero,
    };
    segment_map::register_range(seg.addr(), b.size);
    // SAFETY: fresh zeroed reservation ≥ header + size + extra.
    unsafe {
        (*seg).kind = SegmentKind::Huge;
        (*seg).total_size = b.size;
        (*seg).next_free_slice = SLICES_PER_SEGMENT as u32;
        (*seg).used_pages = 1;
        (*seg).thread_id = AtomicUsize::new(crate::init::thread_id());
        (*seg).mem_is_zero = b.is_zero;
        (*seg).purged_any = false;
        (*seg).guarded = false;
        (*seg).next = ptr::null_mut();
        (*seg).free_spans = ptr::null_mut();
        let area = b.ptr.add(header);
        // (block + offset) aligned: round (area + offset) up, subtract offset.
        // `area + offset + align` can wrap for a caller-chosen offset; that is
        // an unsatisfiable request, not a block placed by wrapped arithmetic.
        let Some(addr) = crate::bins::aligned_at_from(area.addr(), offset, align) else {
            // SAFETY: the reservation is registered and unused; no block escaped.
            let _ = huge_free(seg);
            return Err(0);
        };
        if addr < area.addr() {
            // SAFETY: as the overflow arm.
            let _ = huge_free(seg);
            return Err(0);
        }
        let block = area.with_addr(addr);
        debug_assert!(block.addr() >= area.addr() && (block.addr() + offset).is_multiple_of(align));
        // The single page lives in slot 1; every reachable interior slice
        // offsets back to it (only slices 1..512 are addressable via the mask
        // trick, and aligned offsets stay < SEGMENT_SIZE/2 by the contract).
        let page: *mut Page = &raw mut (*seg).pages[1];
        // The block's usable size is what was ASKED for, rounded up to a
        // slice — upstream's `psize` (`mi_segment_huge_page_alloc`, oracle
        // segment.c:1599) — and NOT the reservation. The reservation is
        // chunk-rounded (`chunks * SEGMENT_SIZE` through an arena), so a 33 MB
        // request holds a 64 MiB chunk pair and a 64 MB one holds 96 MiB.
        // This field used to report that whole extent, and `usable_size` is
        // the copy length of every `realloc` that MOVES: growing a 33 MB block
        // copied — and first-touched, on both sides — 64 MiB, and a 64 MB
        // block 96 MiB. Measured from a consumer at **1.41x / 1.23x mimalloc,
        // 0/6 pairs** for exactly those two steps, at parity for every step
        // below the segment size (`docs/plans/youslowbro.md` §3). `zalloc` on
        // a recycled chunk zeroes `usable_size` bytes and paid the same tax.
        // The capacity is still `total_size`, and nothing reads past
        // `block_size`; `usable_size` now agrees with `mi_usable_size` here.
        let capacity = b.size - (block.addr() - seg.addr());
        (*page).block_size = crate::prim::align_up(size.max(1), SEGMENT_SLICE_SIZE).min(capacity);
        (*page).used = 1;
        (*page).capacity = 1;
        (*page).reserved = 1;
        (*page).slice_count = (SLICES_PER_SEGMENT - 1) as u16;
        (*page).slice_offset = 0;
        // `header == SEGMENT_SLICE_SIZE`, so this is exactly `page_area(seg, 1)`.
        // Huge segments build their one slot by hand instead of through
        // `span_mark`, which is the only other writer of this field — so this
        // was the single page in the allocator whose `area` stayed null.
        // `page_index` and `unalign` both read it now, and both are reachable
        // for a huge block (`SINGLE_BLOCK` is set, so `unalign` does not take
        // its early return).
        (*page).area = area;
        debug_assert_eq!(
            (*page).area,
            page_area(seg, 1),
            "huge: area != page_area(1)"
        );
        (*page).flags.store(
            crate::page::pflags::HUGE_SEGMENT | crate::page::pflags::SINGLE_BLOCK,
            Ordering::Relaxed,
        );
        (*page).free_is_zero = b.is_zero;
        // Every slice of a huge segment resolves to slot 1, so the owner table
        // is one value repeated across the run. (Writing it as a vectorised
        // `slice::fill` instead of a store in the loop below measured exactly
        // flat — the loop is dominated by `slice_offset`, which strides by
        // `size_of::<Page>()` and cannot vectorise.)
        let owner = page_off_for(1);
        let tab: *mut u32 = (&raw mut (*seg).page_off).cast();
        *tab.wrapping_add(1) = owner;
        let mut j = 2;
        while j < SLICES_PER_SEGMENT {
            // Slices back to slot 1, which holds this huge block's page data.
            (*seg).pages[j].slice_offset = (j - 1) as u16;
            *tab.wrapping_add(j) = owner;
            j += 1;
        }
        Ok((seg, block))
    }
}

/// Free a huge segment (the whole reservation).
///
/// # Safety
/// `seg` must be a live Huge segment with no live references into it.
pub unsafe fn huge_free(seg: *mut Segment) -> Result<(), PrimError> {
    // SAFETY: per contract; reconstruct the OsBlock we allocated with.
    unsafe {
        // SAME BARRIER AS `segment_free`, and this is the path that actually
        // raced: a huge segment recycles through `chunk_free_n` WITHOUT going
        // through `segment_free`, so guarding only there left the hole open.
        // Every route by which memory can reach an arena needs it.
        wait_no_remote_in_flight(seg);
        segment_map::unregister_range(seg.addr(), (*seg).total_size);
        // Arena-backed huge blocks recycle their contiguous chunks — but the
        // memory must be handed back in a USABLE state: lift any guard-page
        // protection and restore commitment first. Skipping this recycles an
        // inaccessible page into the next tenant (the M8 P0).
        if (*seg).total_size.is_multiple_of(SEGMENT_SIZE) {
            if (*seg).guarded || (*seg).purged_any {
                let base = seg.cast::<u8>().add(HEADER_SLICES * SEGMENT_SLICE_SIZE);
                let bytes = (*seg).total_size - HEADER_SLICES * SEGMENT_SLICE_SIZE;
                let _ = os::protect(base, bytes, false);
                let _ = os::commit(base, bytes);
                (*seg).guarded = false;
                (*seg).purged_any = false;
            }
            if crate::arena::chunk_free_n(seg.cast(), (*seg).total_size / SEGMENT_SIZE) {
                return Ok(());
            }
        }
        // wasm: ragged (slice-granular) huge reservations recycle through
        // the slice pool — the F2 fix itself. Chunk-multiple ones only
        // reach here when no arena owns them, and the pool takes those too.
        #[cfg(all(target_arch = "wasm32", not(miri)))]
        if crate::slice_pool::free_range(seg.cast::<u8>().expose_provenance(), (*seg).total_size) {
            return Ok(());
        }
        let block = os::OsBlock {
            ptr: seg.cast(),
            size: (*seg).total_size,
            is_large: false,
            is_zero: false,
        };
        os::free(block)
    }
}
