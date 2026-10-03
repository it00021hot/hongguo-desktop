//! Arenas v1 (M6 subset of upstream `arena.c`): pre-reserved OS regions carved
//! into SEGMENT-sized chunks that segment alloc/free recycles instead of
//! round-tripping the OS. Upstream arenas allocate at slice granularity; ours
//! are segment-granular (32 MiB chunks) — sufficient for the §5.9 API and the
//! `disallow_os_alloc` / programmatic-memory use cases; slice-granular arenas
//! are a post-v1 refinement.
//!
//! Chunk state is two atomic bitmaps: `used` (allocation) and `dirty`
//! (ever-used → its memory is NOT zero — feeds the segment zero-tracking).

use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicUsize, Ordering};

/// Word type of the chunk bitmaps.
///
/// `u32`, not `u64`: a bitmap's word width is a FREE CHOICE — the same total
/// bits either way — and 32-bit RISC-V (ESP32-C3/C6) and Xtensa (ESP32-S3)
/// have no 64-bit atomic at all. This is the one CORRECTNESS-path 64-bit
/// atomic the crate had (P3 of `docs/plans/small-metal.md`, §2.2), and
/// narrowing it keeps the claim/verify loop lock-free rather than routing it
/// through `portable-atomic`'s fallback lock.
type BitWord = u32;
type AtomicBitWord = AtomicU32;
/// Bits per bitmap word.
const WORD_BITS: usize = BitWord::BITS as usize;

use crate::os;
use crate::types::SEGMENT_SIZE;

const MAX_ARENAS: usize = 32;
const MAX_CHUNKS: usize = 1024; // 32 GiB per arena at 32 MiB chunks

/// One reserved region.
pub struct Arena {
    /// Base pointer (SEGMENT_SIZE-aligned). A POINTER, not an address:
    /// chunk derivation keeps provenance and the region stays reachable
    /// (the M4 reachability-follows-pointers lesson, recaught here by miri).
    pub base: *mut u8,
    /// Usable size in bytes at REGISTRATION (whole chunks). Adoption may
    /// extend an arena in place afterwards; the LIVE geometry is tracked by
    /// `chunks_live` and reported by [`arena_area`]. Kept as a plain public
    /// field deliberately: no pub fn returns an `Arena` (the table is
    /// private), so this field is unreachable from outside the crate and
    /// changing its shape would be a semver break with no beneficiary —
    /// cargo-semver-checks flagged exactly that when these briefly became
    /// private atomics.
    pub size: usize,
    /// Chunk count at registration. Same note as `size`.
    pub chunks: usize,
    /// LIVE chunk count: the registration value plus any in-place extensions
    /// by [`adopt_os_block`]. Atomic because extension races the lock-free
    /// scans: new chunks' `dirty` bits are published BEFORE the enlarged
    /// count (`Release`/`Acquire`), so a scanner that observes the count
    /// observes their state too; one holding the old value merely misses the
    /// new chunks for one scan. Every internal reader derives the live byte
    /// size as `chunks_live * SEGMENT_SIZE` rather than reading `size`.
    chunks_live: AtomicUsize,
    /// Only heaps created with this arena id may allocate from it.
    pub exclusive: bool,
    /// We own the mapping (false for `mi_manage_os_memory` memory).
    pub owned: bool,
    /// Advisory NUMA node (recorded; placement lands post-v1).
    pub numa_node: i32,
    used: [AtomicBitWord; MAX_CHUNKS / WORD_BITS],
    dirty: [AtomicBitWord; MAX_CHUNKS / WORD_BITS],
}

static ARENAS: [AtomicPtr<Arena>; MAX_ARENAS] =
    [const { AtomicPtr::new(ptr::null_mut()) }; MAX_ARENAS];
static ARENA_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Register a region as an arena. `base` must be SEGMENT_SIZE-aligned and
/// `size` a whole number of chunks. Returns the arena id.
fn arena_register(
    base: *mut u8,
    size: usize,
    exclusive: bool,
    owned: bool,
    numa_node: i32,
) -> Result<i32, ()> {
    let chunks = size / SEGMENT_SIZE;
    if chunks == 0 || chunks > MAX_CHUNKS || !base.addr().is_multiple_of(SEGMENT_SIZE) {
        return Err(());
    }
    // Arena descriptor lives in its own OS pages (never in the allocator).
    let desc = os::alloc_aligned(core::mem::size_of::<Arena>(), os::page_size(), true, false)
        .map_err(|_| ())?;
    let a: *mut Arena = desc.ptr.cast();
    // SAFETY: fresh zeroed mapping; the bitmap words' zero bit-pattern is valid, so
    // only the scalar fields need writing.
    unsafe {
        (*a).base = base;
        (*a).size = size;
        (*a).chunks = chunks;
        (*a).chunks_live.store(chunks, Ordering::Release);
        (*a).exclusive = exclusive;
        (*a).owned = owned;
        (*a).numa_node = numa_node;
    }
    // CAS so `ARENA_COUNT` never exceeds `MAX_ARENAS` (no transient `ARENAS[32]`
    // window for `arena_area`, and the overflow path frees the descriptor
    // instead of leaking it).
    loop {
        let id = ARENA_COUNT.load(Ordering::Acquire);
        if id >= MAX_ARENAS {
            // SAFETY: `desc` is the descriptor mapping allocated above and
            // never published (no `ARENAS` slot names it).
            let _ = unsafe { os::free(desc) };
            return Err(());
        }
        if ARENA_COUNT
            .compare_exchange(id, id + 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            ARENAS[id].store(a, Ordering::Release);
            return Ok(id as i32);
        }
    }
}

/// `mi_reserve_os_memory_ex`: reserve (and commit) fresh OS memory as an
/// arena. Returns the arena id.
#[allow(clippy::result_unit_err)] // C error-code mapping at the FFI edge
pub fn reserve_os_memory_ex(
    size: usize,
    _commit: bool,
    allow_large: bool,
    exclusive: bool,
) -> Result<i32, ()> {
    // A fixed-region target has no OS to reserve from; an arena there could
    // only be carved out of the one region and would serve the same bytes
    // through one more table -- and a range managed from outside would carve
    // its chunks on absolute segment boundaries, where this target's segments
    // stride from the region's base (`crate::FIXED_REGION`). Refused, so the
    // arena layer -- its 32-slot registry, the chunk scan on every segment
    // allocation, the reserve-on-miss path -- has no reachable entry and
    // leaves the image (`firmware-what-is-left.md` §3).
    if crate::FIXED_REGION {
        return Err(());
    }
    // Unsatisfiable size (e.g. usize::MAX) is Err, not `chunks * SEGMENT_SIZE`
    // overflow (OH-rusty_alloc-12).
    let chunks = size.div_ceil(SEGMENT_SIZE);
    let Some(total) = chunks.checked_mul(SEGMENT_SIZE) else {
        return Err(());
    };
    // Eager commit (our segment model); `_commit=false` still reserves+commits
    // in v1 — recorded divergence, matches how segments consume chunks.
    let b = os::alloc_aligned(total, SEGMENT_SIZE, true, allow_large).map_err(|_| ())?;
    match arena_register(b.ptr, total, exclusive, true, -1) {
        Ok(id) => Ok(id),
        Err(()) => {
            // SAFETY: `b` is the reservation made two lines up; a refused
            // register never recorded it, so nothing else can reach it.
            let _ = unsafe { os::free(b) };
            Err(())
        }
    }
}

/// `mi_manage_os_memory_ex`: adopt caller-provided memory (never freed by us).
/// The SEGMENT_SIZE-aligned interior is used; ragged edges are ignored.
#[allow(clippy::result_unit_err)] // C error-code mapping at the FFI edge
#[allow(
    clippy::fn_params_excessive_bools,
    reason = "mirrors mi_manage_os_memory_ex's C signature 1:1; grouping the \
              flags into a struct would break the ABI parity this crate exists for"
)]
pub fn manage_os_memory_ex(
    start: *mut u8,
    size: usize,
    _is_committed: bool,
    _is_large: bool,
    _is_zero: bool,
    numa_node: i32,
    exclusive: bool,
) -> Result<i32, ()> {
    // As `reserve_os_memory_ex`: no arenas on a one-region target.
    if crate::FIXED_REGION {
        return Err(());
    }
    // Null / overflowing ranges are Err, not `addr + size` debug overflow or a
    // registry covering the address space (OH-rusty_alloc-15). Whether the
    // range is real memory is the OS's answer (`range_is_reserved`, below),
    // not an address floor's.
    if start.is_null() {
        return Err(());
    }
    let Some(end) = start.addr().checked_add(size) else {
        return Err(());
    };
    let Some(lo_raw) = start.addr().checked_add(SEGMENT_SIZE - 1) else {
        return Err(());
    };
    let lo_addr = lo_raw & !(SEGMENT_SIZE - 1);
    let hi = end & !(SEGMENT_SIZE - 1);
    if hi <= lo_addr {
        return Err(());
    }
    let lo = start.with_addr(lo_addr);
    let span = hi - lo_addr;
    // A window we already named is live heap, not caller memory
    // (OH-rusty_alloc-202). An unmapped lie must not become an arena
    // (OH-rusty_alloc-201) — the next default malloc would write a Segment
    // there.
    if crate::segment_map::contains(lo) || !crate::os::range_is_reserved(lo, span) {
        return Err(());
    }
    // Conservative: treat managed memory as dirty (not-zero) — the dirty
    // bitmap starts clear, so mark it at first alloc instead; simplest is to
    // pre-mark every chunk dirty.
    let id = arena_register(lo, hi - lo_addr, exclusive, false, numa_node)?;
    let a = ARENAS[id as usize].load(Ordering::Acquire);
    // SAFETY: freshly registered arena descriptor.
    unsafe {
        let chunks = (*a).chunks_live.load(Ordering::Acquire);
        for w in 0..chunks.div_ceil(WORD_BITS) {
            let bits = if (w + 1) * WORD_BITS <= chunks {
                BitWord::MAX
            } else {
                ((1 as BitWord) << (chunks % WORD_BITS)) - 1
            };
            (*a).dirty[w].store(bits, Ordering::Relaxed);
        }
    }
    Ok(id)
}

/// Whether reserving a large default arena up front pays for itself.
///
/// **False on wasm — but not for the reason a first reading suggests.** The
/// arena plays two roles: it is a cheap ADDRESS-SPACE RESERVATION that keeps
/// segment churn off the OS, and it is the FREE LIST that `segment_free` /
/// `huge_free` return chunks to. On wasm the first role is actively harmful —
/// `memory.grow` backs every byte immediately, so a 1 GiB "reservation" costs
/// 1 GiB of real linear memory before the first `malloc` returns (measured by
/// `bench/wasm-selftest.mjs`: 1056.06 MiB -> 64.00 MiB, 6.79 ms -> 1.75 ms).
/// The second role is INDISPENSABLE there: `prim::free` is a no-op, so the
/// arena is the only recycling layer that exists, and v1.1.4 shipped with
/// both roles disabled — every freed segment reached the no-op `free` and
/// 32 MiB of linear memory became permanently unreachable, one whole segment
/// per >16 MiB alloc/free cycle (measured: 20 cycles of one 20 MiB `Vec`
/// leaked exactly +640 MiB).
///
/// So on wasm the default arena stays OFF and the free-list role is served
/// incrementally instead: [`adopt_os_block`] registers each OS-allocated
/// block as arena chunks at the moment it is FREED, growing the recyclable
/// pool to the workload's peak footprint and never beyond it — no up-front
/// commit, no leak. Upstream reaches for a milder lever (`arena.c` divides
/// the reserve by 4 where virtual reserve is unsupported); adopt-on-free
/// dominates both.
const DEFAULT_ARENA_PAYS: bool = !cfg!(all(target_arch = "wasm32", not(miri)));

/// Reserve ANOTHER default-sized arena (`mi_option_arena_reserve`, 1 GiB by
/// default), called when every existing arena is full — this is what keeps
/// segment churn off the OS (without it, malloc-large pays a
/// VirtualAlloc/munmap round-trip per cycle, measured 3–4× slower on the
/// Tier-A gate).
///
/// Returns true when the caller should rescan the arena table. Until v1.1.4
/// this was one-shot behind a `TRIED` flag: the FIRST miss reserved the
/// default arena and every later exhaustion fell through to raw OS segments
/// forever — correct but 3–4× slower on exactly the workloads arenas exist
/// for, and the flag was a swap on EVERY chunk_alloc. Now reserving is
/// miss-driven: the fast path pays nothing, and a workload that outgrows one
/// reserve gets another (upstream does the same; `MAX_ARENAS` bounds the
/// total). A failed reserve latches off so an OOM-adjacent process does not
/// hammer the OS on every allocation.
fn reserve_default_arena_on_miss() -> bool {
    // 0 = idle, 1 = a thread is reserving, 2 = failed permanently.
    static RESERVE: AtomicUsize = AtomicUsize::new(0);
    if !DEFAULT_ARENA_PAYS {
        return false;
    }
    let reserve = crate::options::get_size(23); // arena_reserve (KiB-scaled)
    if reserve < SEGMENT_SIZE {
        return false;
    }
    match RESERVE.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => {
            let ok = reserve_os_memory_ex(reserve, true, false, false).is_ok();
            RESERVE.store(if ok { 0 } else { 2 }, Ordering::Release);
            ok
        }
        Err(1) => {
            // Another thread is reserving right now: wait it out, then rescan
            // — its fresh arena serves this miss too. Do NOT reserve again.
            while RESERVE.load(Ordering::Acquire) == 1 {
                core::hint::spin_loop();
            }
            RESERVE.load(Ordering::Acquire) == 0
        }
        Err(_) => false,
    }
}

/// Allocate one chunk. `restrict_id >= 0` → only that arena; else any
/// NON-exclusive arena (reserving the default arena on first miss). Returns
/// (chunk ptr, chunk-is-zero).
pub fn chunk_alloc(restrict_id: i32) -> Option<(*mut u8, bool)> {
    // No arenas on a one-region target: every segment comes straight from the
    // region. Folding this here is what removes the registry scan from the
    // segment-allocation path on a chip.
    if crate::FIXED_REGION {
        return None;
    }
    if restrict_id < 0 && crate::options::is_enabled(27) {
        return None; // disallow_arena_alloc
    }
    if let Some(r) = chunk_alloc_inner(restrict_id) {
        return Some(r);
    }
    // Exhausted (or empty) table. An exclusive-arena heap gets no default
    // arena; otherwise reserve one more and rescan once.
    if restrict_id >= 0 || !reserve_default_arena_on_miss() {
        return None;
    }
    chunk_alloc_inner(restrict_id)
}

#[allow(clippy::needless_range_loop)] // indexed scan over a fixed atomic table
fn chunk_alloc_inner(restrict_id: i32) -> Option<(*mut u8, bool)> {
    let n = ARENA_COUNT.load(Ordering::Acquire).min(MAX_ARENAS);
    for id in 0..n {
        if restrict_id >= 0 && id != restrict_id as usize {
            continue;
        }
        let a = ARENAS[id].load(Ordering::Acquire);
        if a.is_null() {
            continue;
        }
        // SAFETY: arena descriptors are live for the process.
        unsafe {
            if restrict_id < 0 && (*a).exclusive {
                continue;
            }
            let chunks = (*a).chunks_live.load(Ordering::Acquire);
            let words = chunks.div_ceil(WORD_BITS);
            for w in 0..words {
                loop {
                    let cur = (*a).used[w].load(Ordering::Acquire);
                    let limit = if (w + 1) * WORD_BITS <= chunks {
                        WORD_BITS
                    } else {
                        chunks % WORD_BITS
                    };
                    let free_bits = !cur
                        & if limit == WORD_BITS {
                            BitWord::MAX
                        } else {
                            ((1 as BitWord) << limit) - 1
                        };
                    if free_bits == 0 {
                        break;
                    }
                    let bit = free_bits.trailing_zeros() as usize;
                    if (*a).used[w]
                        .compare_exchange_weak(
                            cur,
                            cur | (1 << bit),
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_err()
                    {
                        continue;
                    }
                    let idx = w * WORD_BITS + bit;
                    let was_dirty =
                        (*a).dirty[w].fetch_or(1 << bit, Ordering::AcqRel) & (1 << bit) != 0;
                    let p = (*a).base.add(idx * SEGMENT_SIZE);
                    return Some((p, !was_dirty));
                }
            }
        }
    }
    None
}

static MULTI_LOCK: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Allocate `n` CONTIGUOUS chunks (huge blocks). Single spinlock for the
/// multi-bit search; the single-chunk path stays lock-free.
#[allow(clippy::needless_range_loop)] // indexed scan over a fixed atomic table
pub fn chunk_alloc_n(restrict_id: i32, n: usize) -> Option<(*mut u8, bool)> {
    if crate::FIXED_REGION {
        return None;
    }
    if n == 0 {
        // Zero chunks is None, not `run == 0` on a used bit handing out a
        // pointer with no claim (OH-rusty_alloc-22).
        return None;
    }
    if n == 1 {
        return chunk_alloc(restrict_id);
    }
    if restrict_id < 0 && crate::options::is_enabled(27) {
        return None;
    }
    if let Some(r) = chunk_alloc_n_inner(restrict_id, n) {
        return Some(r);
    }
    if restrict_id >= 0 || !reserve_default_arena_on_miss() {
        return None;
    }
    chunk_alloc_n_inner(restrict_id, n)
}

#[allow(clippy::needless_range_loop)] // indexed scan over a fixed atomic table
fn chunk_alloc_n_inner(restrict_id: i32, n: usize) -> Option<(*mut u8, bool)> {
    while MULTI_LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    let result = (|| {
        let count = ARENA_COUNT.load(Ordering::Acquire).min(MAX_ARENAS);
        for id in 0..count {
            if restrict_id >= 0 && id != restrict_id as usize {
                continue;
            }
            let a = ARENAS[id].load(Ordering::Acquire);
            if a.is_null() {
                continue;
            }
            // SAFETY: live descriptor; index arithmetic bounded by chunks.
            unsafe {
                if restrict_id < 0 && (*a).exclusive {
                    continue;
                }
                let chunks = (*a).chunks_live.load(Ordering::Acquire);
                if n > chunks {
                    continue;
                }
                let mut run = 0usize;
                let mut idx = 0usize;
                while idx < chunks {
                    let bit = (*a).used[idx / WORD_BITS].load(Ordering::Acquire)
                        & (1 << (idx % WORD_BITS));
                    run = if bit == 0 { run + 1 } else { 0 };
                    if run == n {
                        let start = idx + 1 - n;
                        // CLAIM-AND-VERIFY. MULTI_LOCK excludes other
                        // multi-chunk claims but NOT the lock-free
                        // single-chunk path, which can take one of these
                        // chunks between our scan and our claim. fetch_or
                        // reports the previous bit: on conflict, roll back
                        // exactly what we claimed and rescan past the thief.
                        // Without this, two segments land on ONE address —
                        // the parallel-test corruption found in M8.
                        let mut conflict = None;
                        for j in start..=idx {
                            let prev = (*a).used[j / WORD_BITS]
                                .fetch_or(1 << (j % WORD_BITS), Ordering::AcqRel);
                            if prev & (1 << (j % WORD_BITS)) != 0 {
                                conflict = Some(j);
                                break;
                            }
                        }
                        if let Some(c) = conflict {
                            for j in start..c {
                                (*a).used[j / WORD_BITS]
                                    .fetch_and(!(1 << (j % WORD_BITS)), Ordering::AcqRel);
                            }
                            run = 0;
                            idx = c + 1;
                            continue;
                        }
                        let mut any_dirty = false;
                        for j in start..=idx {
                            any_dirty |= (*a).dirty[j / WORD_BITS]
                                .fetch_or(1 << (j % WORD_BITS), Ordering::AcqRel)
                                & (1 << (j % WORD_BITS))
                                != 0;
                        }
                        let p = (*a).base.add(start * SEGMENT_SIZE);
                        return Some((p, !any_dirty));
                    }
                    idx += 1;
                }
            }
        }
        None
    })();
    MULTI_LOCK.store(false, Ordering::Release);
    result
}

/// Free `n` contiguous chunks. True when the address belonged to an arena.
#[allow(clippy::needless_range_loop)] // indexed scan over a fixed atomic table
pub fn chunk_free_n(p: *mut u8, n: usize) -> bool {
    if crate::FIXED_REGION {
        return false; // nothing came from an arena, so nothing returns to one
    }
    // Zero / overflowing `n` is false, not `start..start+n` OOB on the bitmap
    // (OH-rusty_alloc-25).
    if n == 0 {
        return false;
    }
    let addr = p.addr();
    let count = ARENA_COUNT.load(Ordering::Acquire).min(MAX_ARENAS);
    for id in 0..count {
        let a = ARENAS[id].load(Ordering::Acquire);
        if a.is_null() {
            continue;
        }
        // SAFETY: live descriptor; indices clamped to live chunks.
        unsafe {
            let chunks = (*a).chunks_live.load(Ordering::Acquire);
            let Some(span) = chunks.checked_mul(SEGMENT_SIZE) else {
                continue;
            };
            let Some(end_addr) = (*a).base.addr().checked_add(span) else {
                continue;
            };
            if addr < (*a).base.addr() || addr >= end_addr {
                continue;
            }
            let off = addr - (*a).base.addr();
            // Interior pointers are false, not a successful free of the
            // containing chunk (OH-rusty_alloc-27).
            if !off.is_multiple_of(SEGMENT_SIZE) {
                return false;
            }
            let start = off / SEGMENT_SIZE;
            let Some(end) = start.checked_add(n) else {
                return false;
            };
            if end > chunks {
                return false;
            }
            for j in start..end {
                (*a).used[j / WORD_BITS].fetch_and(!(1 << (j % WORD_BITS)), Ordering::AcqRel);
            }
            return true;
        }
    }
    false
}

/// Return a chunk to its arena. True when the address belonged to one.
#[allow(clippy::needless_range_loop)] // indexed scan over a fixed atomic table
pub fn chunk_free(p: *mut u8) -> bool {
    if crate::FIXED_REGION {
        return false;
    }
    let addr = p.addr();
    let n = ARENA_COUNT.load(Ordering::Acquire).min(MAX_ARENAS);
    for id in 0..n {
        let a = ARENAS[id].load(Ordering::Acquire);
        if a.is_null() {
            continue;
        }
        // SAFETY: live descriptor; bit index bounded by the range check.
        unsafe {
            let chunks = (*a).chunks_live.load(Ordering::Acquire);
            let Some(span) = chunks.checked_mul(SEGMENT_SIZE) else {
                continue;
            };
            let Some(end_addr) = (*a).base.addr().checked_add(span) else {
                continue;
            };
            if addr >= (*a).base.addr() && addr < end_addr {
                let off = addr - (*a).base.addr();
                // Interior pointers are false, not a successful free of the
                // containing chunk (OH-rusty_alloc-27).
                if !off.is_multiple_of(SEGMENT_SIZE) {
                    return false;
                }
                let idx = off / SEGMENT_SIZE;
                (*a).used[idx / WORD_BITS].fetch_and(!(1 << (idx % WORD_BITS)), Ordering::AcqRel);
                return true;
            }
        }
    }
    false
}

/// Adopt an OS block that cannot be returned to the host, making its memory
/// recyclable through this module. This is the free list `prim/wasm.rs`'s
/// no-op `free` depends on: [`crate::os::free`] calls it (only) on platforms
/// where `prim::FREE_RETURNS_MEMORY` is false, at the moment a segment or
/// huge block is released. Without it every such release strands its whole
/// mapping — v1.1.4 leaked one 32 MiB segment per >16 MiB alloc/free cycle on
/// wasm this way.
///
/// The block becomes `size / SEGMENT_SIZE` chunks, all free, all pre-marked
/// dirty (they held live data; a future tenant must not treat them as zero —
/// the same rule [`manage_os_memory_ex`] applies to adopted host memory).
///
/// **Coalescing:** consecutive `memory.grow` reservations are address-
/// contiguous, so a freed block usually lands exactly at the end of a
/// previously adopted arena. When it does — and the arena is ours,
/// non-exclusive, and has bitmap headroom — the arena is EXTENDED in place
/// rather than a new one registered: the new chunks' dirty bits are published
/// before the enlarged `chunks`/`size` counts (`Release`/`Acquire`), so a
/// scanner never sees a chunk whose state it cannot trust. Extension is what
/// keeps the fixed `MAX_ARENAS` table from filling: the common
/// grow-free-grow-free pattern collapses into ONE arena that tracks the peak
/// footprint, instead of one slot per 32 MiB forever.
///
/// Returns the arena id (existing on extension, fresh on registration), or
/// `None` for a block this module cannot recycle — not chunk-aligned, not a
/// whole number of chunks, or the table is full — in which case the caller
/// falls through to `prim::free` and, on a no-return platform, the block is
/// lost (which is why [`crate::os::alloc_aligned`] rounds segment-aligned
/// requests up to whole chunks there).
///
/// Registration allocates the descriptor through [`os::alloc_aligned`]; that
/// is an ALLOCATION, never a free, so it cannot re-enter this function.
#[allow(clippy::needless_range_loop)] // indexed scan over a fixed atomic table
pub(crate) fn adopt_os_block(ptr: *mut u8, size: usize) -> Option<i32> {
    let addr = ptr.addr();
    if size == 0 || !addr.is_multiple_of(SEGMENT_SIZE) || !size.is_multiple_of(SEGMENT_SIZE) {
        return None;
    }
    let n = size / SEGMENT_SIZE;
    // Serialise against other adopters and the multi-chunk allocator; the
    // lock-free single-chunk paths are ordered by the Release publication
    // below. (On wasm, the only platform whose os::free reaches here, there
    // is exactly one thread anyway.)
    while MULTI_LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    let id = (|| {
        let count = ARENA_COUNT.load(Ordering::Acquire).min(MAX_ARENAS);
        for id in 0..count {
            let a = ARENAS[id].load(Ordering::Acquire);
            if a.is_null() {
                continue;
            }
            // SAFETY: live descriptor; extension writes stay inside the
            // fixed-size bitmap arrays per the MAX_CHUNKS bound.
            unsafe {
                if (*a).exclusive || !(*a).owned {
                    continue;
                }
                let chunks = (*a).chunks_live.load(Ordering::Acquire);
                if (*a).base.addr() + chunks * SEGMENT_SIZE != addr || chunks + n > MAX_CHUNKS {
                    continue;
                }
                // Dirty bits FIRST, counts after: a reader that observes the
                // new count must observe the new chunks as dirty.
                for j in chunks..chunks + n {
                    (*a).dirty[j / WORD_BITS].fetch_or(1 << (j % WORD_BITS), Ordering::AcqRel);
                    (*a).used[j / WORD_BITS].fetch_and(!(1 << (j % WORD_BITS)), Ordering::AcqRel);
                }
                (*a).chunks_live.store(chunks + n, Ordering::Release);
                return Some(id as i32);
            }
        }
        None
    })();
    MULTI_LOCK.store(false, Ordering::Release);
    if id.is_some() {
        return id;
    }
    // No adjacent arena: register the block as its own. Chunks start free;
    // pre-mark them dirty exactly as manage_os_memory_ex does.
    let id = arena_register(ptr, size, false, true, -1).ok()?;
    let a = ARENAS[id as usize].load(Ordering::Acquire);
    // SAFETY: freshly registered live descriptor.
    unsafe {
        for j in 0..n {
            (*a).dirty[j / WORD_BITS].fetch_or(1 << (j % WORD_BITS), Ordering::AcqRel);
        }
    }
    Some(id)
}

/// `mi_arena_area`: the arena's base and size, or null.
pub fn arena_area(id: i32) -> (*mut u8, usize) {
    // Index by the table bound, not a count that can race past it
    // (`ARENAS` is `MAX_ARENAS` slots).
    if crate::FIXED_REGION || id < 0 || (id as usize) >= MAX_ARENAS {
        return (ptr::null_mut(), 0);
    }
    let a = ARENAS[id as usize].load(Ordering::Acquire);
    if a.is_null() {
        return (ptr::null_mut(), 0);
    }
    // SAFETY: live descriptor.
    unsafe {
        let chunks = (*a).chunks_live.load(Ordering::Acquire);
        ((*a).base, chunks.checked_mul(SEGMENT_SIZE).unwrap_or(0))
    }
}

/// Debug print of arena occupancy (mi_debug_show_arenas / mi_arenas_print).
///
/// std-only: each line is built as an owned string (P3, §2.5). The occupancy
/// itself is readable without it — `arena_area` and the chunk counters are not
/// gated; only the human formatting is.
#[cfg(feature = "std")]
#[allow(clippy::needless_range_loop)] // indexed scan over a fixed atomic table
pub fn arenas_print(out: &mut dyn FnMut(&str)) {
    let n = ARENA_COUNT.load(Ordering::Acquire).min(MAX_ARENAS);
    if n == 0 {
        out("arenas: none\n");
        return;
    }
    for id in 0..n {
        let a = ARENAS[id].load(Ordering::Acquire);
        if a.is_null() {
            continue;
        }
        // SAFETY: live descriptor.
        unsafe {
            let chunks = (*a).chunks_live.load(Ordering::Acquire);
            let mut used = 0usize;
            for w in 0..chunks.div_ceil(WORD_BITS) {
                used += (*a).used[w].load(Ordering::Relaxed).count_ones() as usize;
            }
            let mut line = heapless_fmt(
                id,
                (*a).base.addr(),
                chunks * SEGMENT_SIZE,
                used,
                chunks,
                (*a).exclusive,
            );
            out(line.as_str());
            line.clear();
        }
    }
}

// Tiny fixed formatting helper (no allocation inside the allocator's own
// diagnostics).
#[cfg(feature = "std")]
fn heapless_fmt(
    id: usize,
    base: usize,
    size: usize,
    used: usize,
    chunks: usize,
    excl: bool,
) -> std::string::String {
    std::format!(
        "arena {id}: base {base:#x} size {} MiB, {used}/{chunks} chunks used{}\n",
        size / (1024 * 1024),
        if excl { " (exclusive)" } else { "" }
    )
}

#[cfg(test)]
mod adopt_tests {
    use super::*;

    /// One lock for all adoption tests. They are the only adopters on native
    /// (adoption runs on wasm), but their OS blocks can be ADJACENT — on
    /// Windows, VirtualAlloc handed two concurrently-running tests
    /// contiguous reservations, the second test's adoption coalesced into
    /// the first test's arena exactly as designed, and the first test's
    /// "exactly two chunks" assertion failed. The production behaviour was
    /// correct; the tests' isolation assumption was not. Serialising them
    /// removes the concurrency half; the assertions below are additionally
    /// written to survive LANDING in a shared or pre-extended arena, because
    /// arenas are never unregistered and an earlier test's arena can adopt a
    /// later test's adjacent block.
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Drain `chunk_alloc(id)` (bounded by the table's capacity), returning
    /// every chunk obtained. The caller inspects the subset that lies in its
    /// own block and frees everything.
    fn drain(id: i32) -> Vec<(*mut u8, bool)> {
        let mut held = Vec::new();
        while let Some(got) = chunk_alloc(id) {
            held.push(got);
            assert!(held.len() <= MAX_CHUNKS, "drain exceeded arena capacity");
        }
        held
    }

    fn free_all(held: &[(*mut u8, bool)]) {
        for &(p, _) in held {
            assert!(chunk_free(p), "drained chunk did not free back");
        }
    }

    /// The wasm leak, replayed natively: a chunk-granular OS block is
    /// adopted, and the SAME memory comes back from chunk_alloc — restricted
    /// to the adopted arena's id, tolerating extra chunks in case this block
    /// coalesced into an earlier test's arena.
    #[test]
    fn adopted_block_recycles_through_chunk_alloc() {
        let _g = lock();
        let b = os::alloc_aligned(2 * SEGMENT_SIZE, SEGMENT_SIZE, true, false)
            .expect("64 MiB test reservation");
        let id = adopt_os_block(b.ptr, 2 * SEGMENT_SIZE).expect("adoptable block");
        let ours = [b.ptr.addr(), b.ptr.addr() + SEGMENT_SIZE];

        for round in 0..3 {
            let held = drain(id);
            let mut got: Vec<usize> = held
                .iter()
                .map(|&(p, _)| p.addr())
                .filter(|a| ours.contains(a))
                .collect();
            got.sort_unstable();
            assert_eq!(got, ours, "round {round}: adopted chunks not recycled");
            // Adoption must pre-mark chunks dirty: the block held live data,
            // so a tenant that trusted a zero claim would read stale bytes.
            for &(p, zero) in &held {
                if ours.contains(&p.addr()) {
                    assert!(!zero, "adopted chunk claimed to be zero");
                }
            }
            free_all(&held);
        }
        // Deliberately NOT os::free'd: arenas hold their memory for the
        // process lifetime, which is the adoption contract.
    }

    /// A block that lands exactly at an adopted arena's end EXTENDS it —
    /// same id, larger live area — because on wasm consecutive `memory.grow`
    /// blocks are contiguous and one growing arena is what keeps the fixed
    /// MAX_ARENAS table from filling.
    #[test]
    fn adjacent_adoption_extends_in_place() {
        let _g = lock();
        let b = os::alloc_aligned(3 * SEGMENT_SIZE, SEGMENT_SIZE, true, false)
            .expect("96 MiB test reservation");
        let first = adopt_os_block(b.ptr, SEGMENT_SIZE).expect("first block");
        // SAFETY: one chunk into the same live reservation.
        let mid = unsafe { b.ptr.add(SEGMENT_SIZE) };
        let second = adopt_os_block(mid, 2 * SEGMENT_SIZE).expect("adjacent block");
        // The arena ending at `mid` is unique (regions never overlap), so
        // wherever the first chunk landed — a fresh arena, or coalesced into
        // an earlier test's — the second adoption must land in that same one.
        assert_eq!(
            first, second,
            "adjacent block registered a new arena instead of extending"
        );
        let (base, sz) = arena_area(first);
        assert!(
            base.addr() <= b.ptr.addr() && base.addr() + sz >= b.ptr.addr() + 3 * SEGMENT_SIZE,
            "extension did not publish the enlarged area"
        );
        let ours = [
            b.ptr.addr(),
            b.ptr.addr() + SEGMENT_SIZE,
            b.ptr.addr() + 2 * SEGMENT_SIZE,
        ];
        let held = drain(first);
        let mut got: Vec<usize> = held
            .iter()
            .map(|&(p, _)| p.addr())
            .filter(|a| ours.contains(a))
            .collect();
        got.sort_unstable();
        assert_eq!(got, ours, "extended arena did not serve all three chunks");
        for &(p, zero) in &held {
            if ours.contains(&p.addr()) {
                assert!(!zero, "extended chunk claimed to be zero");
            }
        }
        free_all(&held);
    }

    /// Blocks the arena cannot recycle are refused, so os::free falls through
    /// to the prim rather than corrupting the chunk map.
    #[test]
    fn ragged_blocks_are_refused() {
        let _g = lock();
        let b = os::alloc_aligned(SEGMENT_SIZE, SEGMENT_SIZE, true, false)
            .expect("32 MiB test reservation");
        // SAFETY: interior pointer / in-range sizes of a live reservation.
        unsafe {
            let misaligned = b.ptr.add(os::page_size());
            assert!(adopt_os_block(misaligned, SEGMENT_SIZE - os::page_size()).is_none());
            assert!(adopt_os_block(b.ptr, SEGMENT_SIZE / 2).is_none());
            assert!(adopt_os_block(b.ptr, 0).is_none());
        }
        let freed = os::OsBlock {
            ptr: b.ptr,
            size: b.size,
            is_large: false,
            is_zero: false,
        };
        // SAFETY: our unfreed reservation with no live references.
        unsafe { os::free(freed).expect("native free") };
    }
}
