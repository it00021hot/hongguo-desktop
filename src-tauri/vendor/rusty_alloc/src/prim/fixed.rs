//! Fixed-region prim backend: memory is a range someone hands us, once.
//!
//! P1 of `docs/plans/small-metal.md`. This is the backend for a target with no
//! OS at all — a microcontroller, where "memory" is a region the linker
//! reserved and there is no `mmap`, no `VirtualAlloc`, and nothing to give it
//! back to. It is the second implementation of the prim seam that P1 asks for;
//! the first is the four platform arms in [`super`].
//!
//! It is **always compiled** so that it is type-checked and unit-tested on the
//! host, and **selected** only where no platform arm matches. Nothing about a
//! host build changes because this file exists.
//!
//! Every consequence of "the region is all there is", and what each one costs:
//!
//! - **The backend cannot allocate.** It *is* the allocator's memory source, so
//!   its own bookkeeping must be a fixed static: [`MAX_EXTENTS`] free extents in
//!   two `AtomicUsize` arrays, guarded by a spin lock. That bound is a real
//!   limit — a fragmentation pattern needing more than [`MAX_EXTENTS`] holes
//!   fails the free rather than corrupting anything (see [`free`]).
//! - **`free` genuinely frees**, unlike the wasm backend: an extent returns to
//!   the list and coalesces with its neighbours. So
//!   [`super::FREE_RETURNS_MEMORY`] is true here and the arena's adopt-on-free
//!   path folds away, as it does on every platform with a working `free`.
//! - **There is no MMU**, so [`commit`] / [`decommit`] / [`reset`] are no-ops
//!   over memory that is always backed, and [`protect`] returns an error rather
//!   than pretending. That is the same call the wasm backend makes and for the
//!   same reason: a guard page that cannot trap would let a `secure` build
//!   claim a hardening it does not have.
//! - **There is no clock and one thread**, so [`clock_now`] is a monotonic
//!   counter (purge *ordering* survives; duration does not) and [`thread_id`] is
//!   a non-zero constant. TLS is a fixed static table whose destructors never
//!   run, because there is no thread exit to run them at.
//! - **Memory is not known-zero.** A `.bss` region starts zeroed, but a range
//!   handed back by [`free`] and re-served does not, and the backend cannot tell
//!   the two apart. [`alloc`] therefore reports `is_zero: false` always, which
//!   is the conservative direction: a caller that needs zeros writes them.
//!
//! - **Segments stride from the region's base, not from address zero.** The
//!   allocator above recovers a block's segment by masking its address; on a
//!   hosted target the mask runs from zero, so every segment — and a region
//!   holding them — must be `SEGMENT_SIZE`-aligned. Here `segment_of` masks
//!   the offset from [`stride_base`] instead (`crate::REGION_STRIDES`), so
//!   [`alloc`] measures every alignment from that base, a region needs only
//!   [`REGION_ALIGN`] (16 bytes), and the linker leaves no segment-sized gap
//!   in front of it: 24,148 bytes returned on the firmware that measured the
//!   gap (`docs/plans/finished/region-alignment-dissolve.md`). The mask, and
//!   the segment alignment with it, is one flag away for a firmware that
//!   would rather have the three instructions per free: `--cfg
//!   ra_aligned_region`, which every rule in this file follows.
//!
//! **What this backend does NOT solve, and P1 does not claim it does:** at the
//! shipped 32 MiB geometry a chip-sized region cannot hold one segment, and
//! [`init_region`] refuses it with [`FERR_GEOMETRY`]. That is §2.1 of
//! `small-metal.md`, it was P2's work (`--cfg ra_small_profile`), and it shows
//! up here as an honest `Err` rather than as anything this file can fix.

use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use super::{Alloc, MemConfig, TlsDtor, align_up};

/// The error type every fallible entry point here returns, re-exported so the
/// whole fixed-region recipe is reachable from ONE path.
///
/// It has always been public as [`crate::prim::PrimError`], but only from the
/// parent module — so a seam re-exporting this API in one `pub use` could name
/// `Region`, `good_region_size`, `init_region` and the `FERR_*` values but not
/// the type they fail with. The Kairos RTOS allocator seam hit exactly that
/// and carried a "arrives with the next release" comment for it
/// (`rusty_rtos_alloc::small_metal`, `rusty_RTOS/docs/plans/build-me-bare.md`
/// B3). Same type, one more path.
pub use super::PrimError;

/// Synthetic error code. The backends surface no errno, so any non-zero
/// sentinel does; this one is distinct from wasm's `0xBEEF` and the mock's.
const FERR: PrimError = 0xF13D;

/// The region is smaller than one [`FIXED_PAGE`].
pub const FERR_TOO_SMALL: PrimError = 0xF13E;

/// The region cannot hold a single `SEGMENT_SIZE` segment at the ACTIVE
/// geometry, so the allocator above this backend could never serve anything.
///
/// Almost always one missing flag: `--cfg ra_small_profile` keeps
/// `SEGMENT_SIZE` at 32 MiB, and a kilobyte-scale region yields zero segments.
pub const FERR_GEOMETRY: PrimError = 0xF13F;

/// A region is already registered; this backend takes one, once.
pub const FERR_REGISTERED: PrimError = 0xF140;

/// The region's BASE costs at least one whole segment: from this address the
/// region yields fewer segments than the same length would from a
/// [`REGION_ALIGN`]-aligned one.
///
/// This is the shape a caller who sized the region *exactly* — with
/// [`good_region_size`], `k * SEGMENT_SIZE`, the README — hits when the
/// container holding it has alignment 1, which is what a plain
/// `static [u8; N]` has. The Janus firmware did exactly that: a
/// `good_region_size(220 * 1024)` region at the base the linker chose,
/// `0x3fc8a1e4`, lost 24,092 bytes to the first segment boundary and served
/// two segments where the number said three, and the first allocation past
/// 128 KiB panicked in `handle_alloc_error` — 484 bytes short of the round
/// number that had happened to work
/// (`docs/plans/finished/region-alignment-bug.md`). `init_region` had the
/// exact answer in hand and threw it away; now it refuses instead.
///
/// Since segments stride from the region's base rather than from zero
/// (`docs/plans/finished/region-alignment-dissolve.md`) the boundary that
/// matters is 16 bytes, not 64 KiB: the same address now loses 12 bytes to
/// [`REGION_ALIGN`], which still costs an EXACT length its last segment, so
/// the refusal stands. The fix is [`Region`], which is aligned by
/// construction and carries no padding.
pub const FERR_MISALIGNED: PrimError = 0xF141;

/// The smallest region this backend will accept, for a [`REGION_ALIGN`]-aligned
/// base: one segment. The heap descriptor is not in it — on this backend the
/// first heap's descriptor lives in a static of its own ([`take_first_heap_box`]),
/// so a region is whole segments and nothing else. (Until 2.0.3 this was
/// `SEGMENT_SIZE + FIXED_PAGE`, the page being the descriptor; the rewrite is
/// §7 of `docs/plans/finished/region-alignment-bug.md`.)
///
/// Exposed so a firmware can settle its budget at COMPILE time rather than on
/// silicon, which is what the first outside adopter asked for
/// (`docs/plans/embedded-adoption.md`):
///
/// ```ignore
/// const _: () = assert!(REGION_BYTES >= rusty_alloc::prim::fixed::MIN_REGION);
/// ```
///
/// A base off the 16-byte grid needs up to `REGION_ALIGN - 1` more, because
/// the first segment starts at the first [`REGION_ALIGN`]-aligned address;
/// [`init_region`] checks the real base and is therefore exact where this
/// constant is optimistic. [`Region`] is aligned, and the two agree.
pub const MIN_REGION: usize = crate::types::SEGMENT_SIZE;

/// Page granularity reported to the layers above. A chip has no paging
/// hardware, so this is a bookkeeping unit rather than a hardware fact; 4 KiB
/// matches the flash/RAM block size the ESP parts use and keeps `page_align_up`
/// rounding modest on a region measured in tens of kilobytes.
/// The backend's page: the granule of every request that is not a segment.
pub const FIXED_PAGE: usize = 4096;

/// The alignment a region's base needs: `MAX_ALIGN_SIZE`, 16 bytes.
///
/// Segments are carved at `SEGMENT_SIZE` strides from the region's first
/// `REGION_ALIGN`-aligned address, and `segment_of` resolves against that
/// address rather than masking from zero (`crate::REGION_STRIDES`). So the
/// base decides the alignment of every block — a page area is a whole number
/// of slices past a segment, a block a whole number of block sizes past a
/// page area — and 16 is exactly the natural alignment the allocator promises
/// (`MAX_ALIGN_SIZE`); coarser requests are placed by address, as they are
/// everywhere. Until 2.0.4 this was `SEGMENT_SIZE`, and a linker paid up to
/// `SEGMENT_SIZE - 1` bytes of gap in front of the region to honour it
/// (`docs/plans/finished/region-alignment-dissolve.md`).
///
/// `SEGMENT_SIZE` again under `--cfg ra_aligned_region`, which restores the
/// hosted mask on the free path — three instructions fewer per segment
/// resolution on the ESP32-S3 — at the price of the alignment, and of the
/// gap. Every sizing rule here, [`Region`]'s alignment and the backend's
/// placement follow this constant, so the two arms cannot disagree.
pub const REGION_ALIGN: usize = if cfg!(ra_aligned_region) {
    crate::types::SEGMENT_SIZE
} else {
    crate::types::MAX_ALIGN_SIZE
};

// Every header carved at a stride from the base must be satisfied by it.
const _: () = assert!(
    core::mem::align_of::<crate::segment::Segment>() <= REGION_ALIGN
        && core::mem::align_of::<crate::init::HeapBox>() <= REGION_ALIGN,
    "a stride from a REGION_ALIGN-aligned base must satisfy every header"
);

/// Free extents tracked at once.
///
/// The bound exists because this backend cannot allocate its own bookkeeping.
/// 32 is far more than a chip needs — the layers above make a handful of large
/// reservations, not many small ones — and exceeding it is reported, never
/// papered over.
/// Free-extent slots. `--cfg ra_max_extents="8"` (or `"16"`, `"64"`) resizes
/// the two tables; the default is 32.
///
/// This is a real bound, not a hint: a [`free`] that would need a slot the
/// table does not have is refused, and that range is lost to the allocator
/// until a neighbouring free coalesces over it. A free extent is bounded on
/// each side by a live block or the region's end, so the table can never need
/// more slots than there are live blocks plus one. A 220 KiB firmware region
/// holds three segments and one heap page, so it cannot need more than 5; a
/// 4 MiB PSRAM region holds 63 segments and can need more than 64. Each slot
/// is two words of `.bss`, and on a chip that is stack the linker did not
/// get, which is why the number is a knob rather than a constant
/// (`firmware-what-is-left.md` §3). Size it from the live-block bound, not
/// from hope.
const MAX_EXTENTS: usize = if cfg!(ra_max_extents = "8") {
    8
} else if cfg!(ra_max_extents = "16") {
    16
} else if cfg!(ra_max_extents = "64") {
    64
} else {
    32
};

/// The region, published once by [`init_region`]. Zero length means "no region
/// yet", which every entry point checks.
static REGION_BASE: AtomicUsize = AtomicUsize::new(0);
static REGION_LEN: AtomicUsize = AtomicUsize::new(0);

/// The free list: `EXT_BASE[i] .. EXT_BASE[i] + EXT_LEN[i]`, kept sorted by
/// base so that coalescing is a look at the two neighbours. Only ever touched
/// with [`LOCK`] held, so plain `Relaxed` access is correct.
static EXT_BASE: [AtomicUsize; MAX_EXTENTS] = [const { AtomicUsize::new(0) }; MAX_EXTENTS];
static EXT_LEN: [AtomicUsize; MAX_EXTENTS] = [const { AtomicUsize::new(0) }; MAX_EXTENTS];
static EXT_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Spin lock over the free list. On the single-threaded target this backend is
/// for it never contends; it is here so the statics are sound under the
/// `Sync` the seam requires, not for throughput.
static LOCK: AtomicBool = AtomicBool::new(false);

/// Guards one named lock. Not reentrant — hold at most one at a time, and
/// never call out to something that takes the same one.
struct Guard(&'static AtomicBool);

impl Guard {
    fn acquire(lock: &'static AtomicBool) -> Self {
        while lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            // Under `ra_single_threaded` a genuinely contended acquire is not a
            // race, because there is no second thread to race with. It can only
            // be REENTRANCY: an interrupt handler that allocated while the main
            // context was inside the allocator. That wedges forever -- the
            // preempted context can never run to release the lock -- and
            // surfaces as a watchdog reset with a backtrace pointing into
            // `spin_loop`, which names nothing.
            //
            // The LOAD is not redundant. `compare_exchange_weak` may fail
            // SPURIOUSLY, so a failed CAS is not by itself proof of anything;
            // only a lock actually observed held is. Getting this wrong would
            // panic firmwares at random, which is worse than the hang it
            // replaces.
            #[cfg(ra_single_threaded)]
            if lock.load(Ordering::Relaxed) {
                reentered();
            }
            core::hint::spin_loop();
        }
        Self(lock)
    }
}

/// The allocator was re-entered on a target that promised one context.
///
/// Separated and `#[cold]` so the happy path is unchanged: the CAS already
/// happens, and only its failure arm gains a load and a call that never
/// returns.
///
/// If the firmware's panic handler itself allocates it will re-enter here and
/// panic again, which aborts. That is a defined ending and a diagnosable one;
/// the behaviour being replaced is an unbounded spin with no message at all.
#[cfg(ra_single_threaded)]
#[cold]
#[inline(never)]
fn reentered() -> ! {
    // A literal, not a format: `core::fmt` is not on this crate's `no_std`
    // budget, and this message must survive a build that has no formatter.
    panic!(
        "rusty_alloc: the allocator was re-entered. On a target built with \
         --cfg ra_single_threaded nothing else can hold this lock, so this is \
         almost certainly an interrupt handler that allocated while the main \
         context was inside the allocator. prim::fixed's lock is NOT \
         reentrant: do not allocate in an ISR. Note that ra_single_threaded \
         means single CONTEXT, and an interrupt handler is a second context on \
         one core."
    )
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// How many bytes of `[base, base + len)` can ever back SEGMENTS.
///
/// The rule this answers used to live only in a design document
/// (`docs/plans/small-metal.md`: *"size an embedded region as
/// `k * 64 KiB + 4 KiB`, or the tail is dead to large allocations"*), which
/// meant a firmware author picking a round number learned it by reading prose
/// or not at all. A 220 KiB region at the small profile yields three segments
/// and strands 24,576 bytes — 11 % of the budget, silently.
///
/// Segments are carved at `SEGMENT_SIZE` strides from the first
/// [`REGION_ALIGN`]-aligned address (16 bytes; until 2.0.4 it was the first
/// `SEGMENT_SIZE`-aligned one, and the run-up to it cost a 4-aligned base a
/// whole segment), so the answer is
/// `floor((end - first_aligned) / SEGMENT_SIZE) * SEGMENT_SIZE`: at most 15
/// leading bytes are unusable, and everything from there is whole segments.
/// Nothing is reserved for the heap
/// descriptor: on this backend the first heap's descriptor is a static
/// ([`take_first_heap_box`]), not a page of the region. (Until 2.0.3 one page
/// was reserved at the top for it, which is why the shipped sizing rule was
/// `k * SEGMENT_SIZE + FIXED_PAGE` — and why an aligned container of that size
/// paid `SEGMENT_SIZE - FIXED_PAGE` of padding: §7 of
/// `docs/plans/finished/region-alignment-bug.md`.)
///
/// **This is not the same question as [`region_stats`]'s `free`.** That reports
/// bytes nobody has taken, and the stranded tail is genuinely available to
/// page-sized allocations — so it is free, and it is also useless for segments.
/// Reporting one number as if it answered both is how a clean number ends up
/// measuring nothing; they are separate on purpose.
///
/// `const fn`, so a seam crate can size its region at compile time:
///
/// ```ignore
/// const _: () = assert!(usable_bytes(0, REGION_BYTES) > 0);
/// ```
#[must_use]
pub const fn usable_bytes(base: usize, len: usize) -> usize {
    let seg = crate::types::SEGMENT_SIZE;
    let Some(end) = base.checked_add(len) else {
        return 0;
    };
    // The first REGION_ALIGN-aligned address at or above `base`, without
    // overflowing: the origin every segment strides from.
    let Some(run_up) = base.checked_add(REGION_ALIGN - 1) else {
        return 0;
    };
    let first = run_up & !(REGION_ALIGN - 1);
    if first >= end {
        return 0;
    }
    let avail = end - first;
    (avail / seg) * seg
}

/// The largest region no bigger than `budget` that strands NOTHING, for a
/// [`REGION_ALIGN`]-aligned base: `k * SEGMENT_SIZE`.
///
/// [`usable_bytes`] lets a firmware *observe* the granule's loss; this is
/// what lets it *avoid* the loss, and it is pure arithmetic. A region is
/// carved into whole segments, so any size that is not a multiple of
/// `SEGMENT_SIZE` leaves the remainder dead to them. The Janus firmware that
/// reported it handed over 220 KiB and got 196,608 usable with 24,576
/// stranded — 11 % of its budget, and three times what the allocator's whole
/// code costs on that chip after 2.0.2
/// (`docs/plans/finished/firmware-what-is-left.md` §1).
///
/// `const fn`, so the answer is settled where the region is declared — in
/// [`Region`], which is [`REGION_ALIGN`]-aligned by construction and, because
/// its size is whole segments, carries no padding:
///
/// ```ignore
/// use rusty_alloc::prim::fixed::{Region, good_region_size};
/// static HEAP: Region<{ good_region_size(220 * 1024) }> = Region::new();
/// // 196,608 bytes: three 64 KiB segments, nothing stranded, 28,672 bytes
/// // of the budget handed back to the firmware's own use.
/// ```
///
/// **Use [`Region`], not a container of your own.** A plain `static [u8; N]`
/// has alignment 1: fifteen times in sixteen the linker puts it off the
/// 16-byte grid, this exact size then yields one segment fewer than its name
/// says, and [`init_region`] refuses it with [`FERR_MISALIGNED`] rather than
/// serve two thirds of the heap. An aligned
/// container of your own with the OLD shape (`k * SEGMENT_SIZE + FIXED_PAGE`,
/// the 2.0.3 rule) is worse: a type's size is rounded up to its alignment, so
/// `#[repr(align(65536))]` around 200,704 bytes occupies 262,144 — and the
/// Janus firmware that took that advice lost 60,952 bytes of stack to it.
/// Both halves of that history are in
/// `docs/plans/finished/region-alignment-bug.md`.
///
/// Rounds DOWN, because a budget is a ceiling: asking for the largest
/// zero-waste region that fits is the question a firmware with N bytes to
/// spare is asking. [`region_for`] is the other direction. Returns 0 when no
/// zero-waste region fits at all (`budget < MIN_REGION`), which [`Region`]'s
/// compile-time check turns into a build error.
#[must_use]
pub const fn good_region_size(budget: usize) -> usize {
    let seg = crate::types::SEGMENT_SIZE;
    (budget / seg) * seg
}

/// The smallest region that serves at least `usable` bytes of segments, for a
/// [`REGION_ALIGN`]-aligned base — [`good_region_size`] read from the other
/// end: a firmware that knows what it needs rather than what it can spare.
///
/// ```ignore
/// // "I need 192 KiB of heap": 196,608 bytes, three segments, and
/// // usable_bytes(0, 196_608) == 196_608 exactly.
/// use rusty_alloc::prim::fixed::{Region, region_for};
/// static HEAP: Region<{ region_for(192 * 1024) }> = Region::new();
/// ```
///
/// Rounds UP to whole segments; `usable == 0` still costs one segment,
/// because a region that can serve nothing is refused by [`init_region`].
#[must_use]
pub const fn region_for(usable: usize) -> usize {
    let seg = crate::types::SEGMENT_SIZE;
    let segments = if usable == 0 { 1 } else { usable.div_ceil(seg) };
    // Unrepresentable `usable` is 0, matching [`region_for_allocs`]. A region
    // of 0 is refused by [`init_region`].
    match segments.checked_mul(seg) {
        Some(bytes) => bytes,
        None => 0,
    }
}

/// What one allocation of a given size costs, and which path serves it —
/// the compile-time answer to "why did that size behave differently?".
///
/// Every field is derived from the active geometry, so it moves with
/// `--cfg ra_small_profile` and `--cfg ra_segment_size` instead of being a
/// second copy of the routing rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Shape {
    /// Bytes of page the request is served from: a small page, a medium page,
    /// its own span, or its own segments.
    pub page_bytes: usize,
    /// Whole segments this allocation reserves for ITSELF; `0` when it shares
    /// (see [`dedicated_segments`]).
    pub dedicated_segments: usize,
    /// Whether the request takes the `direct[]` fast-path route, i.e.
    /// `size <= SMALL_SIZE_MAX`.
    ///
    /// **This is the boundary that moves with POINTER WIDTH**, not with the
    /// profile: `SMALL_SIZE_MAX` is `128 * size_of::<usize>()`, so it is 1,024
    /// on a 64-bit host and **512 on a 32-bit chip**. A sweep that looks flat
    /// on a workstation can step on the device for that reason alone, which is
    /// exactly what the Kairos RTOS measured
    /// (`docs/plans/finished/fixed-prim-small-step.md`).
    pub direct_route: bool,
}

/// The [`Shape`] of an allocation of `size` bytes.
///
/// ```ignore
/// use rusty_alloc::prim::fixed::shape_of;
/// // On a 32-bit target this steps at 512; on a 64-bit one, at 1024.
/// const _: () = assert!(shape_of(512).direct_route);
/// ```
#[must_use]
pub const fn shape_of(size: usize) -> Shape {
    use crate::types::{
        LARGE_OBJ_SIZE_MAX, MEDIUM_OBJ_SIZE_MAX, MEDIUM_PAGE_SIZE, SEGMENT_SLICE_SIZE,
        SMALL_OBJ_SIZE_MAX, SMALL_PAGE_SIZE, SMALL_SIZE_MAX,
    };
    let dedicated = dedicated_segments(size);
    let page_bytes = if size <= SMALL_OBJ_SIZE_MAX {
        SMALL_PAGE_SIZE
    } else if size <= MEDIUM_OBJ_SIZE_MAX {
        MEDIUM_PAGE_SIZE
    } else if size <= LARGE_OBJ_SIZE_MAX {
        // Its own span of whole slices inside a shared segment.
        match size
            .div_ceil(SEGMENT_SLICE_SIZE)
            .checked_mul(SEGMENT_SLICE_SIZE)
        {
            Some(n) => n,
            None => 0,
        }
    } else {
        match dedicated.checked_mul(crate::types::SEGMENT_SIZE) {
            Some(n) => n,
            None => 0,
        }
    };
    Shape {
        page_bytes,
        dedicated_segments: dedicated,
        direct_route: size <= SMALL_SIZE_MAX,
    }
}

/// The largest allocation that can SHARE a segment with other allocations.
///
/// At or below this, a request is carved as a span of slices inside a segment
/// and several of them pack together. **Above it the cost jumps**: the request
/// gets a dedicated run of segments of its own, because the segment header
/// owns slice 0 and so no allocation of `SEGMENT_SIZE` can ever share a
/// segment with the metadata describing it.
///
/// 61,440 bytes at the default small profile; `--cfg ra_segment_size` raises
/// it (`crate::types::SLICES_PER_SEGMENT`). **This is the number a firmware
/// with a large allocation unit must design against**, and the one the
/// README's floor model used to omit.
pub const LARGEST_SHARED_ALLOC: usize = crate::types::LARGE_OBJ_SIZE_MAX;

/// Whole segments one allocation of `size` bytes reserves FOR ITSELF; `0` when
/// it shares a segment ([`LARGEST_SHARED_ALLOC`]).
///
/// The cliff this reports is the one a `rusty_zstd` firmware fell off: at the
/// default geometry a 64 KiB request answers **2**, so a four-segment 256 KiB
/// region holds one of them and a second fails with 192 KiB free
/// (`docs/plans/finished/esp32-large-alloc-ceiling.md`).
///
/// ```ignore
/// use rusty_alloc::prim::fixed::dedicated_segments;
/// const _: () = assert!(dedicated_segments(64 * 1024) <= 1, "raise ra_segment_size");
/// ```
#[must_use]
pub const fn dedicated_segments(size: usize) -> usize {
    if size <= LARGEST_SHARED_ALLOC {
        return 0;
    }
    // Mirrors `segment::huge_alloc`: one slice of header, then the payload,
    // rounded up to whole segments because the reservation must start on a
    // segment stride. Page-rounding inside `huge_alloc` cannot change this
    // count, since a segment is a whole number of pages.
    //
    // Overflow is `usize::MAX`, not 0: 0 already means "shares", and a
    // firmware `assert!(dedicated_segments(size) <= 1)` must fail closed on
    // an unrepresentable size (OH-rusty_alloc-33).
    let slice = crate::types::SEGMENT_SLICE_SIZE;
    let seg = crate::types::SEGMENT_SIZE;
    match slice.checked_add(size) {
        Some(need) => need.div_ceil(seg),
        None => usize::MAX,
    }
}

/// The smallest region that can hold `count` simultaneously-live allocations
/// of `size` bytes each — the question a firmware actually has, answered
/// including the costs that are easy to forget.
///
/// Two of those bit the reporting firmware. **`count` large allocations do not
/// cost `count * size`**: once each is over [`LARGEST_SHARED_ALLOC`] it takes
/// [`dedicated_segments`] of its own, so three 64 KiB blocks cost six segments
/// at the default geometry, not three. And **the small allocations every
/// program makes need somewhere to live** — a `Vec`'s spine, a formatting
/// buffer — which is a whole extra segment when the large blocks took
/// dedicated ones, and one more slice when they are sharing.
///
/// ```ignore
/// use rusty_alloc::prim::fixed::{Region, region_for_allocs};
/// // three live 64 KiB tables, plus room for everything smaller
/// static HEAP: Region<{ region_for_allocs(64 * 1024, 3) }> = Region::new();
/// // 448 KiB at the default geometry; 256 KiB under --cfg ra_segment_size="256k"
/// ```
///
/// Returns 0 if the arithmetic would overflow.
#[must_use]
pub const fn region_for_allocs(size: usize, count: usize) -> usize {
    let seg = crate::types::SEGMENT_SIZE;
    let usable_slices = crate::types::SLICES_PER_SEGMENT - 1;
    let dedicated = dedicated_segments(size);
    let segments = if dedicated == 0 {
        // Shares: every block is a span of slices, and the small allocations
        // take one more slice from the same segments rather than a segment of
        // their own. `+ 1` slice, not `+ 1` segment — getting that wrong is
        // what made a 3-block region read 512 KiB when 256 KiB serves it.
        let slices_each = if size == 0 {
            1
        } else {
            size.div_ceil(crate::types::SEGMENT_SLICE_SIZE)
        };
        let total = match slices_each.checked_mul(count) {
            Some(n) => match n.checked_add(1) {
                Some(n) => n,
                None => return 0,
            },
            None => return 0,
        };
        total.div_ceil(usable_slices)
    } else {
        // Dedicated: each block owns its segments, so the small allocations
        // have no carved segment to share and need one of their own.
        match dedicated.checked_mul(count) {
            Some(n) => match n.checked_add(1) {
                Some(n) => n,
                None => return 0,
            },
            None => return 0,
        }
    };
    let segments = if segments == 0 { 1 } else { segments };
    match segments.checked_mul(seg) {
        Some(bytes) => bytes,
        None => 0,
    }
}

/// Hand the backend the region it will serve from, once.
///
/// Takes `&'static mut [u8]` because that is exactly the claim being made: the
/// range lives forever and nobody else may touch it. On a chip this is the
/// linker-reserved heap symbol; in a test it is a `static mut` array or a
/// leaked box.
///
/// Returns `Err` if a region is already registered, or if this one is too small
/// to hold anything after alignment.
///
/// # Errors
/// - [`FERR_TOO_SMALL`] — below one [`FIXED_PAGE`].
/// - [`FERR_GEOMETRY`] — cannot hold one `SEGMENT_SIZE` segment at this
///   geometry, so the allocator above could never serve an allocation. This is
///   the one that used to be accepted silently: `init_region` returned `Ok`,
///   the build was clean, and the first `Vec` on the board returned null with a
///   backtrace pointing at whatever happened to allocate first. Reported by the
///   first outside firmware to adopt 2.0.0 (`docs/plans/embedded-adoption.md`).
/// - [`FERR_MISALIGNED`] — the base costs a whole segment against what this
///   length would yield from an aligned base, so the caller's model of the
///   size is wrong for this address. Use [`Region`], or size from
///   [`usable_bytes`] on the real base.
/// - [`FERR_REGISTERED`] — a region is already registered.
pub fn init_region(region: &'static mut [u8]) -> Result<(), PrimError> {
    let len = region.len();
    if len < FIXED_PAGE {
        return Err(FERR_TOO_SMALL);
    }
    let base = region.as_mut_ptr().expose_provenance();

    // EXACT, not conservative. `MIN_REGION` assumes a REGION_ALIGN-aligned
    // base; the real base is in hand here, so ask the question that actually
    // matters -- does a segment fit past the first aligned address? A check
    // against `len` alone would accept a region whose base sits one byte past
    // the grid and still fail on the board.
    if usable_bytes(base, len) == 0 {
        return Err(FERR_GEOMETRY);
    }
    // The base can cost a whole segment, and this is the one place that holds
    // both numbers: `usable_bytes` on the real base is exact, and on base 0 it
    // is what this length promises from an aligned container. When they
    // differ, the caller believed a size that this address cannot deliver —
    // `good_region_size(220 * 1024)` at `0x3fc8a1e4` serves two segments, not
    // three, 12 bytes off the 16-byte grid (24,092 off the segment grid, when
    // that was the grid) — and serving the smaller heap silently is how the
    // Janus firmware
    // reached `handle_alloc_error` 484 bytes short. A round length that
    // strands as much at an aligned base as it loses here passes: the numbers
    // agree, and that caller made no claim of exactness.
    if usable_bytes(base, len) < usable_bytes(0, len) {
        return Err(FERR_MISALIGNED);
    }

    let _g = Guard::acquire(&LOCK);
    if REGION_LEN.load(Ordering::Relaxed) != 0 {
        return Err(FERR_REGISTERED);
    }
    install_region(base, len);
    Ok(())
}

/// The install half of [`init_region`], with no geometry check.
///
/// Split out for the unit tests, which exercise this backend as a plain extent
/// allocator -- first fit, coalescing, two-ended placement -- on a region far
/// smaller than a 32 MiB segment. That is a legitimate thing to test and NOT a
/// legitimate thing to ship: an allocator handed a region that cannot hold one
/// segment is dead on arrival, which is exactly what [`init_region`] now
/// refuses. Private, so the refusal has no public bypass.
fn install_region(base: usize, len: usize) {
    // The origin every segment strides from is the first REGION_ALIGN-aligned
    // address; the bytes before it (at most 15) are not the region's. The
    // callers have established that a segment, or a page, fits past it. Under
    // the mask there is no origin: the base is kept as handed over, and the
    // bytes below the first boundary stay servable to page-sized requests.
    let origin = if cfg!(ra_aligned_region) {
        base
    } else {
        align_up(base, REGION_ALIGN)
    };
    let len = (base + len).saturating_sub(origin);
    REGION_BASE.store(origin, Ordering::Relaxed);
    REGION_LEN.store(len, Ordering::Relaxed);
    EXT_BASE[0].store(origin, Ordering::Relaxed);
    EXT_LEN[0].store(len, Ordering::Relaxed);
    EXT_COUNT.store(1, Ordering::Relaxed);
}

/// Storage for the FIRST heap's descriptor, so the region needs no page.
///
/// `create_heap` used to take one `FIXED_PAGE` from the region for every
/// `HeapBox`, which made a firmware's region `k * SEGMENT_SIZE + FIXED_PAGE`
/// — and an aligned container of that size is rounded up to the alignment,
/// so it paid `SEGMENT_SIZE - FIXED_PAGE` of padding for the privilege
/// (`docs/plans/finished/region-alignment-bug.md` §7). One heap lives for the
/// life of a firmware; its descriptor is this static, the region is whole
/// segments, and the padding has nothing to pad. A second heap, should a
/// firmware create one, takes a page from the region as before and costs the
/// segment that page breaks.
///
/// Bare metal only — a hosted target has an OS to allocate from, and pays
/// nothing for this.
#[cfg(all(not(miri), not(windows), not(unix), not(target_arch = "wasm32")))]
struct FirstHeapBox(core::cell::UnsafeCell<core::mem::MaybeUninit<crate::init::HeapBox>>);

// SAFETY: handed out exactly once, by `take_first_heap_box`'s swap, on the
// one thread a bare-metal build has; nothing else names the cell.
#[cfg(all(not(miri), not(windows), not(unix), not(target_arch = "wasm32")))]
unsafe impl Sync for FirstHeapBox {}

#[cfg(all(not(miri), not(windows), not(unix), not(target_arch = "wasm32")))]
static FIRST_HEAP_BOX: FirstHeapBox =
    FirstHeapBox(core::cell::UnsafeCell::new(core::mem::MaybeUninit::uninit()));
#[cfg(all(not(miri), not(windows), not(unix), not(target_arch = "wasm32")))]
static FIRST_HEAP_BOX_TAKEN: AtomicBool = AtomicBool::new(false);

/// The first heap's descriptor storage, once; `None` afterwards and on every
/// hosted target. Uninitialised: the caller writes every field, as it does
/// for a fresh page.
#[must_use]
pub fn take_first_heap_box() -> Option<*mut crate::init::HeapBox> {
    #[cfg(all(not(miri), not(windows), not(unix), not(target_arch = "wasm32")))]
    {
        if FIRST_HEAP_BOX_TAKEN.swap(true, Ordering::AcqRel) {
            None
        } else {
            Some(FIRST_HEAP_BOX.0.get().cast())
        }
    }
    #[cfg(not(all(not(miri), not(windows), not(unix), not(target_arch = "wasm32"))))]
    {
        None
    }
}

/// Whether `hb` is the descriptor [`take_first_heap_box`] handed out, i.e.
/// storage that must never be returned to the region.
#[must_use]
pub fn is_first_heap_box(hb: *const crate::init::HeapBox) -> bool {
    #[cfg(all(not(miri), not(windows), not(unix), not(target_arch = "wasm32")))]
    {
        core::ptr::eq(hb, FIRST_HEAP_BOX.0.get().cast_const().cast())
    }
    #[cfg(not(all(not(miri), not(windows), not(unix), not(target_arch = "wasm32"))))]
    {
        let _ = hb;
        false
    }
}

/// A firmware's heap region: `N` bytes of static storage, `SEGMENT_SIZE`-
/// aligned by construction, a whole number of segments so it carries no
/// padding, handed to the allocator once.
///
/// ```ignore
/// use rusty_alloc::prim::fixed::{Region, good_region_size};
///
/// static HEAP: Region<{ good_region_size(220 * 1024) }> = Region::new();
///
/// fn main() {
///     let usable = HEAP.give().expect("the region is accepted, and given once");
///     // usable == 196_608: three 64 KiB segments, nothing stranded.
/// }
/// ```
///
/// This exists because the alternatives both lose memory. The caller's own
/// `static [u8; N]` has alignment 1, and every sizing rule in this module
/// assumes a [`REGION_ALIGN`]-aligned base, so a region sized by
/// [`good_region_size`] but placed by the linker at an arbitrary address
/// yields one segment fewer than its name says fifteen times in sixteen —
/// the Janus firmware found that out at `handle_alloc_error`. And the
/// caller's own
/// `#[repr(align(65536))]` container around an exact size is rounded up to
/// the alignment: 200,704 bytes became 262,144 in `.bss` and cost that
/// firmware 60,952 bytes of stack. This type is aligned AND a whole number
/// of segments, so `size_of::<Region<N>>() == N` and the region is the
/// segments it serves, exactly (`docs/plans/finished/region-alignment-bug.md`).
///
/// `const fn new()`, so it is a `static` and the bytes sit in the image's
/// `.bss` rather than on anybody's stack; `N` is checked at compile time to
/// be at least one segment and a multiple of `SEGMENT_SIZE`, so
/// `Region<{ good_region_size(x) }>` for a budget below the floor is a build
/// error rather than a board run.
///
/// **Its alignment is 16 bytes, not a segment, and that is the whole RAM
/// story.** Segments stride from the region's base rather than from address
/// zero (`crate::REGION_STRIDES`,
/// `docs/plans/finished/region-alignment-dissolve.md`), so the linker owes
/// this static no gap. Until 2.0.4 the type was `SEGMENT_SIZE`-aligned, and
/// on the ESP32-S3 firmware that measured it the linker left 24,148 bytes in
/// front of it — charged to no section, so `size -A` could not see it and a
/// `.bss` delta overstated the saving by exactly that. Now
/// `.data + .bss + .stack` reconciles, and the section a region lives in is
/// the section it costs. Judging a region change by that sum, or by `.stack`,
/// is still the honest method on a fixed RAM map; it just has nothing left
/// to find. Under `--cfg ra_aligned_region` — the mask, for a firmware that
/// would rather have three instructions per free than the RAM — the type is
/// a segment-aligned again, and the gap is back with it.
#[repr(C)]
#[cfg_attr(not(ra_aligned_region), repr(align(16)))]
#[cfg_attr(all(ra_aligned_region, ra_small_profile), repr(align(65536)))]
#[cfg_attr(all(ra_aligned_region, not(ra_small_profile)), repr(align(33554432)))]
pub struct Region<const N: usize> {
    bytes: core::cell::UnsafeCell<[u8; N]>,
}

/// Whether any [`Region`] has been given. ONE flag for every instance rather
/// than a field in each: a field — even one byte — beside the array pads the
/// type to its alignment, which is the padding this type exists to avoid
/// (measured when the type was segment-aligned: `Region<196_608>` with a
/// flag inside was 262,144 bytes; at 16 it would still be 16 bytes of
/// nothing). One region can ever be registered per program, so one
/// flag is exact, and it is swapped BEFORE the `&mut` is formed so a second
/// `give` on the same instance never aliases the first.
static REGION_GIVEN: AtomicBool = AtomicBool::new(false);

// The literal in `repr(align)` cannot name a constant, so pin it to the
// alignment a region's base needs; and a whole-segment size at that
// alignment must not be padded, or the type has failed its purpose.
const _: () = assert!(
    core::mem::align_of::<Region<MIN_REGION>>() == REGION_ALIGN,
    "Region's alignment must equal REGION_ALIGN"
);
const _: () = assert!(
    core::mem::size_of::<Region<MIN_REGION>>() == MIN_REGION,
    "Region must carry no padding"
);

// SAFETY: the bytes are handed out exactly once. `give` swaps the
// module-wide `REGION_GIVEN` first and every later call, on any instance, is
// refused without touching `bytes`; nothing else in this module reads or
// writes `bytes`, so the one `&'static mut` that `give` produces is never
// aliased. The same handoff the Janus seam has carried since 2.0.0, moved
// here so that the alignment travels with it.
unsafe impl<const N: usize> Sync for Region<N> {}

impl<const N: usize> Region<N> {
    /// Bytes this region serves as segments: all of them, because the base
    /// is aligned and `N` is whole segments. Equal to `N`; spelled out so a
    /// firmware's `const` assertions read as what they mean.
    pub const USABLE: usize = usable_bytes(0, N);

    /// Reserve `N` bytes. A build error when `N` is not a whole number of
    /// segments, or cannot hold one.
    #[must_use]
    pub const fn new() -> Self {
        const {
            assert!(
                N >= MIN_REGION,
                "Region<N>: N cannot hold one segment at this geometry - raise it, \
                 or set --cfg ra_small_profile (64 KiB segments)"
            );
            assert!(
                N.is_multiple_of(crate::types::SEGMENT_SIZE),
                "Region<N>: N must be a whole number of segments - size it with \
                 good_region_size(budget) or region_for(usable)"
            );
        }
        Region {
            bytes: core::cell::UnsafeCell::new([0; N]),
        }
    }

    /// Hand the region to the allocator. Call once, before the first
    /// allocation. Returns the bytes the allocator can serve from it, which
    /// for this type is [`Region::USABLE`].
    ///
    /// # Errors
    /// [`FERR_REGISTERED`] on a second call, on any `Region`, or when a
    /// region was registered through [`init_region`] directly;
    /// [`init_region`]'s other codes as documented there
    /// ([`FERR_MISALIGNED`] cannot occur — that is what this type is for).
    pub fn give(&'static self) -> Result<usize, PrimError> {
        if REGION_GIVEN.swap(true, Ordering::AcqRel) {
            return Err(FERR_REGISTERED);
        }
        // SAFETY: the swap above succeeded, so this is the first `give` on
        // any `Region` in the program; `self` is `'static`, so the bytes live
        // for the program; and no other code touches `bytes`, so this is the
        // only reference to them there will ever be.
        let bytes: &'static mut [u8] = unsafe { &mut *self.bytes.get() };
        let base = bytes.as_ptr().expose_provenance();
        init_region(bytes)?;
        Ok(usable_bytes(base, N))
    }

    /// Bytes the allocator can serve from this region at its real base.
    /// Equal to [`Region::USABLE`], because the base is aligned; exposed so a
    /// firmware can log what it measured rather than what it assumed.
    #[must_use]
    pub fn usable(&self) -> usize {
        usable_bytes(self.bytes.get().expose_provenance(), N)
    }

    /// The region's size in bytes.
    #[must_use]
    pub const fn len(&self) -> usize {
        N
    }

    /// Whether the region is empty; never, since `N >= MIN_REGION`.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        N == 0
    }
}

impl<const N: usize> Default for Region<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether `addr` lies inside the registered region.
///
/// On a one-region target this IS the segment map: the allocator's memory is
/// exactly one range, whose bounds this backend already holds, so
/// `segment_map::contains` answers with two compares here instead of a
/// 64-entry range table (`firmware-what-is-left.md` §3). Same best-effort
/// meaning as the map's: memory we own, not memory currently allocated.
/// `false` before a region is registered.
#[must_use]
pub fn region_contains(addr: usize) -> bool {
    let base = REGION_BASE.load(Ordering::Relaxed);
    let len = REGION_LEN.load(Ordering::Relaxed);
    len != 0 && addr >= base && addr - base < len
}

/// What alignment is measured from: the region's base where segments stride
/// from it, address zero under `ra_aligned_region` (where they stride from
/// zero, as on a hosted target). The one definition both [`alloc`] and
/// [`region_capacity`] read, so a placement and a report cannot disagree.
#[inline]
fn stride_origin() -> usize {
    if cfg!(ra_aligned_region) {
        0
    } else {
        REGION_BASE.load(Ordering::Relaxed)
    }
}

/// The origin segments stride from: the registered region's base, or 0 while
/// none is registered — when no pointer can be ours and the answer is the
/// hosted mask's. Crate-internal: `segment_of` and the free-list plausibility
/// check are its callers (`crate::REGION_STRIDES`).
#[inline]
pub(crate) fn stride_base() -> usize {
    REGION_BASE.load(Ordering::Relaxed)
}

/// The region's occupancy: `(used, free, total)` bytes.
///
/// A fixed-region allocator that cannot report how much of its region is out
/// is unmeasurable on exactly the deployment it exists for — `esp_alloc::HEAP`
/// answers `used()`/`free()` and P4 of `docs/plans/small-metal.md` compares
/// against it. `used` is derived (`total - free`) rather than counted, so it
/// cannot drift from the free list.
///
/// A snapshot: another thread could change it, though on the single-threaded
/// targets this backend serves there is no other thread.
#[must_use]
pub fn region_stats() -> (usize, usize, usize) {
    let _g = Guard::acquire(&LOCK);
    let total = REGION_LEN.load(Ordering::Relaxed);
    let free: usize = (0..EXT_COUNT.load(Ordering::Relaxed))
        .map(|i| EXT_LEN[i].load(Ordering::Relaxed))
        .sum();
    (total - free, free, total)
}

/// What the region can still SERVE, which is not what is merely free:
/// `(whole segments still placeable, largest single allocation in bytes)`.
///
/// [`region_stats`] answers "how many bytes are unclaimed" and that number
/// **hides the constraint that actually fails an allocation**. The reporting
/// firmware saw 192 KiB free and a failing 64 KiB request; this function would
/// have answered `(1, 61_440)` and named the reason — one segment left, and
/// nothing bigger than a shared span can be placed in it.
///
/// The second value is the largest DEDICATED allocation placeable in a fresh
/// run of segments. A request at or below [`LARGEST_SHARED_ALLOC`] may still
/// succeed above this figure by sharing a segment that is already carved,
/// which this backend cannot see — so treat it as the floor of what will
/// work, not the ceiling.
///
/// A snapshot, as [`region_stats`] is.
#[must_use]
pub fn region_capacity() -> (usize, usize) {
    let _g = Guard::acquire(&LOCK);
    let seg = crate::types::SEGMENT_SIZE;
    let origin = stride_origin();
    let mut segments = 0usize;
    let mut largest = 0usize;
    for i in 0..EXT_COUNT.load(Ordering::Relaxed) {
        let base = EXT_BASE[i].load(Ordering::Relaxed);
        let len = EXT_LEN[i].load(Ordering::Relaxed);
        let end = base + len;
        // The first segment stride at or above this extent's base.
        let first = origin + (base - origin).next_multiple_of(seg);
        if first >= end {
            continue;
        }
        let run = end - first;
        segments += run / seg;
        if run > crate::types::SEGMENT_SLICE_SIZE {
            let placeable = run - crate::types::SEGMENT_SLICE_SIZE;
            if placeable > largest {
                largest = placeable;
            }
        }
    }
    (segments, largest)
}

/// Remove the extent at `idx`, shifting the tail down to keep the list sorted.
fn remove_at(idx: usize) {
    let n = EXT_COUNT.load(Ordering::Relaxed);
    for i in idx..n - 1 {
        EXT_BASE[i].store(EXT_BASE[i + 1].load(Ordering::Relaxed), Ordering::Relaxed);
        EXT_LEN[i].store(EXT_LEN[i + 1].load(Ordering::Relaxed), Ordering::Relaxed);
    }
    EXT_COUNT.store(n - 1, Ordering::Relaxed);
}

/// Insert `(base, len)` at `idx`, shifting the tail up. Caller has checked
/// there is room.
fn insert_at(idx: usize, base: usize, len: usize) {
    let n = EXT_COUNT.load(Ordering::Relaxed);
    let mut i = n;
    while i > idx {
        EXT_BASE[i].store(EXT_BASE[i - 1].load(Ordering::Relaxed), Ordering::Relaxed);
        EXT_LEN[i].store(EXT_LEN[i - 1].load(Ordering::Relaxed), Ordering::Relaxed);
        i -= 1;
    }
    EXT_BASE[idx].store(base, Ordering::Relaxed);
    EXT_LEN[idx].store(len, Ordering::Relaxed);
    EXT_COUNT.store(n + 1, Ordering::Relaxed);
}

/// A slice must be at least a page.
///
/// `bins::good_size` answers the large range with `os::page_align_up`, but the
/// large path allocates EXACT SLICES. `usable_size >= good_size` — an
/// ABI-visible promise, and a proptest — therefore holds only while a slice is
/// no smaller than a page. Every other backend gets this for free (a 64 KiB
/// slice over a 4 KiB page); this is the only one where the two can be tuned
/// into conflict, so this is where it is written down. Found by dropping the
/// small profile to a 2 KiB slice: `good_size(49_153)` promised 53,248 while
/// the 25-slice span delivered 51,200.
const _: () = assert!(
    crate::types::SEGMENT_SLICE_SIZE >= FIXED_PAGE,
    "SEGMENT_SLICE_SIZE must be >= FIXED_PAGE or good_size over-promises"
);

pub(super) fn mem_init() -> MemConfig {
    MemConfig {
        page_size: FIXED_PAGE,
        alloc_granularity: FIXED_PAGE,
        large_page_size: 0,
        has_overcommit: false,
        // A sub-range can be returned independently: `free` takes any extent.
        has_partial_free: true,
    }
}

/// Where inside `[base, base + len)` a `size`-byte `align`-aligned block goes:
/// the HIGHEST such address when `from_top`, the lowest otherwise. `None` when
/// it does not fit. `align` is a power of two (it comes from a `Layout` or from
/// [`FIXED_PAGE`]), so the top-down case is a mask.
fn place(
    base: usize,
    len: usize,
    size: usize,
    align: usize,
    from_top: bool,
    origin: usize,
) -> Option<usize> {
    if size > len {
        return None;
    }
    debug_assert!(base >= origin, "an extent lies inside the region");
    // Alignment is measured from `origin` — the region's base — not from
    // address zero: segments stride from it and `segment_of` resolves against
    // it (`crate::REGION_STRIDES`). `origin` is REGION_ALIGN-aligned, so a
    // request aligned no coarser than that is aligned in absolute terms too.
    let at = if from_top {
        origin + ((base + len - size - origin) & !(align - 1))
    } else {
        origin + align_up(base - origin, align)
    };
    // Top-down can mask below `base`; bottom-up can align past the end. Compare
    // on the sum, not a subtraction that would wrap.
    if at < base || at.saturating_add(size) > base + len {
        return None;
    }
    Some(at)
}

/// Two-ended first-fit over the free list, honouring `try_alignment`.
///
/// **Coarsely-aligned requests take the bottom; merely page-aligned ones take
/// the top.** That split is the whole point on a chip-sized region. A
/// `SEGMENT_SIZE` reservation can only start on a `SEGMENT_SIZE` stride from
/// the region's base, so every byte handed out below one pushes it to the next — a single 4 KiB heap
/// block placed at the bottom of the region costs an entire segment of reach.
/// Measured on a XIAO ESP32-S3 (docs/plans/small-metal.md §2.9): bottom-only
/// placement needed a 192 KiB region for a workload whose segments and metadata
/// total 132 KiB, with 61,440 bytes sitting on the free list that no segment
/// request could ever use. Requests that do NOT care about coarse alignment are
/// the ones that can move, so they are the ones that move.
///
/// Alignment slack around the chosen placement is not lost: head and tail stay
/// on the list as their own extents, which is what makes repeated aligned
/// requests on a small region survivable at all.
///
/// # Errors
/// [`FERR`] when no region is registered, when no extent can hold
/// `size` at `try_alignment` — which is what a `SEGMENT_SIZE` request on a
/// chip-sized region does — or when splitting would need more than
/// [`MAX_EXTENTS`] entries.
pub(super) unsafe fn alloc(
    size: usize,
    try_alignment: usize,
    _commit: bool,
    _allow_large: bool,
) -> Result<Alloc, PrimError> {
    if size == 0 {
        return Err(FERR);
    }
    let align = try_alignment.max(FIXED_PAGE);
    let size = align_up(size, FIXED_PAGE);

    let _g = Guard::acquire(&LOCK);
    if REGION_LEN.load(Ordering::Relaxed) == 0 {
        return Err(FERR);
    }
    let origin = stride_origin();

    // Page-aligned requests search from the HIGHEST extent down and settle at
    // its top; coarsely-aligned ones search from the lowest up, as before.
    let from_top = align == FIXED_PAGE;
    let n = EXT_COUNT.load(Ordering::Relaxed);
    for k in 0..n {
        let i = if from_top { n - 1 - k } else { k };
        let base = EXT_BASE[i].load(Ordering::Relaxed);
        let len = EXT_LEN[i].load(Ordering::Relaxed);
        let Some(aligned) = place(base, len, size, align, from_top, origin) else {
            continue;
        };
        let head = aligned - base;
        let tail = (base + len) - (aligned + size);

        // Splitting an extent into head + tail costs one extra entry; growing
        // the list by one must stay inside the bound, or nothing moves.
        if head > 0 && tail > 0 && n + 1 > MAX_EXTENTS {
            return Err(FERR);
        }

        remove_at(i);
        let mut at = i;
        if head > 0 {
            insert_at(at, base, head);
            at += 1;
        }
        if tail > 0 {
            insert_at(at, aligned + size, tail);
        }
        return Ok(Alloc {
            ptr: core::ptr::with_exposed_provenance_mut(aligned),
            is_large: false,
            // Conservative: a recycled extent holds whatever its last tenant
            // left. See the module doc.
            is_zero: false,
        });
    }
    Err(FERR)
}

/// Return an extent to the free list, coalescing with either neighbour.
///
/// # Errors
/// [`FERR`] if the range is not inside the registered region, or if the list is
/// full and the range touches neither neighbour. The latter is the
/// [`MAX_EXTENTS`] bound biting; it refuses rather than dropping the range.
pub(super) unsafe fn free(ptr: *mut u8, size: usize) -> Result<(), PrimError> {
    if size == 0 {
        return Ok(());
    }
    let base = ptr.expose_provenance();
    let size = align_up(size, FIXED_PAGE);

    let _g = Guard::acquire(&LOCK);
    let rbase = REGION_BASE.load(Ordering::Relaxed);
    let rlen = REGION_LEN.load(Ordering::Relaxed);
    if rlen == 0 || base < rbase || base + size > rbase + rlen {
        return Err(FERR);
    }

    let n = EXT_COUNT.load(Ordering::Relaxed);
    // Sorted insertion point: the first extent starting above `base`.
    let idx = EXT_BASE[..n]
        .iter()
        .position(|e| e.load(Ordering::Relaxed) > base)
        .unwrap_or(n);

    let prev_touches = idx > 0 && {
        let pb = EXT_BASE[idx - 1].load(Ordering::Relaxed);
        pb + EXT_LEN[idx - 1].load(Ordering::Relaxed) == base
    };
    let next_touches = idx < n && EXT_BASE[idx].load(Ordering::Relaxed) == base + size;

    match (prev_touches, next_touches) {
        // Bridges two extents: absorb both into the earlier one.
        (true, true) => {
            let grown = EXT_LEN[idx - 1].load(Ordering::Relaxed)
                + size
                + EXT_LEN[idx].load(Ordering::Relaxed);
            EXT_LEN[idx - 1].store(grown, Ordering::Relaxed);
            remove_at(idx);
        }
        (true, false) => {
            let grown = EXT_LEN[idx - 1].load(Ordering::Relaxed) + size;
            EXT_LEN[idx - 1].store(grown, Ordering::Relaxed);
        }
        (false, true) => {
            EXT_BASE[idx].store(base, Ordering::Relaxed);
            let grown = EXT_LEN[idx].load(Ordering::Relaxed) + size;
            EXT_LEN[idx].store(grown, Ordering::Relaxed);
        }
        (false, false) => {
            if n >= MAX_EXTENTS {
                return Err(FERR);
            }
            insert_at(idx, base, size);
        }
    }
    Ok(())
}

/// Always backed; reports NOT-known-zero, because [`decommit`] preserves
/// contents here.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the prim backends share one signature; a no-op backend still returns the contract's Result"
)]
pub(super) unsafe fn commit(_ptr: *mut u8, _size: usize) -> Result<bool, PrimError> {
    Ok(false)
}

/// No-op. `false` = no re-commit needed, contents preserved.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the prim backends share one signature; a no-op backend still returns the contract's Result"
)]
pub(super) unsafe fn decommit(_ptr: *mut u8, _size: usize) -> Result<bool, PrimError> {
    Ok(false)
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "the prim backends share one signature; a no-op backend still returns the contract's Result"
)]
pub(super) unsafe fn reset(_ptr: *mut u8, _size: usize) -> Result<(), PrimError> {
    Ok(())
}

/// No MMU. Fail loudly rather than pretend — same reasoning as the wasm arm.
pub(super) unsafe fn protect(_ptr: *mut u8, _size: usize, _on: bool) -> Result<(), PrimError> {
    Err(FERR)
}

pub(super) fn range_is_reserved(ptr: *const u8, size: usize) -> bool {
    let start = ptr as usize;
    let Some(last) = start.checked_add(size.saturating_sub(1)) else {
        return false;
    };
    region_contains(start) && region_contains(last)
}

pub(super) fn numa_node_count() -> usize {
    1
}

/// One thread, so one id. Must be non-zero: zero is the allocator's "segment is
/// abandoned" sentinel.
#[inline]
pub(super) fn thread_id() -> usize {
    1
}

/// No clock. A monotonic counter preserves purge ORDERING, which is all the
/// purge policy reads; duration does not survive.
///
/// **Two 32-bit words, not one `AtomicU64`.** The seam's return type is `u64`,
/// but this backend's whole reason to exist is a target without 64-bit
/// atomics — an `AtomicU64` here would be the one §2.2 site the port itself
/// introduced. Widening two `AtomicU32`s under their own lock keeps the full
/// range without one, and a 32-bit counter alone would wrap and invert purge
/// ordering, which is exactly the property this function exists to provide.
///
/// The lock is separate from [`LOCK`] on purpose: [`Guard`] is not reentrant,
/// and a shared lock would deadlock the moment an allocation path wanted a
/// timestamp.
static CLOCK_LOCK: AtomicBool = AtomicBool::new(false);
static TICK_LO: AtomicU32 = AtomicU32::new(0);
static TICK_HI: AtomicU32 = AtomicU32::new(0);

pub(super) fn clock_now() -> u64 {
    let _g = Guard::acquire(&CLOCK_LOCK);
    let (lo, carry) = TICK_LO.load(Ordering::Relaxed).overflowing_add(1);
    TICK_LO.store(lo, Ordering::Relaxed);
    let hi = if carry {
        let h = TICK_HI.load(Ordering::Relaxed).wrapping_add(1);
        TICK_HI.store(h, Ordering::Relaxed);
        h
    } else {
        TICK_HI.load(Ordering::Relaxed)
    };
    (u64::from(hi) << 32) | u64::from(lo)
}

/// TLS for a single-threaded world: a fixed static table. Destructors are
/// accepted and never run — there is no thread exit.
const MAX_TLS: usize = 8;
static TLS_VALUES: [AtomicUsize; MAX_TLS] = [const { AtomicUsize::new(0) }; MAX_TLS];
static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);

pub(super) struct TlsSlotImpl(usize);

pub(super) fn tls_new(_dtor: Option<TlsDtor>) -> Option<TlsSlotImpl> {
    let idx = NEXT_SLOT.fetch_add(1, Ordering::Relaxed);
    if idx < MAX_TLS {
        Some(TlsSlotImpl(idx))
    } else {
        None
    }
}

pub(super) fn tls_get(slot: &TlsSlotImpl) -> *mut c_void {
    core::ptr::with_exposed_provenance_mut(TLS_VALUES[slot.0].load(Ordering::Relaxed))
}

pub(super) fn tls_set(slot: &TlsSlotImpl, value: *mut c_void) {
    TLS_VALUES[slot.0].store(value.expose_provenance(), Ordering::Relaxed);
}

pub(super) fn tls_raw(slot: &TlsSlotImpl) -> usize {
    slot.0
}

pub(super) fn tls_from_raw(raw: usize) -> TlsSlotImpl {
    TlsSlotImpl(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SEGMENT_SIZE;

    /// Unlike the wasm backend, `free` here really frees — so the arena's
    /// adopt-on-free path folds away, exactly as on every platform whose free
    /// works. A `const` assertion because it is a compile-time fact.
    const _: () = assert!(super::super::FREE_RETURNS_MEMORY);

    /// The whole free list, as `(base_offset, len)` against the region base.
    fn extents() -> Vec<(usize, usize)> {
        let _g = Guard::acquire(&LOCK);
        let rbase = REGION_BASE.load(Ordering::Relaxed);
        (0..EXT_COUNT.load(Ordering::Relaxed))
            .map(|i| {
                (
                    EXT_BASE[i].load(Ordering::Relaxed) - rbase,
                    EXT_LEN[i].load(Ordering::Relaxed),
                )
            })
            .collect()
    }

    fn free_total() -> usize {
        extents().iter().map(|e| e.1).sum()
    }

    /// P1's kill test: a 512 KiB static region, served and recycled.
    ///
    /// One `#[test]` rather than several, because the free list is process-wide
    /// state that `init_region` deliberately refuses to re-initialise — so the
    /// ordering has to be explicit rather than left to the harness.
    /// P1's kill test asks for a 512 KiB region; this is it, plus ONE page.
    /// `static mut` rather than a leak so the test needs no allocator of its
    /// own — the one under test is the allocator.
    ///
    /// The odd page and the 64 KiB alignment are what make §2.9's half two
    /// able to fail. A region that is an exact multiple of `SEGMENT_SIZE`
    /// cannot tell the two placement policies apart — a page off either end
    /// costs a segment — so at 512 KiB flat the assertion would pass under the
    /// bug it exists to catch. `K * SEGMENT_SIZE + FIXED_PAGE` on an aligned
    /// base is the shape the board actually has, and the shape that
    /// discriminates.
    ///
    /// The base is carved at RUNTIME from an oversized backing array, and
    /// carved DELIBERATELY OFF every segment boundary: `RAGGED` bytes past a
    /// 64 KiB line, on the 16-byte grid. Segments stride from the base
    /// (`crate::REGION_STRIDES`), so a region at such an address must serve
    /// exactly what an aligned one does; until 2.0.4 it lost a segment to the
    /// run-up, which is what the Janus firmware hit. Carving the window also
    /// matches how a linker script hands a chip its heap.
    const GRID: usize = 64 * 1024;
    /// Zero under `--cfg ra_aligned_region`, where the base must sit ON the
    /// segment grid and the assertions below say so instead.
    const RAGGED: usize = if cfg!(ra_aligned_region) { 0 } else { 0x1f0 };
    const SLACK: usize = GRID + RAGGED;
    const N: usize = 512 * 1024 + FIXED_PAGE;
    static mut BACKING: [u8; N + SLACK] = [0; N + SLACK];
    static mut OTHER: [u8; FIXED_PAGE] = [0; FIXED_PAGE];

    #[test]
    fn serves_and_recycles_a_static_region() {
        // The window inside BACKING: `RAGGED` past a 64 KiB line, so at the
        // small profile the base is off the segment grid on purpose. `add`
        // keeps the array's provenance, so the slice below is a real borrow
        // of it.
        let bp = (&raw mut BACKING).cast::<u8>();
        let skip = align_up(bp.expose_provenance(), GRID) - bp.expose_provenance() + RAGGED;
        // SAFETY: `skip < GRID + RAGGED == SLACK`, so `skip + N` is inside BACKING.
        let rp = unsafe { bp.add(skip) };
        assert_eq!(
            rp.expose_provenance() % REGION_ALIGN,
            0,
            "on the grid the base needs"
        );
        assert_eq!(
            rp.expose_provenance() % GRID,
            RAGGED,
            "and where the test put it"
        );
        // SAFETY: the only reference ever taken to REGION, handed straight to
        // init_region which requires (and consumes) exactly that exclusivity.
        let region: &'static mut [u8] = unsafe { core::slice::from_raw_parts_mut(rp, N) };

        // The negative cases FIRST: each must be refused without consuming the
        // one registration this backend accepts.
        let tiny: &'static mut [u8] = &mut [];
        assert_eq!(
            init_region(tiny),
            Err(FERR_TOO_SMALL),
            "a region below one page is refused, and says which"
        );

        // Branch on the ACTIVE geometry, because both arms are real. At the
        // shipped 32 MiB segment this 512 KiB region cannot hold one, so
        // `init_region` refuses it -- correctly, since an allocator handed it
        // would be dead on arrival -- and the extent allocator beneath, which is
        // what the rest of this test exercises, is installed directly. At the
        // small profile a 64 KiB segment fits eight times over and the public
        // entry point is used as a firmware would.
        if usable_bytes(rp.expose_provenance(), N) == 0 {
            assert_eq!(
                init_region(region),
                Err(FERR_GEOMETRY),
                "a region that cannot hold one segment is refused BEFORE the board"
            );
            install_region(rp.expose_provenance(), N);
        } else {
            init_region(region).expect("this geometry's segment fits in N");
        }
        assert_eq!(free_total(), N, "the whole region starts free");
        assert_eq!(extents().len(), 1, "as one extent");

        // A second registration is refused, and says so distinctly.
        let op = &raw mut OTHER;
        // SAFETY: as above; the call is expected to fail before it stores it.
        let other: &'static mut [u8] =
            unsafe { core::slice::from_raw_parts_mut(op.cast::<u8>(), FIXED_PAGE) };
        let second = init_region(other);
        assert!(second.is_err(), "no second region");

        // Serve three page-aligned blocks.
        // SAFETY: the prim contract — sizes are page multiples, alignment a
        // power of two.
        let (a, b, c) = unsafe {
            (
                alloc(64 * 1024, FIXED_PAGE, true, false).expect("a"),
                alloc(128 * 1024, FIXED_PAGE, true, false).expect("b"),
                alloc(64 * 1024, FIXED_PAGE, true, false).expect("c"),
            )
        };
        assert_eq!(free_total(), N - 256 * 1024, "three blocks are out");
        assert!(!a.is_zero, "recycled memory is never claimed zero");

        // Blocks are distinct, in the region, and do not overlap.
        let base = REGION_BASE.load(Ordering::Relaxed);
        for (p, len) in [(a.ptr, 64 * 1024), (b.ptr, 128 * 1024), (c.ptr, 64 * 1024)] {
            let off = p.expose_provenance() - base;
            assert!(off + len <= N, "block lies inside the region");
        }
        assert_ne!(a.ptr, b.ptr);
        assert_ne!(b.ptr, c.ptr);

        // Write a pattern through each and read it back: the region is real
        // memory, not just bookkeeping.
        for (p, len, tag) in [(a.ptr, 64 * 1024, 0xA5u8), (b.ptr, 128 * 1024, 0x5Au8)] {
            // SAFETY: `p` is a live block of `len` bytes from `alloc` above.
            unsafe {
                core::ptr::write_bytes(p, tag, len);
                assert_eq!(*p, tag);
                assert_eq!(*p.add(len - 1), tag);
            }
        }

        // Free the middle block: it becomes its own extent, no coalescing.
        let holes = extents().len();
        // SAFETY: `b` came from `alloc` and is unfreed.
        unsafe { free(b.ptr, 128 * 1024).expect("free b") };
        assert_eq!(free_total(), N - 128 * 1024);
        assert_eq!(extents().len(), holes + 1, "an isolated hole");

        // Free its neighbours: everything coalesces back to one extent.
        // SAFETY: both came from `alloc` and are unfreed.
        unsafe {
            free(a.ptr, 64 * 1024).expect("free a");
            free(c.ptr, 64 * 1024).expect("free c");
        }
        assert_eq!(free_total(), N, "the whole region is back");
        assert_eq!(extents().len(), 1, "coalesced into one extent");

        // The region is reusable: the same 256 KiB can be served again.
        // SAFETY: prim contract, as above.
        let d = unsafe { alloc(256 * 1024, FIXED_PAGE, true, false).expect("d") };
        assert_eq!(free_total(), N - 256 * 1024);
        // SAFETY: `d` is live and unfreed.
        unsafe { free(d.ptr, 256 * 1024).expect("free d") };
        assert_eq!(free_total(), N);

        // ---- §2.1, both sides ----
        //
        // This lives HERE, and not in a test of its own, because the free list
        // is process-wide and the harness orders tests arbitrarily: standalone,
        // it passed while NO region was registered, i.e. for the trivial reason
        // rather than the interesting one. Measured, not assumed.
        //
        // P1 wrote this as a one-sided refusal, because at the shipped geometry
        // a segment cannot come out of a chip-sized region. P2 made the
        // geometry a parameter, so the property is now two-sided and says
        // something either way — which is the point of having kept it.
        assert_eq!(free_total(), N, "the whole region is free before this");
        // SAFETY: prim contract; SEGMENT_SIZE is a power of two.
        let seg = unsafe { alloc(SEGMENT_SIZE, SEGMENT_SIZE, true, false) };
        if SEGMENT_SIZE > N {
            // The shipped 32 MiB geometry: 512 KiB cannot hold a segment, and
            // the refusal must cost nothing.
            assert!(
                seg.is_err(),
                "a {SEGMENT_SIZE}-byte segment cannot come out of a {N}-byte region"
            );
            assert_eq!(
                free_total(),
                N,
                "a refused request leaves the list untouched"
            );

            let base = REGION_BASE.load(Ordering::Relaxed);
            if cfg!(ra_aligned_region) {
                // The ALIGNMENT half is decided by the region's ADDRESS, not
                // its size: a region smaller than SEGMENT_SIZE still holds one
                // SEGMENT_SIZE-aligned page whenever it straddles a boundary,
                // and where BACKING lands is the loader's choice (a 1-in-64
                // chance per run at this N; the one-sided form went red on CI
                // on 2026-09-08 when ASLR put the window across a 32 MiB
                // line). Decide from the address, and demand the answer that
                // follows from it either way.
                let boundary = align_up(base, SEGMENT_SIZE);
                let straddles = boundary + FIXED_PAGE <= base + N;
                // SAFETY: prim contract, as above.
                let al = unsafe { alloc(FIXED_PAGE, SEGMENT_SIZE, true, false) };
                if straddles {
                    let al = al.expect("the boundary is inside the region, so a page at it fits");
                    assert_eq!(
                        al.ptr.expose_provenance(),
                        boundary,
                        "served AT the one SEGMENT_SIZE-aligned address the region has"
                    );
                    // SAFETY: `al` is live and unfreed.
                    unsafe { free(al.ptr, FIXED_PAGE).expect("free the aligned page") };
                } else {
                    assert!(
                        al.is_err(),
                        "no SEGMENT_SIZE-aligned address lies inside this region"
                    );
                }
            } else {
                // Segments stride from the region's base, so the base IS the
                // first stride and a SEGMENT_SIZE-aligned page is served
                // there, whatever the address: the straddle case above is the
                // one this dissolves.
                // SAFETY: prim contract, as above.
                let al = unsafe { alloc(FIXED_PAGE, SEGMENT_SIZE, true, false) }
                    .expect("the region's base is its first segment stride");
                assert_eq!(
                    al.ptr.expose_provenance(),
                    base,
                    "served AT the base: stride 0, whatever the address"
                );
                // SAFETY: `al` is live and unfreed.
                unsafe { free(al.ptr, FIXED_PAGE).expect("free the aligned page") };
            }
            assert_eq!(free_total(), N, "and the list is whole again");
        } else {
            // The small profile: this is what P2 bought. A whole segment, at
            // segment alignment, served from a chip-sized region.
            let a = seg.expect("a segment must fit once the geometry allows it");
            let base = REGION_BASE.load(Ordering::Relaxed);
            if cfg!(ra_aligned_region) {
                assert_eq!(
                    a.ptr.expose_provenance() % SEGMENT_SIZE,
                    0,
                    "a segment must be SEGMENT_SIZE-aligned — `segment_of` masks on it"
                );
            } else {
                assert_eq!(
                    (a.ptr.expose_provenance() - base) % SEGMENT_SIZE,
                    0,
                    "a segment sits on a SEGMENT_SIZE stride from the base — `segment_of` masks the offset"
                );
                assert_eq!(
                    a.ptr.expose_provenance(),
                    base,
                    "the first stride is the base itself"
                );
                assert_ne!(
                    a.ptr.expose_provenance() % SEGMENT_SIZE,
                    0,
                    "and it is NOT segment-aligned in absolute terms: the region was carved ragged on purpose"
                );
            }
            // `saturating_sub`: the compiler const-evaluates this arm even when
            // the branch is dead, and at the shipped geometry SEGMENT_SIZE > N.
            assert_eq!(free_total(), N.saturating_sub(SEGMENT_SIZE));
            // SAFETY: `a` is live and unfreed.
            unsafe { free(a.ptr, SEGMENT_SIZE).expect("free the segment") };
        }
        // Either way the region ends whole: a refusal consumed nothing, and a
        // served segment was handed back.
        assert_eq!(free_total(), N);

        // ---- §2.9, the two-ended placement, both halves ----
        //
        // Also here rather than standalone, for the same process-wide-state
        // reason as §2.1 above.
        //
        // HALF ONE, the mechanism: a merely page-aligned request goes to the
        // TOP, leaving the low end of the region contiguous. Under the old
        // bottom-only first-fit this offset was 0 and the surviving extent
        // started at FIXED_PAGE — which is exactly how a 4 KiB heap block used
        // to cost a whole segment of reach.
        // SAFETY: prim contract — a page multiple at a power-of-two alignment.
        let top = unsafe { alloc(FIXED_PAGE, FIXED_PAGE, true, false).expect("top") };
        assert_eq!(
            top.ptr.expose_provenance() - REGION_BASE.load(Ordering::Relaxed),
            N - FIXED_PAGE,
            "a page-aligned request is placed at the top of the region"
        );
        assert_eq!(
            extents(),
            vec![(0, N - FIXED_PAGE)],
            "and leaves the low end as ONE contiguous extent"
        );
        // SAFETY: `top` is live and unfreed.
        unsafe { free(top.ptr, FIXED_PAGE).expect("free top") };
        assert_eq!(free_total(), N);

        // HALF TWO, the consequence that was actually measured: taking that
        // page must not cost a single SEGMENT_SIZE-aligned segment. Counted
        // both ways rather than asserted, so the test says what it means at
        // whichever geometry it is compiled for (at the shipped 32 MiB one
        // both counts are 0, and the equality still holds honestly).
        let clean = greedy_segments();
        assert_eq!(free_total(), N, "counting segments leaves the region whole");
        // SAFETY: prim contract, as above.
        let hdr = unsafe { alloc(FIXED_PAGE, FIXED_PAGE, true, false).expect("hdr") };
        let with_hdr = greedy_segments();
        assert_eq!(
            with_hdr, clean,
            "a page-sized block must not cost a whole segment of reach"
        );
        // SAFETY: `hdr` is live and unfreed.
        unsafe { free(hdr.ptr, FIXED_PAGE).expect("free hdr") };
        assert_eq!(free_total(), N, "and the region ends whole");

        // ---- the large-allocation ceiling, reported from rusty_zstd ----
        //
        // `docs/plans/finished/esp32-large-alloc-ceiling.md`: a 64 KiB request
        // in a 256 KiB region served ONCE, with 192 KiB free. Here against the
        // real extent allocator, and pinned to what `dedicated_segments`
        // predicts so the sizing API and the backend cannot drift.
        //
        // Also here rather than standalone, for the same process-wide-state
        // reason as §2.1 and §2.9.
        assert_eq!(free_total(), N, "the region is whole before this");
        let seg_count = N / SEGMENT_SIZE;
        if seg_count >= 2 {
            // A request of exactly SEGMENT_SIZE is the reported shape.
            let size = SEGMENT_SIZE;
            let cost = dedicated_segments(size);
            // The whole finding in one assertion: an allocation the size of a
            // segment can NEVER share one, because the header owns slice 0.
            if size > LARGEST_SHARED_ALLOC {
                assert!(
                    cost >= 2,
                    "a SEGMENT_SIZE request cannot fit one segment: the header owns slice 0"
                );
            }
            let served = greedy_dedicated(size);
            assert_eq!(free_total(), N, "counting leaves the region whole");
            assert!(served >= 1, "a region of {seg_count} segments serves none");

            // THE REPORTED SYMPTOM, as a property rather than a placement
            // count: the region is left with far more free bytes than the
            // payload it managed to serve. At a geometry where a
            // segment-sized request is DEDICATED, utilisation cannot reach
            // half, because every block drags a header into a second segment.
            if cost >= 2 {
                assert!(
                    served * size * 2 <= N + SEGMENT_SIZE,
                    "dedicated blocks cannot use half the region: served {served} x {size} of {N}"
                );
            }

            // And the half the report could not see: the FIRST small
            // allocation claims a WHOLE segment (a normal segment is a
            // SEGMENT_SIZE reservation), so it costs a large consumer reach.
            // This is why the firmware measured 1 where the arithmetic on
            // free bytes alone suggested more.
            // SAFETY: prim contract - a power-of-two alignment, page multiple.
            let seg_taken =
                unsafe { alloc(SEGMENT_SIZE, SEGMENT_SIZE, true, false).expect("a segment") };
            let after_small = greedy_dedicated(size);
            // SAFETY: `seg_taken` is live and unfreed.
            unsafe { free(seg_taken.ptr, SEGMENT_SIZE).expect("free the segment") };
            assert_eq!(free_total(), N, "and the region ends whole");
            assert!(
                after_small <= served,
                "taking a segment cannot increase the large-allocation reach"
            );

            // `region_for_allocs` must not promise a region that would fail:
            // whatever this region actually served, the API's answer for one
            // MORE block has to be bigger than this region.
            let promised = region_for_allocs(size, served + 1);
            assert!(
                promised > N,
                "region_for_allocs({size}, {}) = {promised} must exceed the {N} that served {served}",
                served + 1
            );
        }
    }

    /// Serve as many DEDICATED reservations of `size` as the region will take,
    /// then hand them all back. The request shape is `segment::huge_alloc`'s:
    /// one slice of header, the payload, page-rounded, at `SEGMENT_SIZE`
    /// alignment - which is exactly why it cannot share a segment.
    fn greedy_dedicated(size: usize) -> usize {
        let want = align_up(crate::types::SEGMENT_SLICE_SIZE + size, FIXED_PAGE);
        let mut held = Vec::new();
        // SAFETY: prim contract - SEGMENT_SIZE is a power of two, and every
        // pointer collected here is freed below before the function returns.
        while let Ok(a) = unsafe { alloc(want, SEGMENT_SIZE, true, false) } {
            held.push(a.ptr);
        }
        let n = held.len();
        for p in held {
            // SAFETY: each `p` came from the `alloc` above and is unfreed.
            unsafe { free(p, want).expect("free a counted reservation") };
        }
        n
    }

    /// Serve `SEGMENT_SIZE`-aligned segments until the region refuses, then
    /// hand them all back. Returns how many it managed — the region's segment
    /// *reach*, which is the quantity §2.9's placement rule protects.
    fn greedy_segments() -> usize {
        let mut held = Vec::new();
        // SAFETY: prim contract — SEGMENT_SIZE is a power of two, and every
        // pointer collected here is freed below before the function returns.
        while let Ok(a) = unsafe { alloc(SEGMENT_SIZE, SEGMENT_SIZE, true, false) } {
            held.push(a.ptr);
        }
        let n = held.len();
        for p in held {
            // SAFETY: each `p` came from the `alloc` above and is unfreed.
            unsafe { free(p, SEGMENT_SIZE).expect("free a counted segment") };
        }
        n
    }

    /// `usable_bytes` is pure arithmetic, so it gets its own test with no
    /// global state -- and the case that motivated it, from the first outside
    /// adopter's report.
    /// §1 of `firmware-what-is-left.md`: a budget rounds DOWN to the largest
    /// zero-waste region, a need rounds UP to the smallest sufficient one,
    /// and both agree with `usable_bytes` to the byte. Whole segments since
    /// 2.0.4: the heap descriptor is a static, so no page is added.
    #[test]
    fn good_region_size_strands_nothing() {
        use crate::types::SEGMENT_SIZE as SEG;
        // The reported case: 220 KiB strands 28,672; the good size strands 0.
        // Only meaningful at the small profile -- at the shipped 32 MiB
        // segment a 220 KiB budget is below the floor and the answer is 0,
        // which is the other thing this function must say.
        let budget = 220 * 1024;
        let good = good_region_size(budget);
        assert!(good <= budget, "a budget is a ceiling");
        if budget >= MIN_REGION {
            // The PROPERTY, true at every geometry: whole segments, and the
            // remainder is exactly what a round budget strands.
            assert_eq!(good, (budget / SEG) * SEG, "whole segments");
            assert_eq!(usable_bytes(0, good), good, "every byte is a segment");
            assert_eq!(
                usable_bytes(0, budget),
                usable_bytes(0, good),
                "the good size serves as much as the budget did"
            );
            assert_eq!(
                budget - good,
                budget % SEG,
                "and that is what the budget was stranding"
            );
            // The REPORTED numbers, pinned at the geometry they were measured
            // on (4 KiB x 16). `ra_segment_size` moves them, and a test that
            // asserted them everywhere would fail for the wrong reason.
            if SEG == 64 * 1024 {
                assert_eq!(good, 3 * SEG, "three segments at the default");
                assert_eq!(good, 196_608);
                assert_eq!(budget - good, 28_672);
            }
        } else {
            assert_eq!(
                good, 0,
                "no zero-waste region fits a budget below the floor"
            );
        }

        // Below the floor there is no zero-waste region at all.
        assert_eq!(good_region_size(0), 0);
        assert_eq!(good_region_size(MIN_REGION - 1), 0);
        assert_eq!(good_region_size(MIN_REGION), MIN_REGION);

        // Every budget: the answer fits, strands nothing, and is the LARGEST
        // such size — one more segment would not fit.
        let mut b = MIN_REGION;
        while b < 40 * SEG {
            let g = good_region_size(b);
            assert!(g <= b && g >= MIN_REGION);
            assert_eq!(g % SEG, 0, "k * SEGMENT_SIZE");
            assert_eq!(usable_bytes(0, g), g);
            assert!(g + SEG > b, "not the largest: {g} for budget {b}");
            b += 4093; // a coprime stride so every residue gets visited
        }

        // The other direction: the smallest region that serves what is asked.
        // "I need 192 KiB": three segments at the small profile, one at the
        // shipped 32 MiB geometry -- the test asks the arithmetic, not a number.
        let need: usize = 192 * 1024;
        let k = need.div_ceil(SEG);
        assert_eq!(region_for(need), k * SEG);
        assert_eq!(usable_bytes(0, region_for(need)), k * SEG);
        assert_eq!(region_for(1), SEG, "one byte still costs a segment");
        assert_eq!(
            region_for(0),
            MIN_REGION,
            "and so does zero — a region must serve something"
        );
        assert_eq!(region_for(SEG + 1), 2 * SEG, "a byte over rounds up");
        let mut u = 1;
        while u < 40 * SEG {
            let r = region_for(u);
            assert!(
                usable_bytes(0, r) >= u,
                "region_for({u}) = {r} serves too little"
            );
            assert!(
                usable_bytes(0, r - SEG) < u || r - SEG < MIN_REGION,
                "region_for({u}) = {r} is not the smallest"
            );
            assert_eq!(
                good_region_size(r),
                r,
                "a region_for answer is already a good size"
            );
            u += 4093;
        }
    }

    /// The sizing API a firmware plans with, and the cliff it exists to make
    /// visible (`docs/plans/finished/esp32-large-alloc-ceiling.md`).
    #[test]
    fn dedicated_segments_names_the_large_allocation_cliff() {
        use crate::types::{SEGMENT_SIZE as SEG, SEGMENT_SLICE_SIZE as SLICE};

        // Below the cliff nothing is dedicated: the request is a span that
        // packs with its neighbours.
        assert_eq!(dedicated_segments(0), 0);
        assert_eq!(dedicated_segments(1), 0);
        assert_eq!(dedicated_segments(LARGEST_SHARED_ALLOC), 0);
        assert_eq!(LARGEST_SHARED_ALLOC, SEG - SLICE, "the header owns slice 0");

        // One byte over, and the request owns segments outright. TWO of them,
        // always: the header cannot share the segment its payload fills.
        assert_eq!(dedicated_segments(LARGEST_SHARED_ALLOC + 1), 2);
        assert_eq!(dedicated_segments(SEG), 2);
        assert_eq!(dedicated_segments(2 * SEG), 3);

        // Monotone, and never less than the payload needs.
        let mut prev = 0;
        let mut size = 0;
        while size < 5 * SEG {
            let d = dedicated_segments(size);
            assert!(d >= prev, "cost cannot fall as the request grows");
            if d > 0 {
                assert!(d * SEG >= size + SLICE, "must hold header plus payload");
            }
            prev = d;
            size += SLICE / 2 + 1;
        }

        // The region a firmware must declare, including the segment the small
        // allocations take when the large ones are dedicated.
        let three = region_for_allocs(SEG, 3);
        assert_eq!(three % SEG, 0, "whole segments");
        assert_eq!(good_region_size(three), three, "already a good size");
        if dedicated_segments(SEG) == 0 {
            // A sharing geometry: three spans plus a slice for the smalls.
            assert!(three <= 2 * SEG, "sharing should not need a segment each");
        } else {
            assert_eq!(
                three,
                (3 * dedicated_segments(SEG) + 1) * SEG,
                "three dedicated runs, plus one segment for everything smaller"
            );
        }
        // The reported case, pinned at the geometry it was measured on: a
        // 256 KiB region is four segments and serves ONE 64 KiB block once a
        // small allocation has taken a segment.
        if SEG == 64 * 1024 {
            assert_eq!(dedicated_segments(64 * 1024), 2);
            assert_eq!(region_for_allocs(64 * 1024, 3), 448 * 1024);
            assert!(
                region_for_allocs(64 * 1024, 3) > 256 * 1024,
                "the reported 256 KiB region cannot hold three, and now says so"
            );
        }
    }

    /// `docs/plans/finished/region-alignment-bug.md` §5: for any base, a
    /// region sized by `good_region_size` either delivers the segments its
    /// name implies, or the caller is told it did not. Both halves failed
    /// when the report was written.
    #[test]
    fn a_misaligned_exact_region_is_refused_not_served_short() {
        use crate::types::SEGMENT_SIZE as SEG;
        // The report's base, at the small profile. 0x1e4 is 4-aligned, not
        // 16-aligned: the base is 12 bytes off the grid. When segments had to
        // be segment-aligned that cost 24,092 bytes; now it costs 12 — and
        // against an EXACT length, 12 bytes is still a segment.
        let base = 0x3fc8_a1e4usize;
        let n = good_region_size(220 * 1024);
        // The reported case is a DEFAULT-geometry case: a 220 KiB budget is
        // three 64 KiB segments. At a raised `ra_segment_size` it is one
        // segment or none, and none of the numbers below describe it.
        if n >= MIN_REGION && SEG == 64 * 1024 {
            assert_eq!(n, 196_608);
            assert_eq!(usable_bytes(base, n), 131_072, "two segments, not three");
            assert!(usable_bytes(base, n) < n);
            assert_eq!(usable_bytes(0, n), 196_608, "what the name promised");
            if cfg!(ra_aligned_region) {
                assert_eq!(
                    usable_bytes(base + 12, n),
                    131_072,
                    "masked: the run-up is 24,080"
                );
                assert_eq!(
                    usable_bytes(base, 200_704),
                    131_072,
                    "the 2.0.3 shape, same loss"
                );
            } else {
                assert_eq!(
                    usable_bytes(base + 12, n),
                    196_608,
                    "the same region on the grid is the three it says"
                );
                assert_eq!(
                    usable_bytes(base, 200_704),
                    196_608,
                    "the 2.0.3 shape carries 4 KiB of slack, which absorbs 12 bytes"
                );
            }
            // The round number the report says was accidentally safe: it
            // strands 28,672 aligned and loses 12 here -- same three
            // segments, so no claim of exactness is broken and it is NOT the
            // misaligned case.
            let round = 220 * 1024;
            assert_eq!(usable_bytes(base, round), usable_bytes(0, round));
            assert_eq!(usable_bytes(base, round), 196_608);
        }

        // The predicate `init_region` now refuses on, at any geometry: the
        // base costs a segment against the aligned promise. One segment from
        // a base one byte past a boundary yields nothing (GEOMETRY, checked
        // first); two segments yield one.
        let two = 2 * SEG;
        assert!(usable_bytes(SEG + 1, two) < usable_bytes(0, two));
        assert_eq!(usable_bytes(SEG + 1, two), SEG);

        // And `init_region` says so, before it touches any state -- so this
        // probe neither needs nor consumes the process-wide registration.
        #[cfg(ra_small_profile)]
        {
            // Two segments EXACTLY, so the base's run-up costs the last one
            // (one segment would be refused as GEOMETRY before MISALIGNED
            // could fire). Derived, so `ra_segment_size` moves it.
            const M: usize = 2 * crate::types::SEGMENT_SIZE;
            const SLACK: usize = crate::types::SEGMENT_SIZE + 0x1e4;
            static mut MIS: [u8; M + SLACK] = [0; M + SLACK];
            let bp = (&raw mut MIS).cast::<u8>().expose_provenance();
            // Put the base at the report's residue, 0x1e4 past a boundary:
            // 12 bytes off the 16-byte grid.
            let want = (bp & !(SEG - 1)) + SEG + 0x1e4;
            let skip = want - bp;
            assert!(skip <= SLACK);
            // SAFETY: `skip + M <= M + SLACK` by the bound just asserted, and
            // this slice is refused before anything retains it.
            let region: &'static mut [u8] = unsafe {
                core::slice::from_raw_parts_mut((&raw mut MIS).cast::<u8>().add(skip), M)
            };
            assert_eq!(
                init_region(region),
                Err(FERR_MISALIGNED),
                "an exact size at a base that costs a segment must be refused, not served short"
            );
        }
    }

    /// The container that makes the refusal unreachable: aligned by
    /// construction, whole segments so it is not padded, handed over once.
    #[test]
    fn region_type_is_aligned_and_unpadded() {
        use crate::types::SEGMENT_SIZE as SEG;
        assert_eq!(
            core::mem::align_of::<Region<MIN_REGION>>(),
            REGION_ALIGN,
            "16 bytes: segments stride from the base, so the type owes the linker no gap"
        );
        assert_eq!(
            core::mem::size_of::<Region<MIN_REGION>>(),
            MIN_REGION,
            "a whole-segment region carries no padding"
        );
        assert_eq!(Region::<MIN_REGION>::USABLE, SEG);
        #[cfg(ra_small_profile)]
        {
            // A plain `static`, exactly as a firmware declares it. (While the
            // type was segment-aligned this had to be a leaked `Box`: a
            // 64 KiB-aligned static did not compile on every host toolchain,
            // and `Box::new(Region::new())` faulted past Windows' guard page.)
            #[cfg(not(ra_aligned_region))]
            let r: &'static Region<MIN_REGION> = {
                static R: Region<MIN_REGION> = Region::new();
                &R
            };
            // Under `ra_aligned_region` the type is segment-aligned again, and
            // a 64 KiB-aligned static does not compile on every host toolchain;
            // allocate it in place instead. Zero is a valid `Region`.
            // SAFETY: an all-zero `Region` is a valid value (its only field is
            // a byte array), so `assume_init` on zeroed storage is sound.
            #[cfg(ra_aligned_region)]
            let r: &'static Region<MIN_REGION> =
                Box::leak(unsafe { Box::<Region<MIN_REGION>>::new_zeroed().assume_init() });
            assert_eq!(r.usable(), Region::<MIN_REGION>::USABLE);
            assert_eq!(r.len(), MIN_REGION);
            assert!(!r.is_empty());
            // NOT given here: the process-wide registration belongs to the
            // region test above, and a second registration is refused.
            // `give` itself is exercised in `tests/region.rs`, its own process.
        }
    }

    /// The Kairos RTOS measured a step at 512 on a 32-bit device and could not
    /// reproduce it on a 64-bit host. The reason is here, as an assertion
    /// rather than as prose: the `direct[]` route's top is a function of
    /// POINTER WIDTH, so it lands on a different size on the two machines and
    /// a host sweep cannot see the device's boundary
    /// (`docs/plans/finished/fixed-prim-small-step.md`).
    #[test]
    fn the_direct_route_boundary_moves_with_pointer_width() {
        use crate::types::{SMALL_OBJ_SIZE_MAX, SMALL_SIZE_MAX, SMALL_WSIZE_MAX};
        assert_eq!(
            SMALL_SIZE_MAX,
            SMALL_WSIZE_MAX * core::mem::size_of::<usize>()
        );
        assert!(shape_of(SMALL_SIZE_MAX).direct_route);
        assert!(!shape_of(SMALL_SIZE_MAX + 1).direct_route);
        // The route top is 1024 on 64-bit and 512 on 32-bit, whatever the
        // geometry -- it is a pointer-width fact, not a profile one.
        // At the DEFAULT `SMALL_WSIZE_MAX`. The relationship asserted above
        // holds at every setting of `ra_small_wsize`; these literals are the
        // default arm's, and are pinned as such rather than unconditionally.
        #[cfg(not(any(ra_small_wsize = "256", ra_small_wsize = "512")))]
        if core::mem::size_of::<usize>() == 8 {
            assert_eq!(SMALL_SIZE_MAX, 1024);
        } else if core::mem::size_of::<usize>() == 4 {
            assert_eq!(SMALL_SIZE_MAX, 512);
        }
        // WHETHER it coincides with the small-page top is a fact about the
        // GEOMETRY, so it is derived rather than asserted -- an unconditional
        // `assert_ne!` here passed at the default slice and failed under
        // `ra_segment_size="256k"`, where an 8 KiB slice puts
        // SMALL_OBJ_SIZE_MAX at 1024 and the two meet on 64-bit too.
        //
        // The coincidence is the interesting part and is what made the device
        // confusing: where the two constants land on the same byte, one step
        // hides two boundaries and a sweep cannot tell them apart.
        assert_eq!(SMALL_OBJ_SIZE_MAX, crate::types::SEGMENT_SLICE_SIZE / 8);
        let coincide = SMALL_SIZE_MAX == SMALL_OBJ_SIZE_MAX;
        assert_eq!(
            coincide,
            SMALL_WSIZE_MAX * core::mem::size_of::<usize>() == crate::types::SEGMENT_SLICE_SIZE / 8,
            "the two boundaries coincide exactly when the arithmetic says so"
        );
        // The page kinds a firmware could not observe before.
        assert!(shape_of(16).page_bytes <= shape_of(SMALL_OBJ_SIZE_MAX).page_bytes);
        assert!(
            shape_of(SMALL_OBJ_SIZE_MAX + 1).page_bytes > shape_of(SMALL_OBJ_SIZE_MAX).page_bytes
        );
        assert_eq!(shape_of(16).dedicated_segments, 0);
        assert!(shape_of(LARGEST_SHARED_ALLOC + 1).dedicated_segments >= 1);
    }

    #[test]
    fn usable_bytes_answers_the_question_a_firmware_asks() {
        let seg = SEGMENT_SIZE;

        // Aligned base: MIN_REGION is exactly enough for one segment, and one
        // byte less is not.
        assert_eq!(
            usable_bytes(0, MIN_REGION),
            seg,
            "MIN_REGION buys a segment"
        );
        assert_eq!(
            usable_bytes(0, MIN_REGION - 1),
            0,
            "one byte short buys none"
        );

        // No page is reserved: a region of exactly one segment serves one
        // segment, because the first heap's descriptor is a static, not a
        // page of the region (2.0.4; it used to be, and this used to be 0).
        assert_eq!(usable_bytes(0, seg), seg, "a bare segment is a segment");

        // A base ANYWHERE on the 16-byte grid loses nothing: segments stride
        // from it, so a page-aligned base off every segment boundary serves
        // exactly what an aligned one does. Until 2.0.4 this was 0.
        assert_eq!(
            usable_bytes(FIXED_PAGE, MIN_REGION),
            if cfg!(ra_aligned_region) { 0 } else { seg },
            "a base off the segment grid serves the segment, because strides start at the base \
             (masked, the run-up eats it)"
        );
        assert_eq!(usable_bytes(REGION_ALIGN, MIN_REGION), seg);
        assert_eq!(usable_bytes(3 * seg + 5 * REGION_ALIGN, MIN_REGION), seg);
        // A base OFF the 16-byte grid loses the run-up to it -- at most 15
        // bytes, and against an exact length that is still a whole segment.
        // This is why `init_region` checks the real base rather than comparing
        // `len` against `MIN_REGION`: this region is >= MIN_REGION and still
        // yields nothing.
        assert_eq!(
            usable_bytes(REGION_ALIGN + 1, MIN_REGION),
            0,
            "a base off the grid eats the segment"
        );
        assert_eq!(
            usable_bytes(REGION_ALIGN + 1, MIN_REGION + REGION_ALIGN),
            seg,
            "sixteen bytes of slack absorb it"
        );

        // The stranded tail, which used to be recorded only in a design doc.
        // Sized in segments so it says the same thing at either geometry; at
        // the small profile this is the report's 220 KiB case exactly.
        let three_and_a_bit = 3 * seg + seg / 2;
        assert_eq!(
            usable_bytes(0, three_and_a_bit),
            3 * seg,
            "a ragged region yields whole segments and strands the remainder"
        );
        let stranded = three_and_a_bit - usable_bytes(0, three_and_a_bit);
        assert_eq!(
            stranded,
            seg / 2,
            "and the strand is exactly the ragged part"
        );
    }

    /// The reentrancy detector, watched firing.
    ///
    /// "A failure mode nobody has watched fire is a claim, not a defence" is
    /// this repo's own line, and it applies to the thing that replaced the
    /// hang. Only compiled where the detector is: run it with
    /// `RUSTFLAGS="--cfg ra_single_threaded" cargo test -p rusty_alloc --lib prim::fixed`,
    /// which CI does.
    ///
    /// Deliberately NOT in `tools/gate-selftest.sh`: poisoning this gate
    /// removes the detector, and the test then HANGS instead of failing --
    /// which is the whole point of the defect, and useless in a CI job. The
    /// evidence that it fires is this test passing where the detector exists
    /// and the code not compiling it where it does not.
    #[cfg(ra_single_threaded)]
    #[test]
    #[should_panic(expected = "re-entered")]
    fn a_reentrant_acquire_is_diagnosed_not_hung() {
        static LOCK2: AtomicBool = AtomicBool::new(false);
        let _outer = Guard::acquire(&LOCK2);
        // Exactly what an allocating ISR does: acquire while the outer context
        // still holds it. Without the detector this line never returns.
        let _inner = Guard::acquire(&LOCK2);
    }

    /// The no-MMU decisions, pinned so a future edit has to mean it.
    #[test]
    fn no_mmu_semantics_are_explicit() {
        let cfg = mem_init();
        assert_eq!(cfg.page_size, FIXED_PAGE);
        assert_eq!(cfg.large_page_size, 0, "no large pages without an MMU");
        assert!(!cfg.has_overcommit, "nothing to overcommit");
        assert!(cfg.has_partial_free, "any extent can be returned");
        assert_ne!(thread_id(), 0, "zero is the abandoned-segment sentinel");
        assert_eq!(numa_node_count(), 1);
        // A monotonic counter, not a clock.
        assert!(clock_now() < clock_now());
        // SAFETY: `protect` on this backend inspects nothing and always fails;
        // it never dereferences the pointer, so a null one is in contract.
        let p = unsafe { protect(core::ptr::null_mut(), FIXED_PAGE, true) };
        assert!(
            p.is_err(),
            "a guard page that cannot trap must not report success"
        );
    }
}
