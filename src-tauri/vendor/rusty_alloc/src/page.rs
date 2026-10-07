//! Pages and their three sharded free lists (mirrors upstream `page.c` data
//! side). A page is a slice-span inside a segment holding blocks of ONE size.
//!
//! The three lists (the mimalloc signature move):
//! - `free`        — the allocation fast path pops here; when it runs dry the
//!   slow path runs at a regular cadence (the heartbeat).
//! - `local_free`  — frees from the owning thread; swapped into `free` on
//!   collect. Separate so the fast list running dry MEANS a heartbeat is due.
//! - `xthread_free` — frees from OTHER threads: an atomic word packing a
//!   block-list head with a 2-bit protocol flag (loom-modeled in
//!   `tests/loom_xthread.rs`, which is the specification):
//!   `NORMAL` remote pushes land here; `DELAYED` remotes nudge the OWNER's
//!   delayed list instead (page invisible to scans: full queue / large span);
//!   `FREEING` transient guard while a remote dereferences the heap pointer —
//!   the abandoner spins this out before heap teardown; `NEVER` abandoned.
//!
//! Owner-only fields (everything non-atomic) are mutated exclusively by the
//! owning thread (`Segment::thread_id` gates entry in `alloc::free`).

use core::ptr;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

/// `Page::flags` bits. The FREE fast path must answer one question — "is this
/// a plain binned page I can just push onto?" — and it used to answer it with
/// three separate loads (`has_aligned`, `bin == BIN_HUGE`, `in_full`) plus the
/// segment's `kind`. Folding them into one byte turns that into a single load
/// and a single test-against-zero (M9 brick #3).
///
/// Any bit set ⇒ leave the fast path and take the general route.
pub mod pflags {
    /// Some block was handed out ADJUSTED (aligned-at interior pointer):
    /// free/usable must recover the block start by block arithmetic.
    pub const HAS_ALIGNED: u8 = 1 << 0;
    /// Unqueued single-block span (large) or a huge segment's page.
    pub const SINGLE_BLOCK: u8 = 1 << 1;
    /// Currently parked in the full queue (free must un-park it).
    pub const IN_FULL: u8 = 1 << 2;
    /// The page lives in a dedicated Huge segment (whole-reservation free).
    pub const HUGE_SEGMENT: u8 = 1 << 3;
    /// Mask of everything the free fast path must NOT see.
    pub const SLOW_FREE: u8 = HAS_ALIGNED | SINGLE_BLOCK | IN_FULL | HUGE_SEGMENT;
}

/// Flag mask in the `xthread_free` word (blocks are ≥ 8-aligned).
pub const XMASK: usize = 0b11;
/// Remote frees push onto the page's own xthread list.
pub const XFLAG_NORMAL: usize = 0;
/// Remote frees push onto the owning heap's delayed list.
pub const XFLAG_DELAYED: usize = 1;
/// Transient: a remote holds the heap pointer; others spin, abandoner waits.
pub const XFLAG_FREEING: usize = 2;
/// Abandoned: no owning heap; remote frees use the page list (adopter drains).
pub const XFLAG_NEVER: usize = 3;

/// A free block: the first word of the block memory itself links the list.
#[repr(C)]
pub struct Block {
    /// Next free block. In the default build this is a plain pointer (the
    /// oracle's release default). Under `secure` it is ENCODED — see
    /// [`block_set_next`]/[`block_next`]: `enc = (next + key2) ^ key1`, so an
    /// overflow that overwrites the link cannot steer the allocator without
    /// knowing both per-page keys, and a link that decodes outside the
    /// carrying block's own segment, or off block alignment, aborts instead of
    /// being followed.
    ///
    /// Note the DEFAULT build performs neither the encoding nor the check —
    /// a link overwrite steers it freely. That is deliberate parity with the
    /// oracle's release default, and it makes enabling `secure` a security
    /// decision rather than a performance one. See `docs/threat-model.md`.
    pub next: *mut Block,
}

/// Modular inverse of an ODD `n` mod 2^64 (Newton–Raphson, 5 doublings).
///
/// Used by the `blockmap` feature to turn an address delta into a block index
/// WITHOUT a division. Block sizes here are not powers of two — the bins run
/// `5,6,7,8 << k`, giving 24, 40, 80, 96, 112… — so the obvious
/// `delta / block_size` is a real hardware divide on the allocation hot path,
/// which costs more than the whole check it would serve.
///
/// The trick works because `delta` is always an exact multiple of the block
/// size for a genuine block. Write `block_size = odd << k`; then
/// `delta / block_size == (delta >> k) * inverse(odd)` in wrapping 64-bit
/// arithmetic — one shift and one multiply.
///
/// For a delta that is NOT an exact multiple — a corrupted or misaligned
/// pointer — the result is one of two things, and BOTH are safe, though the
/// first is not what a first reading suggests:
///
/// - offset below `2^k`: the shift discards it, so the result is the index of
///   the CONTAINING block. For `block_size = 24` (`k = 3`), `4*24 + 1` gives
///   index 4, not garbage. That is correct behaviour for a liveness map — a
///   pointer inside block 4 should consult block 4's bit — but it means this
///   is NOT a free alignment check, which an earlier version of this comment
///   wrongly claimed. Alignment is `link_is_plausible`'s job.
/// - any larger non-multiple: a garbage index, which overwhelmingly fails the
///   `idx < capacity` bound that follows. The chance of a random 64-bit value
///   landing under a few thousand is about 2^-52.
// Not `blockmap`-gated: `bins::exact_div_by_block_size` derives its four
// constants from this at compile time in every build.
#[inline]
pub(crate) const fn odd_mod_inverse(n: usize) -> usize {
    debug_assert!(n % 2 == 1);
    // x = n is correct to 3 bits; each Newton step doubles that: 6, 12, 24,
    // 48, 96 — so five steps cover 64 bits with room to spare.
    let mut x = n;
    let mut i = 0;
    while i < 5 {
        x = x.wrapping_mul(2usize.wrapping_sub(n.wrapping_mul(x)));
        i += 1;
    }
    x
}

/// `blockmap`: bytes of map needed for `blocks` bits, rounded up so the first
/// block still lands on a `MAX_ALIGN_SIZE` boundary.
#[cfg(feature = "blockmap")]
#[inline]
pub(crate) const fn bitmap_bytes(blocks: usize) -> usize {
    blocks
        .div_ceil(8)
        .next_multiple_of(crate::types::MAX_ALIGN_SIZE)
}

/// A block was handed out while already live, or freed while already free.
///
/// Silent for the same reasons as [`corrupt_free_list_abort`]: formatting a
/// message from inside the allocator can allocate, and unwinding would cross
/// `extern "C"` frames.
#[cold]
#[inline(never)]
#[cfg(feature = "blockmap")]
pub(crate) fn blockmap_abort() -> ! {
    crate::abort()
}

/// `blockmap`: flip `b`'s liveness bit, requiring it to currently be the
/// opposite of `to_live`.
///
/// **This is the check that actually stops R-005.** Encoding and bounds both
/// try to keep a link from being FORGED; this one detects what a forgery is
/// FOR — the allocator handing out a block that is already live — and so it
/// does not care how the link was produced. It carries no key, and the map is
/// not part of the free list, so unlike the encoding it is not undone by an
/// attacker with a read primitive.
///
/// Owner-only, therefore no atomics: the two callers are the allocation pop
/// and the local free. Blocks freed by OTHER threads land on `xthread_free`
/// and have their bit cleared later, by the owner, in [`page_collect`].
///
/// # Safety
/// `page` live and owned by the caller; `b` a block of that page.
#[cfg(feature = "blockmap")]
#[inline]
unsafe fn blockmap_transition(page: *mut Page, b: *const u8, to_live: bool) {
    // SAFETY: owner-only fields; `b` is a block of this page per the contract.
    unsafe {
        let payload = (*page).payload;
        if payload.is_null() {
            // Huge/large single-block pages carve no map — nothing to track.
            return;
        }
        let bsize = (*page).block_size;
        let reserved = (*page).reserved as usize;
        let delta = b.addr().wrapping_sub(payload.addr());
        let idx = (delta >> bsize.trailing_zeros()).wrapping_mul((*page).bs_inv);
        if idx >= reserved {
            // Either a pointer that is not a block of this page at all, or a
            // non-multiple delta that produced a garbage index. Both mean the
            // free list has been steered somewhere it cannot legitimately go.
            blockmap_abort();
        }
        let byte = payload.add(reserved * bsize).add(idx >> 3);
        let mask = 1u8 << (idx & 7);
        if (byte.read() & mask != 0) == to_live {
            // Allocating a block already marked live is the free-list hijack
            // landing; freeing one already marked free is a double free.
            blockmap_abort();
        }
        byte.write(byte.read() ^ mask);
    }
}

/// Could `dec` be a genuine free-list link stored in a block at `b_addr`?
///
/// The bound is the SEGMENT, not the page, and that is a measured decision
/// rather than the tightest statement available.
///
/// A page-scoped bound is sound and ~512x tighter (±64 KiB against ±32 MiB),
/// and it was implemented and tested. It costs **+5 Ir/op** on the small and
/// batch ops — `secure` goes from +14.99 to +21.97 per allocation, perl from
/// 1.0160 to 1.0214 — and reformulating it (`slice_count << 16` instead of
/// `capacity * block_size`) changed the number by literally nothing, so the
/// cost is the extra load and the wider comparison, not the arithmetic.
///
/// It was dropped because of what it does NOT buy. Narrowing from segment to
/// page removes cross-page redirection, but the attack that matters here is
/// INTRA-page — R-005's relative forgery, redirecting a link to a neighbouring
/// block. A page bound is powerless against that. The only defence that covers
/// it is the `blockmap` liveness map, which is measured at +58 Ir/op and
/// therefore off by default. Paying 0.5% for a partial narrowing, when the
/// complete answer is already too expensive to run, is not a good trade.
///
/// Segments are SEGMENT_SIZE-aligned and SEGMENT_SIZE is a power of two, so
/// `(a ^ b) < SEGMENT_SIZE` IS "same segment" in two ALU ops with no memory
/// access — cheaper than asking the global segment map (an atomic load) and
/// stronger than it, since the map would accept any segment we own. On a
/// fixed region the segments stride from the region's base instead
/// (`crate::REGION_STRIDES`), and the same identity holds of the two OFFSETS
/// from it.
///
/// Split out of [`block_next`] so the PREDICATE can be tested exhaustively
/// in-process (`link_tests` below) while the fault PATH — which aborts, and so
/// needs a child process — is tested separately in `tests/corruption.rs`.
/// Testing an abort end-to-end proves the wiring; it cannot enumerate the
/// boundary cases, and the boundary is where a bounds check earns its keep.
///
/// Compiled in every configuration (only the CALL SITE is `secure`-gated) and
/// public so `fuzz_targets/corruption.rs` can differentially check the
/// `(a ^ b) < SEGMENT_SIZE` identity against a naive reference over arbitrary
/// inputs. That identity is the one clever line here, and clever is exactly
/// what wants a fuzzer pointed at it. Not part of the supported API.
#[inline]
#[doc(hidden)]
pub fn link_is_plausible(dec: usize, b_addr: usize) -> bool {
    // On a fixed region "same segment" is a property of the offsets from the
    // region's base, not of the addresses; the base is `MAX_ALIGN_SIZE`-
    // aligned, so the alignment test reads the same on either.
    let (dec, b_addr) = if crate::REGION_STRIDES {
        let base = crate::prim::fixed::stride_base();
        (dec.wrapping_sub(base), b_addr.wrapping_sub(base))
    } else {
        (dec, b_addr)
    };
    dec.is_multiple_of(crate::types::MAX_ALIGN_SIZE.min(8))
        && (dec ^ b_addr) < crate::types::SEGMENT_SIZE
}

/// Read a free-list link (decoding under `secure`).
///
/// # Safety
/// `b` must be a live free block of `page`.
#[inline]
pub unsafe fn block_next(page: *const Page, b: *const Block) -> *mut Block {
    // SAFETY: b is a valid free block; its first word holds the link.
    unsafe {
        #[cfg(not(feature = "secure"))]
        {
            let _ = page;
            let n = (*b).next;
            #[cfg(feature = "linkcheck")]
            {
                // NULL TERMINATES the list — it is not a link, and checking it
                // would abort on every list tail. The `secure` arm gets this
                // for free from its `enc == 0` early return; this arm has no
                // such return and must say so.
                //
                // This arm DID NOT COMPILE from the day the page-extent
                // narrowing was dropped (it still passed the extent as a third
                // argument) until 2026-09-09: CI builds only the default and
                // `--all-features`, and `linkcheck` without `secure` is
                // neither. Found by a feature bisection of the `stress_mt`
                // flake — 40/40 "failures" that were one rustc error. There
                // is now a per-feature clippy step so a combination nobody
                // runs cannot rot again.
                if !n.is_null() && !link_is_plausible(n.addr(), b.addr()) {
                    corrupt_free_list_abort();
                }
            }
            n
        }
        #[cfg(feature = "secure")]
        {
            let enc = (*b).next as usize;
            if enc == 0 {
                return core::ptr::null_mut();
            }
            let keys = (*page).keys;
            let dec = (enc ^ keys[0]).wrapping_sub(keys[1]);
            // A decoded link must be a block-aligned address inside the SAME
            // segment as the block carrying it — anything else means the list
            // was corrupted (overflow/UAF) and must not be followed.
            //
            // Same-segment is the tight invariant: a page is a slice-span
            // within ONE 32 MiB segment, so every block of a page — and hence
            // every link in its free list — shares that segment. Because
            // segments are SEGMENT_SIZE-aligned and SEGMENT_SIZE is a power of
            // two, `(a ^ b) < SEGMENT_SIZE` IS "same segment", in two ALU ops
            // with no memory access. That is both cheaper than asking the
            // global segment map (an atomic load) and strictly stronger than
            // it: the map would accept any segment we own, this accepts only
            // the one the link must be in.
            //
            // Alignment alone — what this checked originally — stops
            // accidental corruption (a stray ASCII overflow fails it 7 times
            // in 8) but barely inconveniences a deliberate attacker, since
            // every target worth steering an allocator at is already
            // pointer-aligned. The bound is what removes out-of-heap targets
            // (GOT entries, vtables, saved return addresses) from reach.
            //
            if !link_is_plausible(dec, b.addr()) {
                corrupt_free_list_abort();
            }
            crate::ptr_with_addr(b.cast_mut(), dec)
        }
    }
}

/// Write a free-list link (encoding under `secure`).
///
/// # Safety
/// `b` must be a dead block of `page`; `next` null or a block of `page`.
#[inline]
pub unsafe fn block_set_next(page: *const Page, b: *mut Block, next: *mut Block) {
    // SAFETY: b is dead memory we own; its first word is the link slot.
    unsafe {
        #[cfg(not(feature = "secure"))]
        {
            let _ = page;
            (*b).next = next;
        }
        #[cfg(feature = "secure")]
        {
            if next.is_null() {
                (*b).next = core::ptr::null_mut();
            } else {
                let keys = (*page).keys;
                let enc = (next.addr().wrapping_add(keys[1])) ^ keys[0];
                (*b).next = crate::ptr_with_addr(b, enc);
            }
        }
    }
}

/// A heap's cross-thread delayed-free list. Lives inside the owner's HeapBox;
/// pages carry its address in `xheap` so remote threads can reach it without
/// knowing the heap type. Plain Treiber push / owner swap-drain.
pub struct DelayedList {
    /// Head block (no flag bits).
    ///
    /// A `usize` rather than a pointer ON PURPOSE, and this is the one place
    /// in the crate where that is correct: the cross-thread protocol packs a
    /// 2-bit state flag into the low bits of this word and CASes the pair
    /// atomically (blocks are >= 8-aligned, so the bits are free). An
    /// `AtomicPtr` cannot carry the flag, and splitting them would break the
    /// single-CAS invariant the loom model verifies.
    // nosemgrep: pointer-stored-as-integer -- packed flag word, see above
    pub head: AtomicUsize,
}

impl DelayedList {
    /// Const-init empty list.
    pub const fn new() -> DelayedList {
        DelayedList {
            head: AtomicUsize::new(0),
        }
    }
}

impl Default for DelayedList {
    fn default() -> Self {
        Self::new()
    }
}

/// Page metadata. Lives in the owning segment's header slice; the payload
/// ("page area") is the corresponding slice span.
pub struct Page {
    /// Fast-path free list (owner-only).
    pub free: *mut Block,
    /// Owner-thread frees since last collect (owner-only).
    pub local_free: *mut Block,
    /// Cross-thread word: block-list head | 2-bit flag (see module docs).
    pub xthread_free: AtomicUsize,
    /// Address of the owning heap's [`DelayedList`] (0 while unowned).
    pub xheap: AtomicUsize,
    /// Next page in its queue (owner-only).
    pub next: *mut Page,
    /// Previous page in its queue (owner-only).
    pub prev: *mut Page,
    /// Blocks currently allocated from this page (owner-only; lags remote
    /// frees until collect).
    pub used: u32,
    /// Blocks handed to the free list so far (lazy extension high-water mark).
    pub capacity: u32,
    /// Maximum blocks this page can hold.
    pub reserved: u32,
    /// Block size in bytes (0 = free span / unused slot).
    pub block_size: usize,
    /// Payload start of this page (`seg + idx * SEGMENT_SLICE_SIZE`), cached so
    /// the refill path never has to recover it from the slot pointer.
    ///
    /// Recovering it means `page_index` — `(slot - pages_base) / size_of::
    /// <Page>()`, a division by a non-power-of-two — followed by a multiply
    /// back. That is fine ONCE, at carve, where the slice index is already an
    /// integer (`span_mark`'s `idx`), so this is written there with a shift and
    /// no division. It replaces three `segment_of + page_index + page_area`
    /// chains on the generic refill path — exactly where batch alloc/free churn
    /// lives, our one measured loss to mimalloc — with a single load. The
    /// address is a fixed geometric property of the slot, so it never changes
    /// once set. Null in the empty sentinel, which is never carved from.
    pub area: *mut u8,
    /// Slices this page spans.
    pub slice_count: u16,
    /// For interior slices: distance BACK to the span-start slot, **in
    /// slices** (not in bytes).
    ///
    /// This held BYTES until 2026-08-22, pre-scaled by `size_of::<Page>()`,
    /// because `page_of` followed it back on every free and a slice count
    /// would have to be scaled there. **That justification expired** when the
    /// `Segment::page_off` table took over `page_of`: in a release build the
    /// only remaining reader is `span_free`'s coalesce-left, and the only
    /// byte-scaled reader is a `debug_assert`.
    ///
    /// Bytes therefore bought nothing and cost a division. `size_of::<Page>()`
    /// is 88 — `8 * 11`, not a power of two — so recovering the slice count
    /// from bytes is a real `movabs; mul; shr` magic-multiply sequence, which
    /// is what `docs/opps.md` #1 named. In slices the same read is a bare
    /// subtract, and the two loops that WRITE the field drop a multiply each.
    pub slice_offset: u16,
    /// Bin index this page is queued under (BIN_HUGE marks unqueued larges).
    pub bin: u8,
    /// Fast-path flag byte (see [`pflags`]): any bit set ⇒ the free fast path
    /// must take the general route.
    ///
    /// **Atomic because it is genuinely raced, and ThreadSanitizer proved it**
    /// (2026-08-19, hardening gate H-24). A thread ADOPTING an abandoned
    /// segment clears `IN_FULL` on each of its pages (`adopt_segment`) while
    /// another thread can concurrently be in `free()` reading this byte to
    /// route the free. Both readings lead to the same outcome for a remote
    /// free — so the bug never manifested — but a non-atomic read racing a
    /// non-atomic read-modify-write is undefined behaviour regardless of
    /// whether today's codegen is kind about it, and this crate's own history
    /// (the aarch64 first-execution defects) is what a "benign on x86-TSO"
    /// race looks like right before it stops being benign.
    ///
    /// `Relaxed` is the correct and sufficient ordering: this byte carries no
    /// happens-before obligation of its own — the segment's `thread_id`
    /// Acquire load already orders everything the free path needs.
    ///
    /// **It costs exactly one instruction on the free fast path, measured.**
    /// A plain field read let LLVM fold the load into the test's memory
    /// operand (`test BYTE PTR [pg+0x4d], 0xf`); it will not fold an ATOMIC
    /// load, so the sequence becomes `movzx` + `test`. Cost: batch_lifo
    /// 59.17 → 60.17 Ir/op, and every other operation +1.00 exactly. That is
    /// the same trade this crate already made for double-free detection
    /// (~0.4%, kept deliberately): an allocator whose premise is memory
    /// safety does not keep a data race on its hottest path to win 1.7% of a
    /// synthetic microbenchmark. Upstream mimalloc reads these flags
    /// non-atomically and does not pay the instruction — and has the race.
    pub flags: AtomicU8,
    /// The un-extended tail AND current free list are known zero.
    pub free_is_zero: bool,
    /// This span's memory was PURGED (decommitted/reset) while free. It must
    /// be re-committed before reuse — on Windows a decommitted range faults
    /// on touch (Linux MADV_DONTNEED does not, which is exactly why this was
    /// a Windows-only access violation until the recommit landed).
    pub purged: bool,
    /// Owning heap's tag (`mi_heap_new_ex`) — survives abandonment so
    /// `mi_abandoned_visit_blocks` can filter (upstream stores it on the page
    /// for the same reason).
    pub heap_tag: i32,
    /// Per-page free-list encoding keys (`secure` builds only; zero elsewhere).
    #[cfg(feature = "secure")]
    pub keys: [usize; 2],
    /// `blockmap`: first byte of block storage. The liveness map sits just
    /// past the last block, at `payload + reserved * block_size`.
    ///
    /// **Only two fields, and that is a hard constraint, not a preference.**
    /// `Segment` holds `[Page; SLICES_PER_SEGMENT]` and a const assertion
    /// requires the whole header to fit in slice 0, so every byte added here
    /// is multiplied by 512. A first cut also stored the map pointer and the
    /// shift; together with `secure`'s keys that broke the assertion at
    /// COMPILE time. Both are cheap to derive — the map from `payload`, the
    /// shift from `block_size.trailing_zeros()` — so they are.
    #[cfg(feature = "blockmap")]
    pub payload: *mut u8,
    /// `blockmap`: modular inverse of the odd part of `block_size`. This one
    /// cannot be derived cheaply — the inverse is five multiplies — so it is
    /// the one value worth the space.
    #[cfg(feature = "blockmap")]
    pub bs_inv: usize,
}

/// `debug_checks` invariant guard (our `dmi` equivalent): a page slot handed
/// to the hot paths must be a live SPAN START with self-consistent counters.
/// Catching a violated invariant here turns "mystery access violation" into
/// "this field was wrong, at this call site".
///
/// # Safety
/// `page` must be a page slot the caller is already entitled to read.
#[inline]
pub unsafe fn debug_validate_page(page: *const Page, where_: &str) {
    #[cfg(feature = "debug_checks")]
    {
        // SAFETY: callers pass a page pointer they are about to use anyway;
        // reading its metadata is exactly as valid as that use.
        unsafe {
            assert!(!page.is_null(), "{where_}: null page");
            assert_eq!((*page).slice_offset, 0, "{where_}: not a span start");
            assert!((*page).block_size > 0, "{where_}: dead page (block_size 0)");
            assert!(
                (*page).block_size.is_multiple_of(8),
                "{where_}: block_size {} not word-aligned",
                (*page).block_size
            );
            assert!((*page).slice_count > 0, "{where_}: zero slice_count");
            assert!(
                (*page).capacity <= (*page).reserved,
                "{where_}: capacity {} > reserved {}",
                (*page).capacity,
                (*page).reserved
            );
            assert!(
                (*page).used <= (*page).capacity,
                "{where_}: used {} > capacity {}",
                (*page).used,
                (*page).capacity
            );
            assert!(
                ((*page).bin as usize) <= crate::types::BIN_FULL,
                "{where_}: bin {} out of range",
                (*page).bin
            );
        }
    }
    #[cfg(not(feature = "debug_checks"))]
    {
        let _ = (page, where_);
    }
}

impl Page {
    /// A permanently-empty page: the sentinel every `Heap::direct` slot holds
    /// instead of null.
    ///
    /// This is upstream's `_mi_page_empty` trick. With null in the table, the
    /// malloc fast path needs TWO tests — "is there a page?" then "did it
    /// yield a block?". Pointing empty slots at a page whose free list is
    /// permanently null collapses both into the second one, because popping
    /// from the sentinel returns null and falls through to the generic path
    /// exactly as an exhausted real page does.
    ///
    /// `block_size`/`slice_count` are 1-ish rather than 0 purely so the
    /// `debug_checks` validator accepts it as a well-formed page.
    pub const fn empty_sentinel() -> Page {
        Page {
            free: ptr::null_mut(),
            local_free: ptr::null_mut(),
            xthread_free: AtomicUsize::new(0),
            xheap: AtomicUsize::new(0),
            next: ptr::null_mut(),
            prev: ptr::null_mut(),
            used: 0,
            capacity: 0,
            reserved: 0,
            block_size: 8,
            area: ptr::null_mut(),
            slice_count: 1,
            slice_offset: 0,
            bin: 0,
            flags: AtomicU8::new(0),
            free_is_zero: false,
            purged: false,
            heap_tag: 0,
            #[cfg(feature = "secure")]
            keys: [0; 2],
            // The sentinel's free list is permanently null, so nothing is ever
            // popped from or pushed to it and the map is never consulted.
            #[cfg(feature = "blockmap")]
            payload: ptr::null_mut(),
            #[cfg(feature = "blockmap")]
            bs_inv: 1,
        }
    }
}

/// Wrapper so the sentinel can be a `static`.
#[repr(transparent)]
pub struct EmptyPage(Page);

// SAFETY: the sentinel is never written. `page_pop` returns before its first
// store when `free` is null, and `free` is null permanently — nothing else ever
// receives this pointer, because a slot holding it is replaced by
// `update_direct` the moment the bin gains a real page.
unsafe impl Sync for EmptyPage {}

/// The one shared empty page (see [`Page::empty_sentinel`]).
/// In flash on a one-region target, for the same reason and under the same
/// contract as `init::EMPTY_HEAP_BOX`: its free list is null, so the fast
/// path never pops from it and nothing ever writes to it.
#[cfg_attr(
    all(
        ra_single_threaded,
        not(miri),
        not(windows),
        not(unix),
        not(target_arch = "wasm32")
    ),
    unsafe(link_section = ".rodata.rusty_alloc_empty_page")
)]
pub static EMPTY_PAGE: EmptyPage = EmptyPage(Page::empty_sentinel());

/// Pointer to the shared empty page, for `Heap::direct` slots with no page.
#[inline]
pub const fn empty_page_ptr() -> *mut Page {
    (&raw const EMPTY_PAGE.0).cast_mut()
}

/// Pop a block off the fast list. Returns null when dry (→ generic path).
///
/// # Safety
/// `page` must be a live page owned by the calling thread.
#[inline]
pub unsafe fn page_pop(page: *mut Page) -> *mut u8 {
    // SAFETY: caller already holds a valid page pointer (see fn contract).
    unsafe { debug_validate_page(page, "page_pop") };
    // SAFETY: owner-only field access per the contract.
    let block = unsafe { (*page).free };
    if block.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: a block on the free list is a valid, free block in this page's
    // area; its first word is the next link.
    unsafe {
        // BEFORE `block_next`, which dereferences `block` to read its link.
        // Order is the whole point: if a forged link steered us here, `block`
        // is an address of the attacker's choosing, and reading from it is a
        // segfault rather than a controlled abort. Validating first turns
        // "process died somewhere" into "the allocator refused". The
        // corruption tests assert on exactly that distinction, and they
        // caught this the first time it was written the other way round.
        //
        // A forged link fails here whatever produced it: recovered keys,
        // relative forgery, or blind luck.
        #[cfg(feature = "blockmap")]
        blockmap_transition(page, block.cast(), true);
        (*page).free = block_next(page, block);
        (*page).used += 1;
    }
    block.cast()
}

/// Link a dead block onto the page's owner-only free list, WITHOUT touching
/// `used`.
///
/// Split out of [`page_push_local`] so the caller can decrement `used` with a
/// single memory-destination RMW whose flags drive the retire branch — see
/// `alloc::free_inline`. The two halves are independent: the link is three
/// stores, the decrement is the count.
///
/// # Safety
/// As [`page_push_local`].
#[inline(always)]
pub unsafe fn page_link_local(page: *mut Page, block: *mut Block) {
    // SAFETY: forwarded contract — `block` belongs to `page` and is dead.
    unsafe {
        // Mark free BEFORE linking it in, so a double free is caught before
        // the block is published on a list twice.
        #[cfg(feature = "blockmap")]
        blockmap_transition(page, block.cast(), false);
        block_set_next(page, block, (*page).local_free);
        (*page).local_free = block;
    }
}

/// Byte offset of `Page::used`, for the one caller that decrements it in asm.
///
/// Asserted against the real layout in `tests` rather than assumed: an asm
/// operand cannot be type-checked, so the offset is the one thing here that a
/// field reordering could silently break.
pub const USED_OFFSET: usize = core::mem::offset_of!(Page, used);

/// Push a block on the owner free list (`mi_free` local path).
///
/// Returns the POST-decrement `used` count, and the caller MUST act on it:
/// a negative reading (as i32) is a double free — the count was already 0 and
/// wrapped — and the caller aborts via [`double_free_abort`]; 0 means the page
/// just emptied and is a retire candidate. Returning the value instead of
/// testing it here lets `alloc::free` fold BOTH cold outcomes into one
/// rarely-taken branch, and `#[must_use]` makes ignoring the double-free
/// signal a compile error. Every legitimate `used` is far below `i32::MAX` (a
/// span holds at most a few million blocks), so a negative value can only be
/// the wrap.
///
/// # Safety
/// `page` owned by the calling thread; `block` must be the start of a block
/// of this page, previously allocated and not yet freed.
#[inline]
#[must_use = "a negative return is a double free the caller must abort on"]
pub unsafe fn page_push_local(page: *mut Page, block: *mut Block) -> u32 {
    // SAFETY: caller already holds a valid page pointer (see fn contract).
    unsafe { debug_validate_page(page, "page_push_local") };
    // SAFETY: caller's contract — block belongs to page and is dead; writing
    // its first word as the link is the free-list representation.
    unsafe {
        // Mark free BEFORE linking it in, so a double free is caught before
        // the block is published on a list twice.
        // The blockmap transition lives in `page_link_local`; do NOT repeat
        // it here. Flipping the liveness bit twice makes the second flip read
        // as "already free" and abort as a double free — which is exactly what
        // happened when this function was split, and what `tests/abandon_rss`
        // caught.
        page_link_local(page, block);
        let u = (*page).used.wrapping_sub(1);
        (*page).used = u;
        // NOTE (2026-08-20, RE-REFUTED 2026-08-21): this decrement is where
        // the gap to mimalloc lives on every free-heavy workload — sh6bench
        // +366M Ir, sh8bench +852M, alloc-test +200M — and it is a safe-Rust
        // CODEGEN FLOOR, not a fixable structure. mimalloc's free emits `subw $1, used; je` — one
        // memory-destination RMW whose flags feed the retire branch (2
        // instructions). We emit load / dec / store / test / branch (5),
        // because the decremented value must be in memory before the branch
        // (the retire tail re-reads `used`) AND drive the branch, and LLVM
        // will not select `dec [mem]; jle` for that shape. Splitting the push
        // from the decrement and inlining the latter next to the branch was
        // tried and produced BYTE-IDENTICAL asm — see docs/opps.md #6.
        //
        // RE-REFUTED 2026-08-21, harder. opps #6 split the push from the
        // decrement; it never removed the REGISTER dependency, which is the
        // stated reason LLVM declines the fold. So that was tried too: the
        // return value was deleted, the decrement written straight to memory,
        // and the caller made to re-read the same field — the exact shape
        // Clang folds for mimalloc. Result on sh6bench, whose allocator count
        // is exact to the unit: **8,005,465,906 before and after**, not one
        // instruction different. The fold is not reachable from safe Rust by
        // any arrangement of this code, and the `#[must_use]` return is worth
        // keeping for the compile-time double-free guard it provides.
        u
    }
}

/// A double free was detected on a [`page_push_local`] return value.
///
/// Aborts rather than returning. Continuing would publish the same block on a
/// free list twice and hand it to two owners — the exact class of bug this
/// allocator exists to make impossible. Aborting keeps the damage local and
/// the failure attributable, and an allocator must not unwind into its C
/// callers in any case (the release profile is `panic = "abort"` for that
/// reason).
#[cold]
#[inline(never)]
pub(crate) fn double_free_abort() -> ! {
    crate::abort()
}

/// A corrupted free-list link was detected on decode (`secure` builds).
///
/// Aborts, and deliberately says nothing on the way out. The obvious thing —
/// `assert!` with a message, which is what this path used to do — is wrong
/// twice over for a fault detected INSIDE the allocator: formatting a panic
/// message can allocate, re-entering the very allocator that just found its
/// own metadata corrupted, and the unwind would cross `extern "C"` frames on
/// the FFI path. A silent abort has neither failure mode, and it keeps the
/// crate's "no logging outside the bench CLI" property (hardening gate H-20)
/// intact. The signal is the diagnostic; `tests/corruption.rs` reads it.
#[cold]
#[inline(never)]
#[cfg(any(feature = "secure", feature = "linkcheck"))]
pub(crate) fn corrupt_free_list_abort() -> ! {
    crate::abort()
}

/// Remote (non-owner) free — the loom-modeled protocol.
///
/// # Safety
/// `page` must be a live page NOT owned by the calling thread; `block` a dead
/// block of this page.
// NOT `#[inline(never)]`. Outlining this is tempting — inlined, its spin loop
// is what gives `free_general` an eight-instruction frame that every general
// free pays. Measured, it is a LOSS: `free_general` −1,495,040 but
// `remote_free` +1,868,800, net +373,760 on the cross-thread op, because a
// workload where this function matters is one where every free reaches it, so
// the call is paid every time AND the callee grows a frame of its own.
pub unsafe fn remote_free(page: *mut Page, block: *mut Block) {
    // Unreachable on a single-context build (`alloc::free` folds every free
    // to local there); if it runs anyway, the single-thread assertion the
    // target made was false.
    if crate::ONE_THREAD {
        unreachable!("a cross-thread free on a build that asserted a single thread");
    }
    // Both arms below detect a double free with one compare and `break` to
    // the single abort AFTER the loop; see `remote_double_free` for why that
    // abort is a tail call and not a diverging one.
    'push: loop {
        // SAFETY: xthread_free/xheap are the designed cross-thread fields.
        let x = unsafe { (*page).xthread_free.load(Ordering::Acquire) };
        match x & XMASK {
            XFLAG_DELAYED => {
                // Claim the transient FREEING state before touching the heap.
                // SAFETY: atomic field.
                let claimed = unsafe {
                    (*page)
                        .xthread_free
                        .compare_exchange_weak(
                            x,
                            (x & !XMASK) | XFLAG_FREEING,
                            Ordering::AcqRel,
                            Ordering::Relaxed,
                        )
                        .is_ok()
                };
                if claimed {
                    // SAFETY: while FREEING is held the abandoner cannot tear
                    // the heap down (it spins us out first) — xheap is valid.
                    unsafe {
                        let dl = (*page).xheap.load(Ordering::Acquire) as *const DelayedList;
                        debug_assert!(!dl.is_null(), "DELAYED page without an owner heap");
                        loop {
                            let head = (*dl).head.load(Ordering::Acquire);
                            // A block that is already the list head is being
                            // freed a second time: abort, do not link it to
                            // itself (OH-rusty_alloc-11).
                            if head == block as usize {
                                break 'push;
                            }
                            // Delayed-list links are heap-scoped: encoding
                            // them would need the owner's page keys here, so
                            // they stay plain even in secure builds.
                            (*block).next = crate::ptr_with_addr(block, head);
                            if (*dl)
                                .head
                                .compare_exchange_weak(
                                    head,
                                    block as usize,
                                    Ordering::AcqRel,
                                    Ordering::Relaxed,
                                )
                                .is_ok()
                            {
                                break;
                            }
                        }
                        // Release FREEING, preserving whatever the owner did
                        // to the pointer bits meanwhile — to NORMAL, as
                        // upstream does (`mi_tf_set_delayed(.., MI_NO_DELAYED_FREE)`
                        // in `_mi_free_block_mt`), not back to DELAYED.
                        //
                        // One block on the owner's delayed list is all the
                        // owner needs: draining it un-parks the page
                        // (`free_local_at`), and from then on the page is
                        // scanned. Restoring DELAYED sent EVERY later remote
                        // free to a parked page through this three-CAS route
                        // and the owner through `drain_delayed` +
                        // `free_local_at` per block — 82 % of the frees on the
                        // `xthread` op. After NORMAL they are one CAS onto the
                        // page's own list, collected in bulk.
                        //
                        // SINGLE-BLOCK pages (large spans, huge segments) keep
                        // DELAYED: they are never scanned, so their only route
                        // to the owner is this list.
                        let restore =
                            if (*page).flags.load(Ordering::Relaxed) & pflags::SINGLE_BLOCK != 0 {
                                XFLAG_DELAYED
                            } else {
                                XFLAG_NORMAL
                            };
                        loop {
                            let y = (*page).xthread_free.load(Ordering::Acquire);
                            if (*page)
                                .xthread_free
                                .compare_exchange_weak(
                                    y,
                                    (y & !XMASK) | restore,
                                    Ordering::AcqRel,
                                    Ordering::Relaxed,
                                )
                                .is_ok()
                            {
                                break;
                            }
                        }
                    }
                    return;
                }
            }
            XFLAG_FREEING => core::hint::spin_loop(),
            flag => {
                // NORMAL or NEVER: push onto the page's own list.
                //
                // A block that is already the chain head is being freed a
                // second time. Linking it would make the chain CYCLIC
                // (`block -> block`), which the owner's collect could never
                // finish walking; on a NEVER page there is no owner at all,
                // so the cycle sat there until a reclaim hung on it. One
                // compare against a value already in a register, on the
                // cross-thread path only (OH-rusty_alloc-11).
                if (x & !XMASK) == block as usize {
                    break 'push;
                }
                // SAFETY: block is dead memory we own; link write is the
                // free-list representation.
                unsafe {
                    block_set_next(page, block, crate::ptr_with_addr(block, x & !XMASK));
                    if (*page)
                        .xthread_free
                        .compare_exchange_weak(
                            x,
                            (block as usize) | flag,
                            Ordering::Release,
                            Ordering::Relaxed,
                        )
                        .is_ok()
                    {
                        return;
                    }
                }
            }
        }
    }
    remote_double_free()
}

/// A double free was detected on the cross-thread path ([`remote_free`]).
///
/// This is deliberately NOT `-> !`, and the `black_box` is load-bearing.
/// `remote_free` is inlined into `alloc::free`, a leaf with no stack frame.
/// A callee LLVM can prove diverging is emitted as `call; ud2`, and on
/// Windows x64 that one `call` pins a frame to the TOP of `free` (SEH unwind
/// info cannot describe a prologue anywhere else, so shrink-wrapping is off)
/// — two instructions on every LOCAL free, for a check that only runs on the
/// remote arm. A callee that may return, in tail position, is a `jmp`: the
/// same shape as `alloc::retire_or_abort`, and the local path stays
/// byte-identical to the build without the check. `black_box(true)` is what
/// keeps the "may return" path visible to attribute inference; it is always
/// true, and the function never returns in practice.
#[cold]
#[inline(never)]
fn remote_double_free() {
    if core::hint::black_box(true) {
        double_free_abort();
    }
}

/// Owner/abandoner flag transition, spinning out any in-flight FREEING.
///
/// # Safety
/// Only the page's owner (or the abandoner during teardown, or the adopter
/// after taking ownership) may call this.
pub unsafe fn page_set_flag(page: *mut Page, flag: usize) {
    loop {
        // SAFETY: atomic field.
        let x = unsafe { (*page).xthread_free.load(Ordering::Acquire) };
        if x & XMASK == XFLAG_FREEING {
            core::hint::spin_loop();
            continue;
        }
        // SAFETY: atomic field.
        let ok = unsafe {
            (*page)
                .xthread_free
                .compare_exchange_weak(x, (x & !XMASK) | flag, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
        };
        if ok {
            return;
        }
    }
}

/// Collect: swap `local_free` and steal the xthread list (flag preserved)
/// into `free`. Called on the slow path when `free` is dry — the heartbeat.
///
/// # Safety
/// `page` owned by the calling thread.
pub unsafe fn page_collect(page: *mut Page) -> bool {
    // SAFETY: forwarded contract; PRESERVE the protocol flag.
    unsafe { page_collect_impl::<false>(page, 0) }
}

/// Collect AND install a new protocol flag in ONE locked read-modify-write.
///
/// The abandon/adopt and teardown walks both used to do
/// `page_set_flag(p, F); page_collect(p)` — two CAS loops over the SAME atomic
/// word, on every live page of every segment they touch. The composition of
/// those two operations is exactly "take the block-list head and set the flag
/// to F", which is one CAS. `page_set_flag`'s spin-while-`XFLAG_FREEING` is
/// kept, so a remote free in flight is still waited out before the word is
/// rewritten — the fused form is atomically at least as strong as the pair it
/// replaces, because the flag and the steal can no longer be observed apart.
///
/// # Safety
/// As [`page_collect`].
pub unsafe fn page_collect_and_set_flag(page: *mut Page, flag: usize) {
    // SAFETY: forwarded contract.
    let _stole = unsafe { page_collect_impl::<true>(page, flag) };
}

/// The body of both. `SET_FLAG` is a const parameter so neither caller pays a
/// runtime branch for the other's behaviour.
///
/// # Safety
/// As [`page_collect`].
#[inline]
unsafe fn page_collect_impl<const SET_FLAG: bool>(page: *mut Page, flag: usize) -> bool {
    // SAFETY: owner-only lists plus designed atomic steal.
    unsafe {
        if (*page).free.is_null() {
            (*page).free = (*page).local_free;
            (*page).local_free = ptr::null_mut();
            if !(*page).free.is_null() {
                // Recycled blocks are not zero.
                (*page).free_is_zero = false;
            }
        }
        // Peek ONCE before entering the exchange loop. A plain collect (no
        // flag to write) has nothing to do when the chain is empty, which is
        // every collect on a thread that never receives a cross-thread free —
        // and `malloc_generic` collects the queue front on every slow-path
        // allocation. Entering the loop to discover that costs the loop's
        // setup as well as the test; this is the test alone, straight-line.
        // Steal the cross-thread chain, preserving the protocol flag — or,
        // when SET_FLAG, replacing it in the same CAS.
        if !SET_FLAG && ((*page).xthread_free.load(Ordering::Acquire) & !XMASK) == 0 {
            return false;
        }
        loop {
            let x = (*page).xthread_free.load(Ordering::Acquire);
            if SET_FLAG && x & XMASK == XFLAG_FREEING {
                // A remote free is mid-flight; the flag must not be rewritten
                // underneath it (this is `page_set_flag`'s wait, inherited).
                core::hint::spin_loop();
                continue;
            }
            let head = (x & !XMASK) as *mut Block;
            if head.is_null() {
                if SET_FLAG
                    && (*page)
                        .xthread_free
                        .compare_exchange_weak(x, flag, Ordering::AcqRel, Ordering::Relaxed)
                        .is_err()
                {
                    continue;
                }
                return false;
            }
            let want = if SET_FLAG { flag } else { x & XMASK };
            if (*page)
                .xthread_free
                .compare_exchange_weak(x, want, Ordering::AcqRel, Ordering::Relaxed)
                .is_err()
            {
                continue;
            }
            (*page).free_is_zero = false;
            // Append the stolen chain, counting its length against `used`.
            //
            // This is also where REMOTELY freed blocks get their liveness bit
            // cleared. Doing it here rather than in `remote_free` is what
            // keeps the map owner-only and therefore free of atomics: the
            // freeing thread only pushes onto `xthread_free`, and the owner
            // reconciles the map when it drains that list. The cost is a
            // window — a block freed remotely still reads as live until the
            // next collect — which is acceptable because the map exists to
            // catch a forged link landing on a LIVE block, and a
            // recently-freed one reading as live is the conservative
            // direction.
            //
            // One `block_next` per element, not two: the original loop called
            // it in the condition AND the body, doubling the decode (and, in
            // `secure`, the bound check) on every element of the walk.
            //
            // The CROSS-THREAD arm of the double-free check `page_push_local`
            // performs, and the walk is BOUNDED by it. A block freed twice
            // from another thread is linked onto `xthread_free` twice, and
            // its second link points back into the chain it is already on:
            // the chain is CYCLIC, so an unbounded walk here never returns —
            // the post-walk `n > used` test this used to rely on was
            // unreachable for the very case it named. No legitimate chain is
            // longer than the number of live blocks, so `n > used` inside the
            // walk turns the hang into the same abort the local path gives.
            // `remote_free` refuses the consecutive case at push time; this
            // catches the interleaved one (A, B, A) — OH-rusty_alloc-11.
            //
            // One register compare per drained element, on the heartbeat,
            // not on any per-free path.
            let used = (*page).used;
            let mut tail = head;
            let mut n = 1u32;
            loop {
                // NOTE (2026-09-24, REFUTED): calling the may-return
                // `remote_double_free` here instead — to stop this diverging
                // call pinning an alignment `push` to the top of
                // `malloc_generic_once`'s frameless medium arm — made it far
                // WORSE: a call that may return, inside the walk loop, keeps
                // `tail`/`n`/`page` live across it, and the entry grew a
                // six-register frame (opscan `huge` +20.00, every op worse).
                // The may-return trick works only in TAIL position.
                if n > used {
                    double_free_abort();
                }
                #[cfg(feature = "blockmap")]
                blockmap_transition(page, tail.cast(), false);
                let nxt = block_next(page, tail);
                if nxt.is_null() {
                    break;
                }
                tail = nxt;
                n += 1;
            }
            block_set_next(page, tail, (*page).free);
            (*page).free = head;
            // `n <= used` here: the walk aborted otherwise.
            (*page).used = used - n;
            break;
        }
        // Reached only by breaking out of the steal arm above, i.e. a
        // cross-thread chain was actually taken.
        true
    }
}

/// The constant term of [`page_extend`]'s batch shift: `SEGMENT_SLICE_SIZE`
/// expressed in 4 KiB pages, as a shift. 4 at the shipped 64 KiB slice, 0 at
/// the small profile's 4 KiB one. See the note in `page_extend`.
const EXTEND_SHIFT_BASE: u32 = {
    assert!(
        crate::types::SEGMENT_SLICE_SIZE >= 4096,
        "the extend bound assumes a slice of at least one 4 KiB page"
    );
    crate::types::SEGMENT_SLICE_SIZE.trailing_zeros() - 12
};

/// The fewest blocks one [`page_extend`] links, whatever the block size —
/// upstream's `MI_MIN_EXTEND` (4 outside its secure mode).
///
/// Ours was an implicit 1 (`.max(1)`), a drift from upstream: every class
/// whose 4 KiB byte bound rounds below four blocks — everything above 1 KiB —
/// was carved one to three blocks per extend, so on perl 8,990 of 10,639
/// extends linked one block and each was a full generic trip (callgrind, per
/// instruction). The floor only moves list-linking earlier inside a span
/// that is already committed; it touches at most four block headers.
const MIN_EXTEND: usize = 4;

/// Lazily extend the free list into never-used capacity (`mi_page_extend_free`).
///
/// # Safety
/// `page` owned by the calling thread; `area` must be the page's payload
/// start, valid committed memory of `reserved * block_size` bytes.
pub unsafe fn page_extend(page: *mut Page, area: *mut u8) {
    // SAFETY: caller already holds a valid page pointer (see fn contract).
    unsafe { debug_validate_page(page, "page_extend") };
    // SAFETY: heap lock held; arithmetic stays inside the page area by the
    // capacity <= reserved invariant.
    unsafe {
        let bsize = (*page).block_size;
        let capacity = (*page).capacity as usize;
        let reserved = (*page).reserved as usize;
        if capacity >= reserved {
            return;
        }
        // First carve of this page's life: place the liveness map AFTER the
        // blocks. `reserved` was already reduced in `page_fresh` to leave room
        // for it, so this stays inside the span.
        //
        // The front would be the better place on security grounds — a forward
        // overflow would run away from the map instead of into it — but the
        // payload start is not ours to move. `page_area` has eight consumers
        // (the interior-pointer adjustment in `alloc::free`, the large-object
        // path, `visit_blocks`, segment reclaim…) and offsetting the payload
        // here made exactly ONE of them agree, desynchronising the rest; the
        // `aligned_at_offsets` test aborted on the first run because free
        // derived a block start the allocator had never handed out. Keeping
        // `area` meaning what it has always meant is worth more than the
        // placement.
        //
        // Zeroing is explicit: a RECLAIMED span is recycled memory and carries
        // whatever the previous tenant left in it.
        #[cfg(feature = "blockmap")]
        if capacity == 0 {
            (*page).payload = area;
            core::ptr::write_bytes(area.add(reserved * bsize), 0, bitmap_bytes(reserved));
        }
        // Blocks linked per extend. The bound is in BYTES of payload, so a
        // small class links many blocks and a large one few — mimalloc uses
        // the same shape with a 4 KiB bound ("one OS page seems to work
        // well"). Raising it to 8 KiB halves how often a draining class has to
        // re-enter the whole slow path — the queue walk, the heartbeat, the
        // direct-table update — without changing how much memory is touched:
        // the page's area is already committed when it is carved, so this
        // moves link-list work earlier, not memory.
        //
        // REFUTED 2026-08-21 — raising this bound is an INSTRUCTION win and a
        // CACHE loss, and the cache side is the one that decides.
        //
        // Doubling to 8 KiB reads −0.16 to −8.39 Ir/op across ten of twelve
        // scanned ops (realloc −8.39, aligned −5.75, med −2.85) because a
        // draining size class re-enters the slow path half as often.
        // Quadrupling to 16 KiB reads better still on most of them. But each
        // extend WALKS its whole batch, and cachegrind on a shbench-shaped
        // workload prices that walk:
        //
        //     bound    I refs        D1 misses      LL misses
        //     4 KiB    11,567,403    144,514        20,750
        //     8 KiB    11,542,248    146,106 +1.1%  21,207 +2.2%
        //     16 KiB   11,544,360    149,413 +3.4%  22,079 +6.4%
        //
        // 8 KiB buys 25,155 instructions for 457 extra last-level misses. At a
        // few hundred cycles each that is ~114k cycles spent to save perhaps
        // 13k. This repository measures instructions BECAUSE its clock cannot
        // resolve small effects — not because instructions are the goal — and
        // the README says so directly: "fewer instructions is not
        // automatically less TIME (cache behaviour and syscalls do not show up
        // here)". A change with a measured cost in the blind spot and an
        // unverifiable gain in the domain that matters does not land.
        //
        // Upstream's own comment on the same 4 KiB bound: "one OS page seems
        // to work well". It is one OS page for a reason.
        // RESOLVED (2026-08-22), after two refutations worth keeping.
        //
        // `4096 / bsize` was a real `div` by a runtime value, emitted twice
        // into `malloc_generic_walk` because this function inlines at both of
        // its call sites. Two removals were built and both LOST: a
        // compile-time per-bin batch table at **+29,457 Ir on cfrac**, and
        // `bins::div_by_block_size` at **+48,605** measured together with the
        // `fresh_page` substitution. The reason is where this function
        // inlines — heap.rs:497 is `malloc_generic`'s FAST path, taken on
        // every slow-path allocation, while the `div` sits on the grow arm
        // that fires on 0.1% of them. Both kept the *computation* and only
        // changed its form, so both moved a live value onto the common path.
        //
        // This removes the computation instead. `reserved` is
        // `(slice_count * SEGMENT_SLICE_SIZE) / bsize`, so `4096 / bsize` is
        // exactly `reserved / (slice_count * 16)`. Every span that can reach
        // here is 1 or `MEDIUM_PAGE_SLICES` (= 8) slices — the huge path sets
        // `reserved = 1` and never extends — so the divisor is a power of two
        // and the bound is a shift of `reserved`, a value the `min` on that
        // same line already holds live. Nothing arrives to replace what
        // leaves, which is why this is the only one of the three that made
        // cfrac faster (-1,500 Ir).
        debug_assert!(
            (*page).slice_count.is_power_of_two(),
            "extend bound assumes a power-of-two span, got {}",
            (*page).slice_count
        );
        // The `16` in that identity is `SEGMENT_SLICE_SIZE / 4096`, so the
        // shift's constant term is the GEOMETRY's, not a literal 4.
        //
        // DEFECT (found 2026-09-10, `docs/plans/finished/fixed-prim-small-step.md`
        // §8.7): it was written as a literal `4`, which is right only for the
        // shipped 64 KiB slice. Under `ra_small_profile` the slice is 4 KiB, so
        // the bound this computes was 256 BYTES of payload rather than 4 KiB --
        // sixteen times too small. For a 512-byte class `reserved >> shift` is
        // then 0, `.max(1)` rescues it to ONE BLOCK, and every page on the
        // profile firmware actually uses was extended one block at a time:
        // `capacity == 1` on every page, and `malloc_generic` on 100 % of
        // allocations instead of one in eight. Measured on a host at the small
        // profile before the fix: `generic` exactly 1.0000/op at 512, 640 and
        // 1024 bytes.
        //
        // Derived, so it is correct at every geometry and byte-identical at the
        // default (65536 >> 12 == 16, whose log2 is the old 4).
        let span_shift = EXTEND_SHIFT_BASE + (*page).slice_count.trailing_zeros();
        // At least `MIN_EXTEND` blocks, as upstream (`MI_MIN_EXTEND`, 4): the
        // byte bound alone gives ONE block to any class above 4 KiB and one
        // to three to classes above 1 KiB, so a fresh medium page served one
        // allocation per slow-path trip. See `MIN_EXTEND`.
        let take = ((reserved >> span_shift).max(MIN_EXTEND)).min(reserved - capacity);
        let start = area.add(capacity * bsize);
        // Link the fresh blocks in address order.
        //
        // Two rewrites of this loop have been measured and both lost. Walking
        // BACKWARDS with a running pointer, to replace `start.add(i * bsize)`'s
        // multiply, read FLAT — LLVM already strength-reduces the index.
        // Walking FORWARD, storing `b + bsize` into `b` to drop the carried
        // `head`, read **+0.33 Ir/op** on a shbench-shaped workload: the
        // forward form makes each store depend on the previous address
        // computation, where the backward form's decrementing induction
        // variable does not. Left as it is.
        let mut i = take;
        let mut head: *mut Block = (*page).free;
        while i > 0 {
            i -= 1;
            let b: *mut Block = start.add(i * bsize).cast();
            block_set_next(page, b, head);
            head = b;
        }
        (*page).free = head;
        (*page).capacity = (capacity + take) as u32;
    }
}

/// Whether every block of the page is free (as seen by the owner; remote
/// frees count only after a collect).
///
/// # Safety
/// `page` owned by the calling thread.
#[inline]
pub unsafe fn page_all_free(page: *mut Page) -> bool {
    // SAFETY: owner-only field.
    unsafe { (*page).used == 0 }
}

/// The address→index arithmetic the `blockmap` feature rests on, checked
/// against EVERY REAL BIN SIZE rather than a few hand-picked ones.
///
/// This is the piece most likely to be silently wrong: a division replaced by
/// a multiply is correct only under a precondition (exact multiples), and a
/// wrong index writes the wrong bit — which would either miss real
/// double-allocations or abort on legitimate ones. Pinning it against
/// `bin_size` for the whole bin range, at every block offset, is cheap and
/// leaves nothing to trust.
#[cfg(all(test, feature = "blockmap"))]
mod blockmap_index_tests {
    use super::*;
    use crate::bins::bin_size;

    /// `(delta >> k) * inv(odd)` must equal `delta / block_size` for every bin
    /// size and every block position in a 64 KiB page.
    #[test]
    fn index_matches_real_division_for_every_bin() {
        for bin in 1..=40usize {
            let bs = bin_size(bin);
            if bs == 0 {
                continue;
            }
            let k = bs.trailing_zeros();
            let odd = bs >> k;
            let inv = odd_mod_inverse(odd);
            let blocks = (64 * 1024 / bs).min(4096);
            for idx in 0..blocks {
                let delta = idx * bs;
                let got = (delta >> k).wrapping_mul(inv);
                assert_eq!(
                    got, idx,
                    "bin {bin} (block_size {bs}): delta {delta} gave index {got}, want {idx}"
                );
            }
        }
    }

    /// An interior pointer must map to the CONTAINING block or to something
    /// the capacity bound rejects — never to a different, plausible block.
    ///
    /// That middle outcome is the dangerous one: attributing liveness to the
    /// wrong block would both miss real double-allocations and abort on
    /// legitimate ones. The two safe outcomes are fine for opposite reasons —
    /// the containing index is the RIGHT answer for a liveness map, and a
    /// garbage index fails `idx < capacity`.
    #[test]
    fn interior_pointers_map_to_the_containing_block_or_out_of_range() {
        const CAP: usize = 4096;
        let mut saw_containing = 0;
        let mut saw_garbage = 0;
        for bin in 2..=40usize {
            let bs = bin_size(bin);
            if bs <= 1 {
                continue;
            }
            let k = bs.trailing_zeros();
            let odd = bs >> k;
            let inv = odd_mod_inverse(odd);
            let base = 4usize;
            for off in 1..bs.min(64) {
                let delta = base * bs + off;
                let got = (delta >> k).wrapping_mul(inv);
                if got == base {
                    saw_containing += 1;
                } else if got >= CAP {
                    saw_garbage += 1;
                } else {
                    panic!(
                        "bin {bin} (block_size {bs}) offset {off}: interior pointer mapped to \
                         index {got} — neither the containing block ({base}) nor out of range"
                    );
                }
            }
        }
        // Both arms must actually occur, or the test is only exercising one.
        assert!(saw_containing > 0, "no containing-block cases covered");
        assert!(saw_garbage > 0, "no out-of-range cases covered");
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    /// `alloc::free_inline` decrements `Page::used` from INLINE ASSEMBLY, as
    /// `sub dword ptr [pg + USED_OFFSET], 1`, because safe Rust cannot express
    /// a memory-destination RMW whose flags drive the retire branch. An asm
    /// operand is not type-checked: if the field moved, or changed width, the
    /// allocator would silently decrement the wrong bytes of every page it
    /// frees into. This is the check that stops that.
    #[test]
    fn used_offset_matches_the_field_the_asm_decrements() {
        assert_eq!(
            USED_OFFSET,
            core::mem::offset_of!(Page, used),
            "USED_OFFSET is out of step with Page::used"
        );
    }

    /// The asm writes a DWORD. If `used` ever stops being a `u32` this stops
    /// compiling, which is the earliest possible place to catch it — a
    /// narrower field would leave the high bytes of a neighbour decremented,
    /// a wider one would tear the count.
    const _WIDTH_CHECK: fn(&Page) -> u32 = |p| p.used;
}

#[cfg(test)]
mod link_tests {
    use super::*;
    use crate::types::SEGMENT_SIZE;

    /// A synthetic SEGMENT_SIZE-aligned base. No memory is touched and no heap
    /// is built: the predicate is pure arithmetic on addresses, which is
    /// precisely why it can be pinned this exactly. The end-to-end proof that
    /// a bad link actually ABORTS lives in `tests/corruption.rs`.
    const BASE: usize = 0x0000_4000_0000_0000;
    /// An offset well inside a segment at ANY geometry.
    ///
    /// Derived rather than the literal 1 MiB it used to be: the predicate
    /// under test is scoped to `SEGMENT_SIZE`, so a fixed offset silently
    /// stops testing the inside of the segment the moment that constant
    /// moves — under the small profile (P2, `docs/plans/small-metal.md`) a
    /// 64 KiB segment does not contain a 1 MiB offset at all, and three of
    /// these assertions inverted. `/8` keeps the +-4096 probes below inside
    /// the segment at every geometry this crate builds.
    const OFF: usize = SEGMENT_SIZE / 8;
    /// A block sitting `OFF` bytes into that segment.
    const B: usize = BASE + OFF;

    #[test]
    fn accepts_genuine_links_anywhere_in_the_same_segment() {
        assert!(link_is_plausible(BASE, B), "segment's first block");
        assert!(link_is_plausible(B, B), "self-link");
        assert!(link_is_plausible(B + 16, B), "next block");
        assert!(link_is_plausible(B - 16, B), "previous block");
        assert!(link_is_plausible(B + 4096, B), "forward link");
        assert!(link_is_plausible(B - 4096, B), "backward link");
        assert!(
            link_is_plausible(BASE + SEGMENT_SIZE - 16, B),
            "last aligned slot in the segment"
        );
    }

    #[test]
    fn rejects_every_misalignment() {
        for off in 1..8usize {
            assert!(!link_is_plausible(B + off, B), "misaligned by {off}");
        }
    }

    /// A deliberate attacker picks an ALIGNED target, because every address
    /// worth steering an allocator at — GOT entries, vtables, function
    /// pointers, saved return addresses — already is one. Alignment alone
    /// filters accidents, not adversaries; the segment bound is what puts
    /// those targets out of reach.
    #[test]
    fn rejects_aligned_targets_outside_the_segment() {
        assert!(
            !link_is_plausible(BASE + SEGMENT_SIZE, B),
            "first byte of the NEXT segment"
        );
        assert!(
            !link_is_plausible(BASE - 16, B),
            "last slot of the PREVIOUS segment"
        );
        assert!(
            !link_is_plausible(0x0000_7fff_ffff_e000, B),
            "a stack-shaped address"
        );
        assert!(
            !link_is_plausible(0x0000_0000_0040_1000, B),
            "a GOT-shaped address"
        );
    }

    /// The bound must be exact on BOTH sides. Too tight and valid links abort
    /// in normal operation (a crash we would ship); too loose and the first
    /// slot of the neighbouring segment is reachable.
    #[test]
    fn the_segment_bound_is_exact() {
        assert!(link_is_plausible(BASE + SEGMENT_SIZE - 16, B));
        assert!(!link_is_plausible(BASE + SEGMENT_SIZE, B));
        assert!(link_is_plausible(BASE, B));
        assert!(!link_is_plausible(BASE - 16, B));
    }

    /// What this bound deliberately does NOT stop, pinned so nobody mistakes
    /// its scope: a target in the SAME segment — including the neighbouring
    /// block, which is exactly R-005's relative forgery — passes.
    ///
    /// A page-scoped bound was built and measured for this and cost +5 Ir/op
    /// while still not closing the intra-PAGE case; the only thing that does
    /// is the `blockmap` liveness map, at +58 Ir/op and off by default. If
    /// this test ever goes red the bound has been tightened, and its cost
    /// needs re-measuring before anyone celebrates.
    #[test]
    fn does_not_stop_intra_segment_redirection() {
        assert!(
            link_is_plausible(B + 16, B),
            "the neighbouring block is reachable by design — see R-005"
        );
        assert!(
            link_is_plausible(B + OFF, B),
            "far away in bytes, still the same segment"
        );
    }
}
