//! The heap: per-bin page queues, the small-size direct table, the generic
//! (heartbeat) allocation path, and free (mirrors `heap.c` + the hot parts of
//! `alloc.c`/`page.c`). Since M4 there is NO global lock: every thread owns its
//! own heap, `free` routes by the segment's owner id, and cross-thread frees go
//! through the loom-modeled 4-state protocol in `page.rs`.

use core::cell::Cell;
use core::ptr;
use core::sync::atomic::Ordering;

ra_thread_local! {
    /// Set while `Heap::malloc_generic_retry` runs its one reclaim-and-retry,
    /// so a second null from the nested pass ends the attempt. Read only on
    /// the OOM path; see `Heap::malloc_generic` for why it is not a parameter.
    static IN_OOM_RETRY: Cell<bool> = const { Cell::new(false) };
}

use crate::bins::{self, BIN_COUNT, PAGES_DIRECT};
use crate::page::{
    Block, DelayedList, Page, XFLAG_DELAYED, XFLAG_NORMAL, block_next, page_all_free, page_collect,
    page_collect_and_set_flag, page_extend, page_pop, page_push_local, page_set_flag, pflags,
};
use crate::segment::{
    self, Segment, SegmentKind, huge_free, page_area, segment_of, span_alloc, span_free,
};
use crate::types::{
    BIN_FULL, BIN_HUGE, LARGE_OBJ_SIZE_MAX, MEDIUM_OBJ_SIZE_MAX, MEDIUM_PAGE_SLICES,
    SEGMENT_SLICE_SIZE, SMALL_OBJ_SIZE_MAX, SMALL_SIZE_MAX, SMALL_WSIZE_MAX, wsize_from_size,
};

/// Largest bin index actually reachable from a size (rest are M3 large pages).
pub const MAX_NORMAL_BIN: usize = bins_max();

/// Largest bin whose block size still fits the `direct[]` fast-path table.
///
/// `update_direct` returns immediately for anything above this, so the
/// wholesale rebuild in `adopt_segment` was calling it MAX_NORMAL_BIN times to
/// do nothing for the upper half of the range. Const-evaluated from the same
/// `bin_size` the check itself uses, so the two cannot drift apart.
pub const MAX_DIRECT_BIN: usize = {
    let mut b = 1;
    while b < MAX_NORMAL_BIN && bins::bin_size(b + 1) <= SMALL_SIZE_MAX {
        b += 1;
    }
    b
};

const fn bins_max() -> usize {
    // const-eval of bins::bin(MEDIUM_OBJ_SIZE_MAX)
    let w = wsize_from_size(MEDIUM_OBJ_SIZE_MAX) - 1;
    let b = (usize::BITS - 1 - w.leading_zeros()) as usize;
    ((b << 2) + ((w >> (b - 2)) & 0x03)) - 3
}

/// A doubly-linked queue of pages serving one bin.
#[derive(Clone, Copy)]
pub struct PageQueue {
    /// Front page — the one the fast path allocates from.
    pub first: *mut Page,
    /// Back page.
    pub last: *mut Page,
    /// Block size of every page in this queue.
    pub block_size: usize,
}

/// Always-on counters (plan §7.5 instrument #2): the primary evidence for
/// sub-1% bricks and the work-parity check for every A/B.
#[derive(Clone, Copy, Default)]
pub struct Stats {
    /// Successful allocations.
    pub allocs: u64,
    /// Frees.
    pub frees: u64,
    /// Entries into the generic (slow) path.
    pub generic: u64,
    /// Pages freshly carved.
    pub pages_fresh: u64,
    /// Normal segments reserved from the OS.
    pub segments: u64,
    /// Huge (dedicated-segment) allocations.
    pub huge_allocs: u64,
    /// Free-list extensions performed.
    pub extends: u64,
    /// Large (in-segment span) allocations.
    pub large_allocs: u64,
    /// Pages retired (span returned to the segment).
    pub pages_retired: u64,
    /// Empty segments returned to the OS.
    pub segments_freed: u64,
    /// Reallocs resolved in place (no move).
    pub realloc_in_place: u64,
    /// Reallocs that moved the block.
    pub realloc_moved: u64,
    /// Blocks processed off the delayed (cross-thread) list.
    pub delayed_frees: u64,
    /// Abandoned segments adopted from dead threads.
    pub reclaims: u64,
    /// Guarded objects handed out (secure/guarded builds).
    pub guarded: u64,
    /// Page-sized ranges purged (decommitted/reset) back to the OS.
    pub purges: u64,
}

impl Stats {
    /// Const-init zero stats.
    pub const fn new() -> Stats {
        Stats {
            allocs: 0,
            frees: 0,
            generic: 0,
            pages_fresh: 0,
            segments: 0,
            huge_allocs: 0,
            extends: 0,
            large_allocs: 0,
            pages_retired: 0,
            segments_freed: 0,
            realloc_in_place: 0,
            realloc_moved: 0,
            delayed_frees: 0,
            reclaims: 0,
            guarded: 0,
            purges: 0,
        }
    }
}

/// Immortal, permanently-empty delayed-free list.
///
/// `Heap::default()` used to leave `delayed` NULL, so the allocation heartbeat
/// had to test for null on every slow-path allocation before it could even
/// peek at the list — three instructions, 2.56 M times on `cfrac`, to rule out
/// a case that cannot arise. The ONLY heap left holding the default value is
/// the immortal EMPTY heap, and it owns no pages, so nothing can ever push
/// here; every real heap has `delayed` repointed at its own HeapBox during
/// init before it is reachable.
///
/// This is the same immortal-sentinel arrangement `direct[]` already uses with
/// `empty_page_ptr()`, for the same reason: a fast path that can read through
/// a sentinel unconditionally beats one that must first prove it is safe to
/// read. Note the sentinel is deliberately NOT a HeapBox — `xheap` doubles as
/// the HeapBox address (see `box_of_xheap`), and the empty heap never stores
/// `delayed` into a page because it never owns one.
static EMPTY_DELAYED: DelayedList = DelayedList::new();

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

/// The heap.
pub struct Heap {
    /// Page queues, indexed by bin; `pages[BIN_FULL]` parks full pages.
    pub pages: [PageQueue; BIN_COUNT],
    /// `direct[wsize] → page`: the small-malloc fast path (null → generic).
    pub direct: [*mut Page; PAGES_DIRECT],
    /// Owned Normal segments.
    pub segments: *mut Segment,
    /// Empty segments currently parked in the list. Policy: keep ONE empty
    /// segment cached (upstream caches segments too); free beyond that —
    /// without this, large alloc/free cycles pay a 32 MiB OS round-trip each.
    pub empty_segments: u32,
    /// Address of this heap's [`DelayedList`] (inside the owning HeapBox);
    /// pages carry it in `xheap` so remote threads can nudge us. Null only
    /// during const bootstrap.
    pub delayed: *const DelayedList,
    /// Huge (dedicated-segment) allocations owned by this heap, so
    /// delete/destroy/abandon can account for them.
    pub huge_segments: *mut Segment,
    /// Arena restriction: allocate segments only from this arena (−1 = any
    /// arena, then the OS — `mi_heap_new_in_arena`).
    pub arena_id: i32,
    /// Heap tag stamped on every page (visitor filtering; `mi_heap_new_ex`).
    pub tag: i32,
    /// Per-heap CSPRNG: free-list keys, guarded sampling (M8).
    pub rng: crate::random::Random,
    /// Has any page of this heap ever received a CROSS-THREAD free?
    ///
    /// Owner-thread only, so a plain `bool`. Sticky: once a remote free has
    /// been seen the medium retry stays off for this heap's life, which is the
    /// conservative direction (it falls back to the behaviour that has always
    /// shipped).
    pub saw_remote_free: bool,
    /// Trips of the generic path left before the next automatic collect.
    ///
    /// `mi_option_generic_collect`. Counts DOWN so the hot check is a compare
    /// against zero rather than a modulo by a runtime value.
    pub generic_countdown: usize,
    /// Guarded-object sampling: 1-in-N (0 = off), and the size window.
    pub guarded_rate: usize,
    /// Countdown to the next guarded object.
    pub guarded_count: usize,
    /// Minimum size eligible for a guard page.
    pub guarded_min: usize,
    /// Maximum size eligible for a guard page.
    pub guarded_max: usize,
    /// Counters.
    pub stats: Stats,
}

/// The page-queue table every heap starts from, with each bin's block size
/// already filled in.
///
/// This is a `const` ITEM, not a loop inside `Heap::new`. `Heap::new` is a
/// `const fn`, but it is CALLED at runtime by `create_heap`, and a `const fn`
/// called at runtime just runs — so every thread that started re-derived all
/// MAX_NORMAL_BIN block sizes through `bins::bin_size`'s shift-and-mask
/// arithmetic. A `const` item is guaranteed to be evaluated at compile time,
/// so the table becomes a static blob the thread copies.
const EMPTY_QUEUES: [PageQueue; BIN_COUNT] = {
    let mut pages = [PageQueue {
        first: ptr::null_mut(),
        last: ptr::null_mut(),
        block_size: 0,
    }; BIN_COUNT];
    let mut i = 1;
    while i <= MAX_NORMAL_BIN {
        pages[i].block_size = bins::bin_size(i);
        i += 1;
    }
    pages
};

impl Heap {
    /// Const-init empty heap (static bootstrap: usable before any OS call).
    pub const fn new() -> Heap {
        Heap {
            pages: EMPTY_QUEUES,
            // NOT hoisted to a `const` blob like EMPTY_QUEUES above. REFUTED
            // 2026-08-21: doing so cuts 126k Ir out of this crate's symbols and
            // adds 129k to libc's `memcpy` — the whole-program count gets
            // WORSE. A repeated-element fill LLVM already turns into a tight
            // store loop is not worth trading for a rodata copy; EMPTY_QUEUES
            // wins because its elements are COMPUTED, not repeated.
            direct: [crate::page::empty_page_ptr(); PAGES_DIRECT],
            segments: ptr::null_mut(),
            empty_segments: 0,
            delayed: &raw const EMPTY_DELAYED,
            huge_segments: ptr::null_mut(),
            arena_id: -1,
            tag: 0,
            rng: crate::random::Random::new(),
            saw_remote_free: false,
            generic_countdown: 0,
            guarded_rate: 0,
            guarded_count: 0,
            guarded_min: 0,
            guarded_max: usize::MAX,
            stats: Stats::new(),
        }
    }

    /// `mi_heap_guarded_set_sample_rate`: guard 1-in-N eligible objects
    /// (0 disables; 1 guards every one). A nonzero seed makes the sequence
    /// reproducible for debugging.
    pub fn guarded_set_sample_rate(&mut self, rate: usize, seed: usize) {
        // Where no guard page can exist this is a no-op that leaves sampling
        // OFF. It used to accept the rate, and every sampled object then took
        // a dedicated segment with an unprotected trailing page: the cost of a
        // guarded object with none of the protection, silently.
        if !crate::GUARD_PAGES {
            self.guarded_rate = 0;
            self.guarded_count = 0;
            return;
        }
        self.guarded_rate = rate;
        if seed != 0 {
            let s = seed as u32;
            self.rng.seed_from(
                [s, s ^ 0x9E37, s.rotate_left(7), s ^ 0xA5A5, s, s, s, s],
                seed as u64,
            );
        }
        self.guarded_count = if rate == 0 {
            0
        } else {
            1 + self.rng.below(rate)
        };
    }

    /// `mi_heap_guarded_set_size_bound`.
    pub fn guarded_set_size_bound(&mut self, min: usize, max: usize) {
        self.guarded_min = min;
        self.guarded_max = max;
    }

    /// Whether this allocation should get a guard page (samples down the
    /// countdown). Only consulted in `secure`/guarded builds.
    #[inline]
    fn guarded_should_sample(&mut self, size: usize) -> bool {
        if self.guarded_rate == 0 || size < self.guarded_min || size > self.guarded_max {
            return false;
        }
        if self.guarded_count > 1 {
            self.guarded_count -= 1;
            return false;
        }
        self.guarded_count = 1 + self.rng.below(self.guarded_rate);
        true
    }

    /// Allocate `size` bytes. Returns (ptr-or-null, block-known-zero).
    #[inline]
    pub fn malloc(&mut self, size: usize) -> (*mut u8, bool) {
        if size <= SMALL_SIZE_MAX {
            let w = wsize_from_size(size);
            // NEVER null — an empty slot holds the shared empty-page sentinel,
            // whose free list is permanently null. That is what lets this be a
            // SINGLE test: "did we get a block?" also answers "was there a
            // page?" (upstream's `_mi_page_empty`).
            let p = self.direct[w];
            // SAFETY: direct entries always point at a live page of this heap
            // or at the immortal sentinel; we hold the heap lock.
            let b = unsafe { page_pop(p) };
            if !b.is_null() {
                self.stat_alloc();
                // SAFETY: p live per above.
                return (b, unsafe { (*p).free_is_zero });
            }
        } else if size <= MEDIUM_OBJ_SIZE_MAX {
            return self.malloc_in_bin(size, bins::bin(size));
        }
        self.malloc_generic(size)
    }

    /// [`Heap::malloc`] for a MEDIUM size whose bin the caller already knows.
    ///
    /// # Safety note
    /// `bin` must be `bins::bin(size)` and `size` in the medium range; the
    /// peek reads the bin's queue front and falls through to the generic path
    /// on any miss, so a wrong bin would mis-size a block rather than fault —
    /// hence the debug assertion.
    fn malloc_in_bin(&mut self, size: usize, bin: usize) -> (*mut u8, bool) {
        debug_assert_eq!(bin, bins::bin(size), "malloc_in_bin: bin/size disagree");
        // MEDIUM sizes get a fast path too.
        //
        // `direct[]` is indexed by word size and stops at SMALL_WSIZE_MAX, so
        // everything from 1 KiB to 64 KiB used to fall straight through to
        // `malloc_generic` — the heartbeat, the queue walk, the full-page
        // parking, `update_direct` — on EVERY allocation, even when the bin's
        // front page had a block ready. `rptest` allocates 8..4000 bytes and
        // paid that on 29% of its allocations; upstream has the same hole.
        //
        // The bin's queue front is one indirection further than a `direct[]`
        // entry and answers the same question. A null queue or a dry page
        // fails the single "did we get a block?" test and falls through
        // exactly as before.
        //
        // NOTE the shape this is NOT: peeking here from `alloc::malloc_slow`,
        // on the plain-malloc path, is a large REGRESSION (`big`/`large`
        // +25.00 Ir/op) — a tight alloc/free loop frees onto `local_free`, so
        // the queue front's `free` list is always dry and the peek can never
        // hit. It pays only where a populated free list is actually left
        // behind, which is why it lives here and not there.
        let p = self.pages[bin].first;
        if !p.is_null() {
            // SAFETY: queue members are live pages of this heap and we hold
            // the heap lock.
            let b = unsafe { page_pop(p) };
            if !b.is_null() {
                self.stat_alloc();
                // SAFETY: p live per above.
                return (b, unsafe { (*p).free_is_zero });
            }
        }
        self.malloc_generic(size)
    }

    /// Allocate `size` bytes, fully zeroed — `malloc` + zeroing, but with the
    /// popped page IN HAND so the recycled-block path never re-resolves the
    /// usable size.
    ///
    /// The public `zalloc`/`calloc` used to do `malloc(size)` then
    /// `zero_block(p, is_zero)`, and `zero_block`'s non-zero arm called
    /// `usable_size(p)` — which masks to the segment, resolves the page, checks
    /// the segment kind and un-aligns, all to recover a `block_size` this
    /// function already holds in `(*p).block_size`. On the steady calloc churn
    /// path (recycled block ⇒ `free_is_zero` false) that resolution ran on
    /// every call. Here it is a single field load.
    #[inline]
    pub fn zalloc(&mut self, size: usize) -> *mut u8 {
        if size <= SMALL_SIZE_MAX {
            let w = wsize_from_size(size);
            let p = self.direct[w];
            // SAFETY: direct entries always point at a live page of this heap
            // or at the immortal sentinel; we hold the heap lock.
            let b = unsafe { page_pop(p) };
            if !b.is_null() {
                self.stat_alloc();
                // SAFETY: b is a live block of p; `(*p).block_size` is its
                // usable extent — this path never hands out an interior
                // (aligned-at) pointer, so no unalign is needed.
                unsafe {
                    if (*p).free_is_zero {
                        b.cast::<usize>().write(0);
                    } else {
                        ptr::write_bytes(b, 0, (*p).block_size);
                    }
                }
                return b;
            }
        }
        self.zalloc_generic(size)
    }

    /// `zalloc`'s miss, out of line and in TAIL position.
    ///
    /// Inline, this arm's call to `malloc_generic` had to return here to zero
    /// the block, so `size` was live across it and every `calloc` — hit or
    /// miss — paid a frame for it: `push`/`push` on entry and `add`/`pop`/
    /// `pop` before the fast path's `jmp memset` (callgrind, opscan `calloc`,
    /// hit rate 94 %). Out here the fast path is a leaf.
    #[cold]
    #[inline(never)]
    fn zalloc_generic(&mut self, size: usize) -> *mut u8 {
        // Slow/large path: rare, and the generic allocator has resolved the
        // page anyway. Recover the usable size the general way.
        let (b, is_zero) = self.malloc_generic(size);
        if !b.is_null() {
            // SAFETY: b is a live block just returned by malloc_generic.
            unsafe {
                if is_zero {
                    b.cast::<usize>().write(0);
                } else {
                    ptr::write_bytes(b, 0, crate::alloc::usable_size(b));
                }
            }
        }
        b
    }

    /// The slow path — mimalloc's heartbeat: runs when a fast list is dry, so
    /// deferred work (collect, extend, fresh pages; later: purge, deferred
    /// frees) happens at a regular allocation cadence.
    // NOTE (2026-08-21): threading the bin through — `malloc_in_bin` derives
    // `bins::bin(size)` to pick the queue it peeks, and on a miss this
    // function derived the very same bin again — was measured and reverted.
    // It is worth −36,918 Ir on `rptest` (−0.35%) and costs `small` +0.02 and
    // `batch_lifo` +0.06 Ir/op, because the "bin not yet known" marker is a
    // compare every generic call pays, including the small ones that never had
    // a bin to pass. The const-generic form that would fold the check away
    // duplicates this whole function; not worth it for 0.35% of one benchmark.
    /// The generic (slow) path, with ONE reclaim-and-retry before it gives up.
    ///
    /// A page allocator keeps a page per size class as a reuse cache, so a heap
    /// can be simultaneously "full" and holding many empty pages that belong to
    /// classes the caller is not asking for. Returning null in that state
    /// reports OOM while still hoarding reclaimable memory.
    ///
    /// P4d measured exactly that on a XIAO ESP32-S3: 22,533 of 50,000 churn
    /// allocations returned null from a heap that a single `collect` restored
    /// from 8 to 240 blocks of capacity (docs/plans/small-metal.md §2.14). The
    /// periodic `generic_collect` sweep does not cover it — that battery makes
    /// only ~649 generic trips in total, far under the 10,000 default, so the
    /// timer never fires. This trigger is failure, not a clock.
    ///
    /// `collect_inner(true, true)` is `mi_collect(true)` — reclaim orphans
    /// too, since this is the last resort before null.
    ///
    /// **Where the retry test lives is a cost decision.** It first sat here as
    /// `let r = once(size); if r.0.is_null() { collect; once(size) }`, which
    /// read as free on the happy path and was not: a test AFTER the call means
    /// the call is no longer in tail position, so `alloc::malloc_slow` — whose
    /// own doc says "this arm must stay a TAIL call" — grew a frame, a spill of
    /// `size` and a `test; je` again, 16 Ir around a 1-instruction jump on
    /// every slow-path allocation (callgrind, opscan `big`). Now the test is in
    /// `malloc_generic_once`'s epilogue, where the result is already in a
    /// register, and this wrapper is a tail call.
    ///
    /// And "only once" is a THREAD-LOCAL, not a parameter. As a `retry: bool`
    /// argument it cost every generic trip a `mov $1` at the call, a copy into
    /// a callee-saved register, and a flag test ahead of the null test in the
    /// epilogue — about four instructions to decide something only a null
    /// result ever asks. Now the epilogue tests the result alone, and the flag
    /// is read on the OOM path only.
    ///
    /// Inside the chain the zero flag is a `u8`, and it becomes a `bool` only
    /// here. A `(*mut u8, bool)` pair return makes the caller re-truncate the
    /// flag (`and $0x1,%dl`) after every call that returns one, which is what
    /// kept `malloc_generic_once` from TAIL-calling `grow_front` — the
    /// commonest generic outcome on a real program (jq: 10,307 of 11,229
    /// trips) — and the walk. Callers that want only the pointer, like
    /// `alloc::malloc_slow`, drop the conversion entirely.
    #[inline]
    pub(crate) fn malloc_generic(&mut self, size: usize) -> (*mut u8, bool) {
        let (p, z) = self.malloc_generic_once(size);
        (p, z != 0)
    }

    /// One pass of the generic path; on null, one reclaim-and-retry through
    /// [`Heap::malloc_generic_retry`].
    ///
    /// NOTE (2026-09-24, REFUTED on real programs): splitting the medium
    /// collect-and-retry into a frameless entry that tail-calls the rest was
    /// worth **-11.00 Ir/op** on opscan `big`/`large` and -6.83 on `mixed` —
    /// the arm was paying this function's four-register frame. It cost every
    /// OTHER generic trip ~8 Ir (an alignment `push` pinned by the collect
    /// walk's diverging `double_free_abort`, a `pop`, a bool zero-extension
    /// and a jump), and real programs make more of those than medium hits:
    /// allocator Ir **lua +31,945, perl +23,549, sqlite +15,997**. Reverted.
    /// Making the abort a may-return callee to drop the `push` was far worse
    /// (see `page_collect_impl`).
    ///
    /// The OOM test is NOT here. In this epilogue it sat after every arm,
    /// including the one that serves most real-program trips — jq: 10,307 of
    /// 11,229 generic trips end in `grow_front` — which it turned from a tail
    /// call into `call`, `jmp`, a copy and a `test; jne` (callgrind, per
    /// instruction), for an arm that cannot fail. It lives at the three exits
    /// that CAN return null: the fresh-page carve at the end of the walk, and
    /// the large and huge arms.
    #[inline(never)]
    fn malloc_generic_once(&mut self, size: usize) -> (*mut u8, u8) {
        self.malloc_generic_body(size)
    }

    /// The reclaim-and-retry. The nested pass it makes lands back here on a
    /// second null, and `IN_OOM_RETRY` is what turns that into the final
    /// answer instead of another round.
    #[cold]
    #[inline(never)]
    fn malloc_generic_retry(&mut self, size: usize) -> (*mut u8, u8) {
        if IN_OOM_RETRY.with(Cell::get) {
            return (ptr::null_mut(), 0);
        }
        IN_OOM_RETRY.with(|c| c.set(true));
        // SAFETY: owner thread; `collect_inner` allocates nothing.
        unsafe { self.collect_inner(true, true) };
        let r = self.malloc_generic_once(size);
        IN_OOM_RETRY.with(|c| c.set(false));
        r
    }

    #[inline(always)]
    fn malloc_generic_body(&mut self, size: usize) -> (*mut u8, u8) {
        self.stats.generic += 1;
        // Guarded objects (secure/guarded builds): sampled allocations get a
        // dedicated segment whose trailing page is PROT_NONE, so an overflow
        // faults immediately instead of corrupting a neighbour.
        // `GUARD_PAGES` first: on a target that cannot protect a page the
        // whole arm folds away, and `try_guarded` (1,641 B on an ESP32-S3)
        // with it. The runtime rate alone could not do that -- it is a field,
        // and the linker cannot prove a field is zero.
        if crate::GUARD_PAGES
            && self.guarded_rate != 0
            && let Some(r) = self.try_guarded(size)
        {
            return (r.0, u8::from(r.1));
        }
        // COLLECT-AND-RETRY for the medium band, and it TURNS ITSELF OFF.
        //
        // Through `GlobalAlloc` a medium allocation reaches this function on
        // every call: `alloc::malloc` serves `size <= SMALL_SIZE_MAX` from the
        // direct table and tail-calls `malloc_slow`, which comes straight here,
        // while `Heap::malloc`'s medium branch is on a different entry point.
        // Measured at 1.000 generic trips per op for 2 KiB against 0.008 for
        // 32 B -- routing, not list state. Collecting the queue front and
        // retrying before the heartbeat is worth +15 % on wasm and +15-19 % on
        // native for a 2 KiB tight alloc/free loop.
        //
        // It is worth **-20 to -30 %** the moment another thread is freeing into
        // these pages. `free`, `local_free` and `xthread_free` are adjacent
        // fields of a `#[repr(C)]` `Page`, so touching the queue front at all
        // pulls a line the remote thread is invalidating -- doing only the
        // local half of the collect measured WORSE, not better, which is how
        // that was established.
        //
        // So the predicate is not "how many threads exist" but "does THIS heap
        // receive remote frees", and the retry answers it itself: `page_collect`
        // reports whether it stole a cross-thread chain, and the first steal
        // latches the retry off for this heap. A thread that owns its
        // allocations keeps the win however many threads the process has; a
        // producer whose pages a consumer frees pays one detection and then
        // behaves exactly as before.
        // NOTE (2026-09-24, REFUTED): the arm usually MISSES on a real program
        // (perl: 7,708 of 11,384 generic trips entered it, 7,695 found the
        // front page dry after the collect), so those trips derive this bin
        // twice — here and after the heartbeat. Computing it ONCE above both
        // uses made it live across the heartbeat's calls: opscan `big`/`large`
        // +17.00, `mixed` +11.86, perl allocator +101,962. Recomputing a pure
        // value is cheaper than holding it in a callee-saved register.
        if !self.saw_remote_free && size > SMALL_SIZE_MAX && size <= MEDIUM_OBJ_SIZE_MAX {
            let bin = bins::bin(size);
            let p = self.pages[bin].first;
            if !p.is_null() {
                // POP FIRST, as `malloc`'s own fast path does: when the front
                // page's free list is non-empty the collect below changes
                // nothing the pop needs — it would test the list, read the
                // cross-thread word and re-load the list, eight instructions
                // ahead of the pop on every medium hit (opscan `mixed`,
                // callgrind per instruction). Remote frees still get
                // collected, and the latch below still set, the first time
                // the list runs dry.
                // SAFETY: as for the collect-and-pop below.
                let b = unsafe { page_pop(p) };
                if !b.is_null() {
                    self.stat_alloc();
                    // SAFETY: p live per above.
                    return (b, u8::from(unsafe { (*p).free_is_zero }));
                }
                // SAFETY: queue members are live pages of this heap, we are the
                // owner thread, and `page_collect` is the same operation the
                // walk below performs on this page.
                //
                // The latch is stored BEFORE the pop, inside the branch that
                // knows it. Written as `(stole, pop)` and tested after, LLVM
                // merged the two collect outcomes first and then materialised
                // and re-tested a flag that is constant on the path that
                // matters — `xor %eax,%eax` … `test %al,%al; je`, two dead
                // instructions on every medium hit (opscan `big`/`large`).
                if unsafe { crate::page::page_collect(p) } {
                    self.saw_remote_free = true;
                }
                // SAFETY: as above.
                let b = unsafe { page_pop(p) };
                if !b.is_null() {
                    self.stat_alloc();
                    // SAFETY: p live per above.
                    return (b, u8::from(unsafe { (*p).free_is_zero }));
                }
                // The front page is dry but not fully carved: grow it HERE.
                // Falling through, the trip ran the heartbeat, derived this
                // same bin again, collected this same page again and only
                // then reached `grow_front` — and on a real program that is
                // the common medium outcome, not the hit (perl: 7,693 of
                // 7,707 medium trips missed here; callgrind, per
                // instruction). Skipping the heartbeat is what the hit above
                // already does; it still runs when the page is exhausted and
                // the walk is needed.
                // SAFETY: `p` is the live front page of this bin's queue,
                // its free list just came back empty, and it has room to
                // carve — `grow_front`'s contract.
                //
                // `w` is passed as "above the direct table" rather than
                // computed: `grow_front` reads it only for that test, a medium
                // size is always above it, and computing `wsize_from_size`
                // here made LLVM rewrite the bin arithmetic on the HIT path
                // above (+5.00 Ir/op on opscan `big`/`large`).
                unsafe {
                    if (*p).capacity < (*p).reserved {
                        return self.grow_front(bin, crate::types::SMALL_WSIZE_MAX + 1, p);
                    }
                }
            }
        }
        // Heartbeat: process cross-thread delayed frees at slow-path cadence
        // (this is what un-parks full pages whose blocks died remotely), and
        // fire the registered deferred-free hook (mi_register_deferred_free).
        // SAFETY: we are the owner thread.
        unsafe { self.process_delayed() };
        crate::options::deferred_free(false);
        // Periodic collect (`mi_option_generic_collect`, default 10,000).
        //
        // Upstream runs an UNFORCED collect every N trips of this path. Ours
        // declared the option and read it nowhere, so nothing ever collected on
        // its own: a heap that had touched many size classes kept a page per
        // class forever and could not give the slices back, even though a
        // manual `collect` would have. P4d measured that on hardware — 512 B
        // capacity decaying 168 -> 8 blocks and 22,533 of 50,000 churn
        // allocations returning null with 61,440 bytes of the region free
        // (docs/plans/small-metal.md §2.14).
        //
        // `reclaim = false`: this is a routine sweep of our own pages, not the
        // orphan adoption a forced `mi_collect(true)` performs.
        //
        // One decrement whose wrap means "it was zero", rather than a test
        // and then a decrement: the same schedule (N trips between sweeps,
        // reset on the trip that finds zero), in a form that is a
        // memory-destination `sub` and one branch on every generic trip
        // instead of load, test, branch, decrement and store.
        let left = self.generic_countdown.wrapping_sub(1);
        self.generic_countdown = left;
        if left == usize::MAX {
            self.generic_countdown =
                crate::options::get_clamp(crate::options::GENERIC_COLLECT, 1, 1_000_000) as usize;
            // SAFETY: owner thread, and `collect_inner` allocates nothing.
            unsafe { self.collect_inner(false, false) };
        }
        if size > MEDIUM_OBJ_SIZE_MAX {
            let r = if size <= LARGE_OBJ_SIZE_MAX {
                self.large_alloc(size)
            } else {
                self.huge_alloc(size, 8, 0)
            };
            if r.0.is_null() {
                return self.malloc_generic_retry(size);
            }
            return (r.0, u8::from(r.1));
        }
        let bin = bins::bin(size);
        let w = wsize_from_size(size);
        // SAFETY: `bin` indexes `self.pages`; the queue's first page, if any,
        // is a live page of this heap.
        unsafe {
            let q: *mut PageQueue = &raw mut self.pages[bin];
            let p = (*q).first;
            if !p.is_null() {
                page_collect(p);
                if !(*p).free.is_null() {
                    // Already the queue head, so the reorder the walk performs
                    // is a no-op here and is left out of this frame.
                    //
                    // `update_direct` re-derives the bin's block size, its top
                    // word index and the front page before discovering that
                    // nothing moved — ten instructions to confirm a no-op, on
                    // the path 99.5% of generic allocations take. `direct[]`
                    // is only ever written as a whole bin-range, so ANY one
                    // slot of this bin answers for all of them, and the
                    // request's own word size is such a slot: `bins::bin`
                    // above already computed it, so reusing it is free.
                    if w > SMALL_WSIZE_MAX || self.direct[w] != p {
                        self.update_direct(bin);
                    }
                    let b = page_pop(p);
                    self.stat_alloc();
                    return (b, u8::from((*p).free_is_zero));
                }
                if (*p).capacity < (*p).reserved {
                    return self.grow_front(bin, w, p);
                }
            }
        }
        // SAFETY: as above.
        unsafe { self.malloc_generic_walk(bin) }
    }

    /// The queue front has no block but has not been fully carved yet:
    /// extend it and serve from the new run.
    ///
    /// Out of line for a reason invisible in the source. Everything on the
    /// fast path above returns or tail-calls, so nothing of ours is live
    /// across a call — except here: `page_extend` had `self`, `bin` and `p`
    /// live across it, and that ALONE forced `malloc_generic` to preserve two
    /// callee-saved registers, on every generic allocation, for an arm that
    /// most of them never reach.
    ///
    /// This is a MEASURED TRADE, taken deliberately, not a free win:
    ///
    /// ```text
    ///     cfrac allocator   4,030,751,631 -> 4,010,286,980   -20,464,651
    ///     opscan big/large      -6.00/op  ->     -14.00/op        -8.00
    ///     opscan mixed          -4.03/op  ->      -9.81/op        -5.78
    ///     perl                776,511,765 ->   776,690,628      +178,863
    /// ```
    ///
    /// perl pays one call per extend and it carves constantly, so the arm
    /// marked `#[cold]` here is genuinely warm in that one workload — the same
    /// shape as the fresh-page split refuted on alloc-test at +145,129 perl.
    /// It is accepted anyway because the ratio is 114:1 in instructions and
    /// the gain is corpus-wide (every `opscan` op improves, several by more
    /// than double), where the loss is 0.023% of a single program. If perl-like
    /// carve-heavy workloads ever become the priority, THIS is the line to
    /// flip, and the numbers above are what it costs to flip it.
    ///
    /// # Safety
    /// `bin` indexes `self.pages`; `w` is the request's word size; `p` is the
    /// live front page of that queue, with `free` empty and
    /// `capacity < reserved`.
    #[cold]
    #[inline(never)]
    unsafe fn grow_front(&mut self, bin: usize, w: usize, p: *mut Page) -> (*mut u8, u8) {
        // SAFETY: forwarded contract.
        unsafe {
            page_extend(p, (*p).area);
            self.stats.extends += 1;
            if (*p).free.is_null() {
                return self.malloc_generic_walk(bin);
            }
            if w > SMALL_WSIZE_MAX || self.direct[w] != p {
                self.update_direct(bin);
            }
            let b = page_pop(p);
            self.stat_alloc();
            (b, u8::from((*p).free_is_zero))
        }
    }

    /// The rest of [`Heap::malloc_generic`]: park the pages that cannot serve,
    /// walk the queue, and carve a fresh page if none can.
    ///
    /// Split off so the overwhelmingly common outcome — the queue's FRONT page
    /// has a block, or can grow one — does not pay a frame sized for the walk,
    /// the fresh-page carve and the OOM arm. That frame cost eight
    /// instructions of push and nine of pop on EVERY generic allocation, 20%
    /// of the function, for machinery most calls never reach.
    ///
    /// The first queue page is re-collected here rather than threaded in: the
    /// collect is idempotent, and re-doing it keeps the two frames
    /// independent of each other's register needs, which is the whole point.
    ///
    /// # Safety
    /// `bin` indexes `self.pages`.
    #[inline(never)]
    unsafe fn malloc_generic_walk(&mut self, bin: usize) -> (*mut u8, u8) {
        // SAFETY: all page/queue manipulation below happens under the heap
        // lock on pages owned by this heap; raw pointers are used so no two
        // Rust references to the same Page coexist.
        unsafe {
            let q: *mut PageQueue = &raw mut self.pages[bin];
            let mut p = (*q).first;
            while !p.is_null() {
                if (*p).free.is_null() {
                    // A steal ANYWHERE in this heap's queue means this heap
                    // receives cross-thread frees, which is what the medium
                    // retry must not run into. Latching only when the retry
                    // itself steals was not enough: the frees that hurt land on
                    // OTHER pages of the same heap, so the retry's own page
                    // never sees them and it never turned itself off.
                    if page_collect(p) {
                        self.saw_remote_free = true;
                    }
                }
                if (*p).free.is_null() && (*p).capacity < (*p).reserved {
                    // `(*p).area` is the cached payload start (see `Page::area`);
                    // it replaces `segment_of + page_index + page_area` — a mask,
                    // a division and a shift — with one load on the refill path.
                    page_extend(p, (*p).area);
                    self.stats.extends += 1;
                }
                if !(*p).free.is_null() {
                    // REFUTED (larson-sized, 2026-08-21): this branch is DEAD.
                    // Every iteration below ends by removing `p` from this
                    // queue before advancing, so the page the loop is looking
                    // at is always the front already, and a `debug_assert_eq!`
                    // in its place ran the whole `debug_checks` suite without
                    // firing. Deleting it nonetheless measured WORSE —
                    // 52.920 -> 52.945 Ir/op on an instrument exact to the
                    // unit — because the compare is free in practice and its
                    // removal perturbs register allocation around the return.
                    // Kept, both for the invariant it documents and because it
                    // is the cheaper of the two forms. Do not "clean this up"
                    // without re-measuring.
                    if p != (*q).first {
                        queue_remove(q, p);
                        queue_push_front(q, p);
                    }
                    self.update_direct(bin);
                    let b = page_pop(p);
                    self.stat_alloc();
                    return (b, u8::from((*p).free_is_zero));
                }
                // Truly full: park it so the queue front stays useful.
                let next = (*p).next;
                // Park it — deliberately INLINE. Outlining this as a
                // `#[cold]` helper was a −2,415,021 win on `cfrac` when
                // `malloc_generic` was one large function; once the walk was
                // split out (`malloc_generic_walk`), the same change turned
                // into a LOSS on every workload measured — cfrac +117,465,
                // perl +23,521, sh6bench +1,553,862 — because this code now
                // sits inside a function that is ALREADY out of line and
                // entered on 0.53% of generic calls. Outlining it again only
                // nests a second call inside the rare path.
                //
                // A win can expire the same way a refutation can. Re-measure
                // an outlining decision whenever the function around it is
                // restructured; the frequency that justified it is a property
                // of the enclosing frame, not of this code.
                queue_remove(q, p);
                (*p).flags.fetch_or(pflags::IN_FULL, Ordering::Relaxed);
                page_set_flag(p, XFLAG_DELAYED);
                queue_push_front(&raw mut self.pages[BIN_FULL], p);
                p = next;
            }
            // No usable page — carve a fresh one.
            let bsize = (*q).block_size;
            let p = self.fresh_page(bin, bsize);
            if p.is_null() {
                self.update_direct(bin);
                // OOM: the one reclaim-and-retry, with this bin's block size —
                // the same class and the same block the caller would get.
                // Here and not in `malloc_generic_once`'s epilogue: see
                // `Heap::malloc_generic`.
                return self.malloc_generic_retry(bsize);
            }
            page_extend(p, (*p).area);
            self.stats.extends += 1;
            self.update_direct(bin);
            let b = page_pop(p);
            self.stat_alloc();
            (b, u8::from((*p).free_is_zero))
        }
    }

    // NOTE (2026-08-21): outlining the large/huge tail as a `#[cold]`
    // `malloc_generic_big`, to keep its register needs out of the binned
    // refill path's prologue, measured FLAT (+28 Ir on alloc-test's 2.2G).
    // Its sibling — outlining the fresh-page tail — was worse than flat; see
    // below.

    // NOTE (2026-08-21): the no-usable-page tail was ALSO outlined here, as
    // `malloc_generic_fresh`, on the same "shrink the frame" reasoning as
    // `malloc_generic_big` above. It gained alloc-test 0.58M Ir and cost PERL
    // **+145,129** — perl carves fresh pages constantly, where alloc-test
    // carves a few hundred against 100M operations, so the call and argument
    // setup are paid on a path that is only cold in one of the two. Reverted:
    // a split is only worth it where the outlined arm is genuinely rare.

    /// The guarded-object sampling arm of [`Heap::malloc_generic`].
    ///
    /// `guarded_rate` is 0 unless the `guarded_max` option is set, so in the
    /// shipped default this never runs at all — but its call to
    /// `guarded_alloc`, which carves a dedicated segment with a PROT_NONE
    /// trailing page, put its register needs in `malloc_generic`'s prologue
    /// and epilogue regardless.
    #[cold]
    #[inline(never)]
    fn try_guarded(&mut self, size: usize) -> Option<(*mut u8, bool)> {
        if !self.guarded_should_sample(size) {
            return None;
        }
        let (p, z) = self.guarded_alloc(size);
        if p.is_null() { None } else { Some((p, z)) }
    }
    /// Carve a fresh page for `bin` from the segment list (new segment if all
    /// are exhausted) and push it to the queue front.
    fn fresh_page(&mut self, bin: usize, bsize: usize) -> *mut Page {
        let slices = if bsize <= SMALL_OBJ_SIZE_MAX {
            1
        } else {
            MEDIUM_PAGE_SLICES
        };
        // SAFETY: heap lock held; segments list is owned by this heap.
        unsafe {
            let Some((p, fresh)) = self.span_from_segments(slices) else {
                return ptr::null_mut();
            };
            (*p).block_size = bsize;
            // `bsize` here is `bins::bin_size(bin)`, so the odd part is in
            // {1,3,5,7} and this is four constant divisions (multiplies),
            // not a runtime `div`. Refuted once before, but only as a pair
            // with the `page_extend` substitution that was itself the loss.
            (*p).reserved =
                crate::bins::div_by_block_size(slices * SEGMENT_SLICE_SIZE, bsize) as u32;
            // `blockmap`: the liveness map is carved from the front of this
            // page's payload (see `page_extend`), so the block count has to
            // come down by the bytes it occupies or the last block would run
            // off the end of the span. Sized from the PRE-reduction count,
            // which is conservative — the map that actually gets carved is
            // sized from the reduced `reserved` and is therefore no larger.
            //
            // `bs_shift`/`bs_inv` turn an address delta into a block index
            // with a shift and a multiply instead of a division; block sizes
            // here are not powers of two, so the division would otherwise land
            // on the allocation hot path.
            #[cfg(feature = "blockmap")]
            {
                let raw = (slices * SEGMENT_SLICE_SIZE) / bsize;
                let bm = crate::page::bitmap_bytes(raw);
                (*p).reserved =
                    crate::bins::div_by_block_size(slices * SEGMENT_SLICE_SIZE - bm, bsize) as u32;
                (*p).payload = ptr::null_mut();
                (*p).bs_inv = crate::page::odd_mod_inverse(bsize >> bsize.trailing_zeros());
            }
            (*p).capacity = 0;
            (*p).used = 0;
            (*p).free = ptr::null_mut();
            (*p).local_free = ptr::null_mut();
            (*p).bin = bin as u8;
            (*p).flags.store(0, Ordering::Relaxed);
            // Bump-fresh spans of an eager-committed zero mapping are zero;
            // RECLAIMED spans are recycled memory and are not.
            (*p).free_is_zero = fresh;
            (*p).xheap.store(self.delayed as usize, Ordering::Release);
            (*p).heap_tag = self.tag;
            #[cfg(feature = "secure")]
            {
                // Fresh per-page keys: an attacker who learns one page's
                // encoding cannot steer another.
                (*p).keys = [self.rng.next_usize() | 1, self.rng.next_usize()];
            }
            page_set_flag(p, XFLAG_NORMAL);
            self.stats.pages_fresh += 1;
            queue_push_front(&raw mut self.pages[bin], p);
            p
        }
    }

    /// First-fit a span across owned segments, allocating a new segment when
    /// all are exhausted. Returns (span start, span-is-fresh-zero).
    ///
    /// # Safety
    /// Heap lock held.
    unsafe fn span_from_segments(&mut self, slices: usize) -> Option<(*mut Page, bool)> {
        /// Stall guard, NOT a reclaim budget: a huge orphan backlog must not
        /// let a single allocation walk the whole abandoned list.
        const MAX_ADOPT: usize = 32;

        // SAFETY: heap lock held; segments list is owned by this heap.
        unsafe {
            let mut seg = self.segments;
            while !seg.is_null() {
                let was_empty = (*seg).used_pages == 0;
                let (p, fresh) = span_alloc(seg, slices);
                if !p.is_null() {
                    if was_empty {
                        self.empty_segments -= 1;
                    }
                    return Some((p, fresh));
                }
                seg = (*seg).next;
            }
            // Before reserving fresh OS memory: adopt abandoned segments from
            // dead threads (bounded — this is a slow-path heartbeat duty).
            // ADOPT UNTIL SATISFIED, not twice. The old bound was `tries < 2`:
            // with orphans piled up we adopted at most two, re-scanned, and
            // then took a FRESH 32 MiB segment from the arena anyway while the
            // rest sat unclaimed. Measured: 25 abandoned segments, and a
            // 2048-block allocation burst reclaimed 4. Each orphan is 32 MiB,
            // which is the RSS tail.
            //
            // Now each adopted segment is tried IMMEDIATELY — the one just
            // taken is the one most likely to have room — and the loop stops as
            // soon as the request is met or the list is empty. The cap only
            // exists so a huge orphan backlog cannot stall one allocation; it
            // is not a reclaim budget (see MAX_ADOPT at the top of the fn).
            for _ in 0..MAX_ADOPT {
                let aseg = crate::init::abandoned_pop();
                if aseg.is_null() {
                    break;
                }
                if !self.adopt_segment(aseg) {
                    // Adoption RELEASED it (arrived empty, or a dead huge
                    // block). `aseg` is unmapped — the request is unmet, so
                    // keep draining the abandoned list rather than touching it.
                    continue;
                }
                let was_empty = (*aseg).used_pages == 0;
                let (p, fresh) = span_alloc(aseg, slices);
                if !p.is_null() {
                    if was_empty && self.empty_segments > 0 {
                        self.empty_segments -= 1;
                    }
                    return Some((p, fresh));
                }
            }
            let seg = segment::segment_alloc(self.arena_id).ok()?;
            self.stats.segments += 1;
            (*seg).next = self.segments;
            self.segments = seg;
            let (p, fresh) = span_alloc(seg, slices);
            debug_assert!(!p.is_null());
            Some((p, fresh))
        }
    }

    /// Large objects (64 KiB..16 MiB): one single-block page spanning enough
    /// slices, allocated fresh per request and retired on free (no queue —
    /// span reclamation IS the reuse mechanism).
    fn large_alloc(&mut self, size: usize) -> (*mut u8, bool) {
        let slices = size.div_ceil(SEGMENT_SLICE_SIZE);
        // SAFETY: heap lock held; page/segment owned by this heap.
        unsafe {
            let Some((p, fresh)) = self.span_from_segments(slices) else {
                return (ptr::null_mut(), false);
            };
            (*p).block_size = slices * SEGMENT_SLICE_SIZE;
            (*p).reserved = 1;
            (*p).capacity = 1;
            (*p).used = 1;
            (*p).free = ptr::null_mut();
            (*p).local_free = ptr::null_mut();
            (*p).bin = BIN_HUGE as u8; // marker: unqueued single-block span
            // `blockmap`: a single-block span carves no map, and the map code
            // knows that only by `payload` being null. This slot may have been
            // a small page's span start, and `fresh_page` — the one other
            // writer — is not on this path, so a stale `payload` survived: an
            // adopted segment's collect then decoded this block against the
            // OLD tenant's map, read a bit past the end of this block, and
            // aborted as a double free. `stress_mt` on 4 CPUs, every run.
            #[cfg(feature = "blockmap")]
            {
                (*p).payload = ptr::null_mut();
            }
            (*p).flags.store(pflags::SINGLE_BLOCK, Ordering::Relaxed); // unqueued single-block span
            (*p).free_is_zero = fresh;
            // Unqueued → never scanned → remote frees must go via the
            // delayed list (same rule as parked-full pages).
            (*p).xheap.store(self.delayed as usize, Ordering::Release);
            (*p).heap_tag = self.tag;
            page_set_flag(p, XFLAG_DELAYED);
            self.stat_alloc();
            self.stats.large_allocs += 1;
            ((*p).area, fresh)
        }
    }

    /// Guarded allocation: a dedicated huge segment whose page after the
    /// block is protected. The block is placed so its END abuts the guard
    /// (buffer overflows fault on the first byte past the object).
    fn guarded_alloc(&mut self, size: usize) -> (*mut u8, bool) {
        let ps = crate::os::page_size();
        let payload = crate::os::page_align_up(size.max(1));
        // An unsatisfiable payload (`page_align_up` saturated, or the guard
        // page does not fit) is null, not `payload + page` wrapped into a
        // small guarded object.
        if payload < size.max(1) {
            return (ptr::null_mut(), false);
        }
        let Some(need) = payload.checked_add(ps) else {
            return (ptr::null_mut(), false);
        };
        // huge_alloc gives us a dedicated segment with a page-aligned block.
        let (block, _z) = self.huge_alloc(need, ps, 0);
        if block.is_null() {
            return (ptr::null_mut(), false);
        }
        // SAFETY: block is the start of a fresh dedicated reservation with at
        // least payload + one page of committed memory.
        unsafe {
            let guard = block.add(payload);
            if crate::os::protect(guard, ps, true).is_err() {
                return (block, true); // protection unavailable: still valid memory
            }
            // Record it on the SEGMENT: the protection must be lifted before
            // this memory can be recycled through an arena (the M8 P0).
            (*segment_of(block)).guarded = true;
            self.stats.guarded += 1;
            // Right-align the object against the guard page.
            let p = guard.sub(size.max(1));
            let seg = segment_of(block);
            let pg: *mut Page = &raw mut (*seg).pages[1];
            (*pg).flags.fetch_or(pflags::HAS_ALIGNED, Ordering::Relaxed);
            (p, true)
        }
    }

    /// Out of line: `segment::huge_alloc` returns its `Result` through a stack
    /// slot, and inlined here that slot became part of `malloc_generic_once`'s
    /// frame — a `sub`/`add` of the stack pointer on EVERY generic trip for
    /// an arm taken on requests above 32 MiB (callgrind, per instruction).
    #[inline(never)]
    fn huge_alloc(&mut self, size: usize, align: usize, offset: usize) -> (*mut u8, bool) {
        match segment::huge_alloc(size, align, offset, self.arena_id) {
            Ok((seg, block)) => {
                // SAFETY: fresh segment we own; page slot 1 is its block's
                // metadata. DELAYED + xheap route remote frees through our
                // delayed list, unifying huge with the protocol.
                unsafe {
                    let pg: *mut Page = &raw mut (*seg).pages[1];
                    (*pg).xheap.store(self.delayed as usize, Ordering::Release);
                    (*pg).flags.fetch_or(
                        pflags::HUGE_SEGMENT | pflags::SINGLE_BLOCK,
                        Ordering::Relaxed,
                    );
                    page_set_flag(pg, XFLAG_DELAYED);
                    (*seg).next = self.huge_segments;
                    self.huge_segments = seg;
                }
                self.stat_alloc();
                self.stats.huge_allocs += 1;
                // A Huge segment IS a segment, and the release path already
                // counts it as one (`segments_freed` next to `huge_free`,
                // heap.rs:1243). Without this the pair is asymmetric and a
                // workload that cycles huge blocks ends with more segments
                // freed than allocated — an impossible reading from the
                // counters this project uses as its work-parity instrument.
                // Found by P2 of docs/plans/small-metal.md.
                self.stats.segments += 1;
                // SAFETY: seg live; recycled arena chunks are NOT zero.
                (block, unsafe { (*seg).mem_is_zero })
            }
            Err(_) => (ptr::null_mut(), false),
        }
    }

    /// Unlink a huge segment from this heap's list. **Returns whether it was
    /// actually found and unlinked; `false` means `seg` is not ours and must
    /// NOT be released.**
    ///
    /// This mirrors [`Heap::remove_segment`], and for the same reason. That
    /// function used to end in a bare `debug_assert!(false, …)` — compiled
    /// out in release — so a caller that failed to unlink fell through and
    /// freed a segment still linked in another list, leaving a dangling head
    /// that later crashed `thread_done`'s walk (0.4.0, defect #3; the probe
    /// fired in 4 of 30 runs). The normal-segment path was fixed by returning
    /// the fact and making it `#[must_use]`; **the HUGE path kept the unsound
    /// shape until a `tools/semgrep-rules.yml` rule written from that very
    /// incident flagged it (2026-08-19).** The one caller now releases only
    /// what it unlinked.
    ///
    /// # Safety
    /// Owner thread.
    #[must_use = "false means the segment was NOT unlinked and must not be freed"]
    unsafe fn remove_huge_segment(&mut self, seg: *mut Segment) -> bool {
        // SAFETY: forwarded contract.
        let unlinked = unsafe { self.try_unlink_huge_segment(seg) };
        // Diagnostic only — the RETURN above is what the caller acts on, and
        // it is correct in release where this assertion does not exist.
        debug_assert!(unlinked, "huge segment not in heap list");
        unlinked
    }

    /// The list walk itself, with NO diagnostic attached.
    ///
    /// Split out so the DECISION is testable: with the `debug_assert!` inline,
    /// a test build panics on the not-found path and the `false` return — the
    /// entire point of the fix — can never be observed. `unlink_tests` below
    /// exercises this directly, which is what turns "reasoned fix" into
    /// "reproduced fix" (the ledger's standing complaint about the 0.3.2-era
    /// repairs is that they were the former).
    ///
    /// # Safety
    /// Owner thread. `seg` is compared but never dereferenced unless found,
    /// so a foreign address is safe to pass.
    #[must_use]
    unsafe fn try_unlink_huge_segment(&mut self, seg: *mut Segment) -> bool {
        // SAFETY: list owned by this heap; `seg` is only read through after
        // it has been matched, i.e. once it is known to be one of ours.
        unsafe {
            let mut cur = &raw mut self.huge_segments;
            while !(*cur).is_null() {
                if *cur == seg {
                    *cur = (*seg).next;
                    return true;
                }
                cur = &raw mut (**cur).next;
            }
            false
        }
    }

    /// Aligned allocation (M5: the full `_at` form): returns `p` with
    /// `(p + offset) % align == 0`. Tiers, cheapest first:
    /// natural fit through the bins (blocks sit at `i * bsize` in 64 KiB-
    /// aligned areas, so `bsize % align == 0` ⟺ every block qualifies);
    /// oversize-and-adjust (interior pointer, page marked `has_aligned`);
    /// dedicated huge segment with computed placement.
    pub fn malloc_aligned_at(
        &mut self,
        size: usize,
        align: usize,
        offset: usize,
    ) -> (*mut u8, bool) {
        // HOT: upstream's aligned fast path (`mi_heap_malloc_zero_aligned_at_fast`)
        // — don't PROVE alignment from the bin geometry, peek the bin's next
        // free block and test its ACTUAL address with one AND.
        //
        // Everything else — the size/alignment bound, the natural-fit search
        // through `good_size`, huge placement and oversize-and-adjust — lives
        // in `malloc_aligned_at_slow`. It is not the branches that cost: it is
        // that sharing a frame with them made this function spill callee-saved
        // registers, and the prologue and epilogue that entailed measured
        // 20.00 Ir/op — 40% of the whole function — on a path that uses almost
        // no registers at all.
        //
        // BOTH halves of the validation stay on this side. `is_power_of_two`
        // because `align - 1` is only a correct mask for a power of two — a
        // caller passing 3 would otherwise be handed a block satisfying
        // `& 2 == 0`. And the upper bound because leaving it to the cold half
        // changes OBSERVABLE behaviour: an `align` past SEGMENT_SIZE/2, which
        // this allocator documents as refused, would otherwise be served
        // whenever a block's address happened to satisfy the mask, making the
        // refusal depend on address luck. One compare against a constant.
        //
        // The power-of-two test is written as the bare `align & (align - 1)`
        // rather than `is_power_of_two`, which is `count_ones() == 1` and
        // expands to a non-zero test AND that `and`. Dropping the non-zero
        // half is sound HERE and only here: `align == 0` makes the mask
        // `usize::MAX`, which no non-null block address can satisfy, so a zero
        // alignment still falls through to the full validation below and is
        // still refused — it just takes the cold road to get there.
        if offset == 0
            && size <= SMALL_SIZE_MAX
            && align & align.wrapping_sub(1) == 0
            && align <= crate::types::SEGMENT_SIZE / 2
        {
            let w = crate::types::wsize_from_size(size);
            let p = self.direct[w];
            // SAFETY: direct entries point at a live page of this heap or the
            // immortal empty-page sentinel; reading `free` is the same access
            // `page_pop` performs.
            let b = unsafe { (*p).free };
            // `wrapping_sub`: `align == 0` reaches this peek (the power-of-two
            // test admits it), and the mask must then be `usize::MAX` — which
            // fails for any non-null `b` and takes the refusing slow path —
            // not a debug overflow. Identical release codegen.
            if !b.is_null() && b.addr() & align.wrapping_sub(1) == 0 {
                // SAFETY: as `Heap::malloc` — pop from this thread's own page.
                let b = unsafe { crate::page::page_pop(p) };
                debug_assert!(!b.is_null());
                self.stat_alloc();
                // SAFETY: p live per above.
                return (b, unsafe { (*p).free_is_zero });
            }
        }
        self.malloc_aligned_at_slow(size, align, offset)
    }

    /// Everything `malloc_aligned_at`'s peek could not serve: an invalid or
    /// oversized alignment, a non-zero offset, a size past the small classes,
    /// a dry fast list, or a block whose address does not happen to satisfy
    /// the constraint. Cold and out of line so the peek carries none of its
    /// register needs.
    #[cold]
    #[inline(never)]
    pub(crate) fn malloc_aligned_at_slow(
        &mut self,
        size: usize,
        align: usize,
        offset: usize,
    ) -> (*mut u8, bool) {
        if !align.is_power_of_two() || align > crate::types::SEGMENT_SIZE / 2 {
            return (ptr::null_mut(), false);
        }
        if align <= 8 && offset == 0 {
            return self.malloc(size);
        }
        // NO PEEK HERE. Every caller of this function reaches it only
        // because an IDENTICAL peek has just failed — `malloc_aligned_at`
        // above, and `alloc::malloc_aligned_slow`, both run the same guard
        // against the same heap, the same page and the same free list, with
        // nothing in between. Repeating it could only fail again, so the copy
        // that used to sit here was pure cold-path tax.
        // Natural fit: only sound when the offset keeps block starts aligned,
        // and only as far as the page area itself is aligned — a slice,
        // hosted; `REGION_ALIGN` on a fixed region, where segments stride
        // from a `MAX_ALIGN_SIZE`-aligned base (`crate::REGION_STRIDES`).
        let natural = if crate::REGION_STRIDES {
            crate::prim::fixed::REGION_ALIGN
        } else {
            SEGMENT_SLICE_SIZE
        };
        if bins::is_aligned_to(offset, align) && size <= MEDIUM_OBJ_SIZE_MAX && align <= natural {
            // The bin is derived ONCE and handed on. `good_size(size)` is
            // `bin_size(bin(size))`, and the `malloc` it decides to call then
            // re-derived the very same bin to reach the same queue —
            // `bins::bin` is a leading-zeros and a pair of shifts, paid twice
            // per aligned allocation that lands here (29% of them on
            // `rptest`).
            let b = bins::bin(size);
            if bins::is_aligned_to(bins::bin_size(b), align) {
                return self.malloc_in_bin(size, b);
            }
            let asize = (size.max(1) + align - 1) & !(align - 1);
            let ab = bins::bin(asize);
            if bins::is_aligned_to(bins::bin_size(ab), align) {
                return self.malloc_in_bin(asize, ab);
            }
        }
        self.stats.generic += 1;
        // Huge sizes get exact placement in a dedicated segment.
        if size > LARGE_OBJ_SIZE_MAX {
            return self.huge_alloc(size, align, offset);
        }
        // Oversize-and-adjust: allocate size + align - 1, hand out the first
        // interior position satisfying the constraint.
        let oversize = size.max(1) + align - 1;
        if oversize > LARGE_OBJ_SIZE_MAX {
            return self.huge_alloc(size, align, offset);
        }
        let (block, is_zero) = self.malloc(oversize);
        if block.is_null() {
            return (ptr::null_mut(), false);
        }
        // `block + offset + align` can wrap for a caller-chosen offset: that
        // is an unsatisfiable request, and the block goes back, not out.
        let Some(addr) = bins::aligned_at_from(block.addr(), offset, align) else {
            // SAFETY: `block` was just allocated on this thread and has not escaped.
            unsafe { self.free_local(block) };
            return (ptr::null_mut(), false);
        };
        if addr < block.addr() {
            // SAFETY: as the overflow arm — block is ours and has not escaped.
            unsafe { self.free_local(block) };
            return (ptr::null_mut(), false);
        }
        let p = block.with_addr(addr);
        debug_assert!((p.addr() + offset).is_multiple_of(align));
        if p != block {
            // SAFETY: block is ours, just allocated on this thread; marking
            // the page before the pointer escapes (see Page::has_aligned).
            unsafe {
                let seg = segment_of(block);
                let pg = segment::page_of(seg, block);
                (*pg).flags.fetch_or(pflags::HAS_ALIGNED, Ordering::Relaxed);
            }
        }
        // A fresh-zero block is zero at every interior position too.
        (p, is_zero)
    }

    /// Process cross-thread delayed frees (the heartbeat's first duty).
    ///
    /// # Safety
    /// Caller must be this heap's owning thread.
    pub unsafe fn process_delayed(&mut self) {
        debug_assert!(!self.delayed.is_null(), "every heap has a delayed list");
        // SAFETY: delayed points into our own live HeapBox; blocks on it are
        // dead blocks of pages we own.
        unsafe {
            // A single-context build can never receive one. The list is fed
            // only by `remote_free`, which only a DIFFERENT thread reaches, so
            // on such a target the peek below is a load that always reads
            // zero and `drain_delayed` is 995 bytes the linker kept for it
            // (`docs/plans/finished/firmware-code-size.md`, lever 1).
            if crate::ONE_THREAD {
                debug_assert_eq!(
                    (*self.delayed).head.load(Ordering::Acquire),
                    0,
                    "a cross-thread free landed on a build that asserted a single thread"
                );
                return;
            }
            // Peek with a plain LOAD before the swap. This runs on every
            // slow-path allocation (the heartbeat's first duty), and on any
            // thread that never receives a cross-thread free — the common
            // single-threaded case, and the steady state of most others — the
            // list is empty. An atomic LOAD is a bare `mov`; the `swap` that
            // drains it is a LOCKed read-modify-write (~an order of magnitude
            // dearer, and it dirties the cache line). Only pay the swap when
            // there is actually a block to take. A push that races in after an
            // empty peek is simply processed on the next heartbeat — frees on
            // this list are drained at heartbeat cadence by design, never
            // synchronously, so deferring one is already the contract.
            if (*self.delayed).head.load(Ordering::Acquire) == 0 {
                return;
            }
            self.drain_delayed();
        }
    }

    /// The draining half of [`Heap::process_delayed`], out of line.
    ///
    /// The heartbeat runs on EVERY slow-path allocation and, on the common
    /// outcome, the peek above finds nothing and returns. But this loop calls
    /// `free_local`, and a call with values live across it is what forces the
    /// caller to preserve callee-saved registers — so the whole of
    /// `malloc_generic` paid the push/pop for a list that is almost always
    /// empty. On `cfrac` that is 2.56 M calls against an empty list.
    ///
    /// Splitting this off measured EXACTLY FLAT the first time it was tried,
    /// because `page_extend` was then also live-across in the same frame and
    /// held the registers on its own. It only pays once that one is gone —
    /// see `grow_front`.
    ///
    /// # Safety
    /// `self.delayed` is non-null and its head is non-zero.
    #[cold]
    #[inline(never)]
    unsafe fn drain_delayed(&mut self) {
        // SAFETY: forwarded contract — blocks on the list are dead blocks of
        // pages we own.
        unsafe {
            let mut b = (*self.delayed).head.swap(0, Ordering::AcqRel) as *mut Block;
            while !b.is_null() {
                let next = (*b).next;
                // (Calling `free_local_at` directly with the segment and page
                // resolved here, to skip `free_local`'s null test, measured
                // EXACTLY flat: LLVM already inlines `free_local` and proves
                // the block non-null from this loop's own condition.)
                self.free_local(b.cast());
                // Debug-only, like `allocs`/`frees` (`stat_alloc`) and
                // upstream's `MI_STAT`: a read-modify-write per drained block
                // on the cross-thread path, and nothing reads it but the stats
                // printer — no test, bench or probe (unlike `generic` and
                // `extends`, which the release bench prints as work-parity
                // counters and so stay live).
                #[cfg(debug_assertions)]
                {
                    self.stats.delayed_frees += 1;
                }
                b = next;
            }
        }
    }

    /// `mi_collect`: drain delayed frees and collect every queued page,
    /// retiring the empties (force is accepted for ABI shape; M4 treats all
    /// collects as forced).
    ///
    /// # Safety
    /// Caller must be this heap's owning thread.
    pub unsafe fn collect(&mut self, force: bool) {
        // SAFETY: forwarded contract. `force` reclaims, which is what
        // `mi_collect(true)` means for a LIVE heap.
        unsafe { self.collect_inner(force, force) }
    }

    /// Collect WITHOUT reclaiming orphans — for a heap that is being torn down.
    ///
    /// **This exists because reclaiming during teardown segfaulted 0.3.1.**
    /// `thread_done` and `heap_delete` both call a forced collect, and once
    /// `force` started adopting abandoned segments, a DYING thread would
    /// swallow every orphan from other dead threads into the heap it was about
    /// to destroy — re-homing their pages onto a `DelayedList` that is freed
    /// moments later. Reported by FFAI: 6/8 runs, and reproducible with five
    /// JPEG decodes.
    ///
    /// The tell was that MORE threads made it LESS frequent (1 thread 5/6,
    /// 16 threads 1/6): with more live heaps, orphans are adopted by a living
    /// thread first, leaving fewer for a dying one to take.
    ///
    /// # Safety
    /// As [`collect`](Self::collect).
    pub unsafe fn collect_for_teardown(&mut self) {
        // SAFETY: forwarded contract.
        unsafe { self.collect_inner(true, false) }
    }

    /// # Safety
    /// Owner thread; `reclaim` only when this heap will REMAIN live.
    unsafe fn collect_inner(&mut self, _force: bool, reclaim: bool) {
        // SAFETY: owner thread per contract.
        unsafe {
            // `force` RECLAIMS ABANDONED SEGMENTS. It used to be ignored
            // (`_force`), which made `mi_collect(true)` a per-heap page sweep
            // and nothing more — it never took back an orphan — and that is a
            // large part of why a caller's "trim" can measure ~0%.
            //
            // Orphans are adopted FIRST so the bin sweep below retires their
            // dead pages in the same pass.
            //
            // NOT ALSO PURGING HERE, and that was measured: purging every free
            // span on force crashed the test suite with an access violation
            // (0xC0000005). `span_free` only purges spans of
            // `len >= MEDIUM_PAGE_SLICES`, so purging smaller ones reaches
            // spans whose reuse path does not re-commit them — the M8 defect
            // (Windows MEM_DECOMMIT faults on touch where Linux MADV_DONTNEED
            // does not). A forced purge needs the recommit path audited first.
            // `reclaim`, NOT `force`: a teardown collect is forced but must
            // never adopt (see `collect_for_teardown`).
            if reclaim {
                // Bounded only as a stall guard; the list is normally short.
                const MAX_RECLAIM: usize = 1024;
                for _ in 0..MAX_RECLAIM {
                    let aseg = crate::init::abandoned_pop();
                    if aseg.is_null() {
                        break;
                    }
                    // Result ignored deliberately: `aseg` is never touched
                    // again on this path, so release-during-adoption is fine.
                    let _ = self.adopt_segment(aseg); // nosemgrep: discarded-lifecycle-result -- terminal, see comment above
                }
            }
            self.process_delayed();
            let mut bin = 1;
            // Running queue pointer: `self.pages[bin]` re-indexed the array
            // (and re-borrowed `self`) on every one of the MAX_NORMAL_BIN
            // steps, on every collect.
            //
            // REFUTED (2026-09-25): moving the per-queue body out of line so
            // this scan would keep its pointer in a register (inlined, the
            // body's calls spill it, and every EMPTY queue pays a reload and a
            // store: nine instructions per bin). The thread-exit workload
            // gained 272 instructions per thread, but the split changed
            // inlining around the generic path's periodic collect and every
            // opscan op got worse — `big`/`large` +4.00, `mixed` +2.89 — with
            // perl +45,287 and python +79,035 whole-program.
            let qbase: *mut PageQueue = (&raw mut self.pages).cast::<PageQueue>();
            let mut q: *mut PageQueue = qbase.add(1);
            while bin <= MAX_NORMAL_BIN {
                let mut p = (*q).first;
                while !p.is_null() {
                    let next = (*p).next;
                    if page_collect(p) {
                        self.saw_remote_free = true;
                    }
                    // An all-free page is freed HERE regardless of whether it
                    // is its bin's only one. That is upstream:
                    // `mi_heap_page_collect` calls `_mi_page_free` whenever
                    // `mi_page_all_free(page)`, at every collect level, with the
                    // comment "this will free retired pages as well". The
                    // keep-one-page-per-bin reuse cache is `mi_page_retire`'s
                    // policy, on the FREE path — not collect's.
                    //
                    // Ours borrowed that exemption into collect, so no collect
                    // at any level could return a size class's slice to a
                    // different class. Invisible at the shipped 32 MiB geometry
                    // (512 slices per segment absorb a cached page per class);
                    // fatal at the small profile's 16, where P4d watched 512 B
                    // capacity decay 168 -> 8 blocks and 22,533 of 50,000 churn
                    // allocations return null with 61,440 bytes of the region
                    // still free. docs/plans/small-metal.md §2.14.
                    if page_all_free(p) {
                        queue_remove(q, p);
                        self.update_direct(bin);
                        let seg = segment_of(p.cast::<u8>());
                        // `seg` is not touched again; `next` is a QUEUED page,
                        // and a released segment has no queued pages (release
                        // requires used_pages == 0).
                        let _ = self.retire_span(seg, p); // nosemgrep: discarded-lifecycle-result -- terminal, see comment above
                    }
                    p = next;
                }
                bin += 1;
                q = q.wrapping_add(1);
            }
        }
    }

    /// Adopt an abandoned segment: take ownership, re-home every live page,
    /// retire the already-dead ones (called with the segment OFF the global
    /// abandoned list, thread_id already set to us by the caller).
    ///
    /// **Returns `false` when `seg` was RELEASED during adoption** — it is
    /// unmapped, and the caller must not touch the pointer again. Adoption
    /// releases in two cases: a Huge segment whose block had already died, and
    /// a Normal segment that arrives empty when this heap already caches an
    /// empty one. Both are common in an abandon/adopt storm.
    ///
    /// This is `#[must_use]` because ignoring it is a use-after-free, not a
    /// missed optimisation: `span_from_segments` did exactly that and read
    /// `(*seg).used_pages` off a freshly-`munmap`ped 32 MiB region. It survived
    /// on x86-64 only because the address space there tended to stay mapped;
    /// on aarch64-apple-darwin it faults immediately (19 of 20 stress runs).
    ///
    /// # Safety
    /// `seg` must be a Normal segment popped from the abandoned list, owned
    /// by nobody; calling thread becomes the owner.
    #[must_use = "returns false when the segment was released — using it then is a use-after-free"]
    pub unsafe fn adopt_segment(&mut self, seg: *mut Segment) -> bool {
        // SAFETY: we are the sole owner as of now; walking spans follows the
        // segment invariants (every span start is marked).
        unsafe {
            if (*seg).kind == SegmentKind::Huge {
                // Single-block segment: re-home, collect any NEVER-pushed
                // free, release if its block already died.
                let pg: *mut Page = &raw mut (*seg).pages[1];
                (*pg).xheap.store(self.delayed as usize, Ordering::Release);
                page_collect_and_set_flag(pg, XFLAG_DELAYED);
                self.stats.reclaims += 1;
                if (*pg).used == 0 {
                    let _ = huge_free(seg);
                    self.stats.segments_freed += 1;
                    return false; // RELEASED — `seg` is unmapped from here
                }
                (*seg).next = self.huge_segments;
                self.huge_segments = seg;
                return true;
            }
            (*seg).next = self.segments;
            self.segments = seg;
            self.stats.reclaims += 1;
            let end = (*seg).next_free_slice as usize;
            let mut idx = segment::HEADER_SLICES;
            // Whether walk 1 saw a span that walk 2 could possibly act on. Walk
            // 2 only ever retires a BIN_HUGE span, and BIN_HUGE > MAX_NORMAL_BIN,
            // so every candidate it could find passes through the `else` arm
            // below. Workloads that never allocate a large single-block span —
            // which is most of them — skip the second full slice walk entirely.
            let mut saw_large_span = false;
            // Walk by a RUNNING POINTER, not by re-indexing `pages[idx]` every
            // step. `Page` is 88 bytes — not a power of two — so each index
            // costs a multiply plus the array's bounds check; advancing by
            // `len` costs one add. The walk visits every carved slice, so this
            // is paid on each one.
            let base: *mut Page = (&raw mut (*seg).pages).cast::<Page>();
            let mut slot: *mut Page = base.add(idx);
            while idx < end {
                let len = ((*slot).slice_count as usize).max(1);
                if (*slot).block_size > 0 {
                    (*slot)
                        .xheap
                        .store(self.delayed as usize, Ordering::Release);
                    // `bin` is loaded ONCE: it decides the branch below and
                    // then indexes the queue, and it was being re-read from the
                    // page for the second use.
                    let bin = (*slot).bin as usize;
                    if bin <= MAX_NORMAL_BIN {
                        // NOT load-guarded. REFUTED 2026-08-21: wrapping this
                        // in `if flags & IN_FULL != 0` costs +1.97M Ir (+4.5% of
                        // adopt) — exactly one instruction per page — because
                        // `fetch_and` is a LEAF op with nothing to skip, so the
                        // peek only adds a load and a branch. Same shape as the
                        // `fetch_or(HAS_ALIGNED)` sibling rejected in
                        // docs/opps.md #9. It IS a latency win (no lock prefix
                        // on the common path); this project's metric is Ir.
                        (*slot).flags.fetch_and(!pflags::IN_FULL, Ordering::Relaxed);
                        // `next`/`prev` are NOT cleared here: `queue_push_front`
                        // below writes both unconditionally (`prev = null`,
                        // `next = q.first`), and nothing between here and there
                        // reads either. Clearing them first was two dead stores
                        // on every live page of every adopted segment.
                        page_collect_and_set_flag(slot, XFLAG_NORMAL);
                        // Do NOT retire empty pages during this walk:
                        // span_free COALESCES, rewriting the span layout we
                        // are iterating, so the next step can land mid-span
                        // and queue a bogus page (rare corrupting race found
                        // by the M8 parallel gate). Empty pages are queued
                        // like any other and retired by the next collect.
                        queue_push_front(&raw mut self.pages[bin], slot);
                    } else {
                        // Large single-block span: stays unqueued; a dead one
                        // is retired by a later collect, same rule as above.
                        page_collect_and_set_flag(slot, XFLAG_DELAYED);
                        saw_large_span = true;
                    }
                }
                idx += len;
                // `wrapping_add`, not `add`: the pointer is only ever
                // DEREFERENCED while `idx < end` holds, which the span-partition
                // invariant makes in-bounds — but the final advance of the loop
                // computes a pointer that may be one past the carved region, and
                // forming that with `add` would be UB even unread.
                slot = slot.wrapping_add(len);
            }
            // Queue fronts changed wholesale — rebuild the small-bin table.
            // REFUTED 2026-08-21: narrowing this to the bins the segment
            // actually touched, by tracking a min/max bin in the walk above,
            // is an Ir REGRESSION of +3.75M (+8.6% of adopt). The two compares
            // it adds run once per QUEUED PAGE (~1.97M times over this
            // benchmark) while the whole loop they save runs once per SEGMENT
            // and costs ~444k. Per-page work to save per-segment work is the
            // wrong direction here; bounding the loop's END (below) is not.
            let mut bin = 1;
            while bin <= MAX_DIRECT_BIN {
                self.update_direct(bin);
                bin += 1;
            }
            // Adopted large spans that are already dead: retire them now that
            // the walk is over (the layout is ours to mutate again).
            let mut idx = segment::HEADER_SLICES;
            while saw_large_span && idx < (*seg).next_free_slice as usize {
                let slot: *mut Page = &raw mut (*seg).pages[idx];
                let len = ((*slot).slice_count as usize).max(1);
                if (*slot).block_size > 0
                    && (*slot).slice_offset == 0
                    && (*slot).bin as usize == BIN_HUGE
                    && (*slot).used == 0
                {
                    // Retiring this span can empty the segment and RELEASE it —
                    // in which case `seg` is unmapped and the `used_pages` read
                    // below would fault. This was the residual aarch64 crash:
                    // EXC_BAD_ACCESS in adopt_segment's own tail.
                    if !self.retire_span(seg, slot) {
                        return false; // RELEASED during adoption
                    }
                    break; // layout changed: leave the rest to later collects
                }
                idx += len;
            }
            if (*seg).used_pages == 0 {
                if self.empty_segments == 0 {
                    self.empty_segments = 1;
                } else if self.remove_segment(seg) {
                    // Releasing here is sound: `used_pages` counts CARVED spans
                    // and a page is only queued while carved, so `used_pages ==
                    // 0` implies none of this segment's pages are still in our
                    // bin queues. Release only what we actually unlinked.
                    let _ = segment::segment_free(seg);
                    self.stats.segments_freed += 1;
                    return false; // RELEASED — `seg` is unmapped from here
                }
            }
            true
        }
    }

    /// Free a block owned by THIS heap's thread. Null is a no-op.
    ///
    /// # Safety
    /// `p` must be null or a live pointer whose segment is owned by the
    /// calling thread (or a Huge segment); routing is `alloc::free`'s job.
    pub unsafe fn free_local(&mut self, p: *mut u8) {
        if p.is_null() {
            return;
        }
        let seg = segment_of(p);
        // SAFETY: forwarded contract; resolve the page once for the callee.
        unsafe {
            let pg = segment::page_of(seg, p);
            self.free_local_at(seg, pg, p)
        }
    }

    /// Work-parity counters, present only in debug builds.
    ///
    /// This mirrors upstream exactly: mimalloc defines `MI_STAT 2` when
    /// `MI_DEBUG > 0` and **`MI_STAT 0` otherwise**, so the release oracle we
    /// benchmark against carries no counters at all. Ours were unconditional,
    /// which both cost real instructions on the hottest paths in the allocator
    /// and made every published ratio an unfair comparison — a counters-on
    /// build measured against a counters-off one. They stay on for `cargo
    /// test` (a debug profile), which is where the counters do their job:
    /// proving two binaries perform identical work.
    #[inline(always)]
    fn stat_alloc(&mut self) {
        #[cfg(debug_assertions)]
        {
            self.stats.allocs += 1;
        }
    }

    #[inline(always)]
    fn stat_free(&mut self) {
        #[cfg(debug_assertions)]
        {
            self.stats.frees += 1;
        }
    }

    /// The tail of a local free that emptied its page: unqueue and retire the
    /// span, unless this is the bin's only page (keep one warm — approximates
    /// upstream's retire delay).
    ///
    /// This is the ONLY part of a fast-path free that needs the owning heap —
    /// upstream's `mi_free_block_local` touches none — and it runs on a small
    /// fraction of frees, so `alloc::free` inlines the rest and calls here
    /// only when a page actually empties.
    ///
    /// Stays `#[inline]`, and that was MEASURED. Marking it `#[cold]` +
    /// `#[inline(never)]` — on the theory that inlining its tree (`retire_span`
    /// -> `span_free` -> `segment_free`) was what forced `alloc::free` to save
    /// callee-saved registers on the fast path — made things WORSE: perl
    /// 1.0068 -> 1.0084, sqlite 1.0041 -> 1.0048, and `free`'s prologue gained
    /// back the `push r15` it was supposed to remove.
    ///
    /// # Safety
    /// `pg` is a queued, now-empty binned page of `seg`, owned by this heap.
    #[inline]
    pub unsafe fn retire_emptied(&mut self, seg: *mut Segment, pg: *mut Page) {
        // SAFETY: forwarded contract.
        unsafe {
            let bin = (*pg).bin as usize;
            let q: *mut PageQueue = &raw mut self.pages[bin];
            // NOTE (RSS investigation, 2026-08-06): deferring this retire —
            // keeping the emptied page queued so the next round reuses the SAME
            // memory instead of first-fitting a span elsewhere — was tried and
            // measured NO CHANGE to peak RSS (62.8 vs 62.6 MiB). Span re-carve
            // churn is real (504 pages retired and re-carved per round) but it
            // is NOT the +18% gap. Reverted; do not re-try without a new
            // mechanism.
            // The keep-one-warm test is NOT repeated here. Both callers —
            // `free`'s emptied branch and `retire_or_abort` — have already
            // established that this page is not its queue's sole member,
            // answering it from the page's own links without touching the
            // heap, so re-deriving `q` to ask again is provably dead.
            debug_assert!(
                !((*q).first == pg && (*q).last == pg),
                "retire_emptied: sole-member page should have been kept warm by the caller"
            );
            queue_remove(q, pg);
            self.update_direct(bin);
            let _ = self.retire_span(seg, pg); // `seg` unused after
        }
    }

    /// Free with the segment ALREADY resolved. `alloc::free` computes the
    /// segment (and page) to route ownership; recomputing them here cost a
    /// mask + shift + two loads on every free for nothing (M9 brick #2,
    /// byte-identical behaviour).
    ///
    /// # Safety
    /// As [`free_local`], with `seg == segment_of(p)`.
    pub unsafe fn free_local_at(&mut self, seg: *mut Segment, pg: *mut Page, p: *mut u8) {
        debug_assert_eq!(seg, segment_of(p));
        // SAFETY: caller resolved this page already.
        debug_assert!(pg.is_null() || unsafe { pg == segment::page_of(seg, p) });
        // SAFETY: p is ours per the contract, so seg is a live segment header.
        unsafe {
            // NOTE (2026-08-21): passing the caller's already-read `flags`
            // byte in as a fifth argument — to route this `match` on
            // `HUGE_SEGMENT` and to spare the IN_FULL re-read below — measured
            // **+6.05 Ir/op** on the cross-thread proxy, and +6.06 with only
            // the IN_FULL half. It is the ARGUMENT that costs: `free_general`
            // gained two (`seg`, `pg`) for −7.01 because the caller had them
            // live already, but a fifth here pushes this larger function over
            // a register-allocation cliff. The M9 "threading all five through
            // measured worse" result, reproduced at a different arity.
            // REFUTED (2026-08-22): splitting the huge-segment arm, the
            // un-park and the retire out as `#[cold]` tails, so the common
            // path would carry nothing live across a call. It removes the
            // frame — 6.91 Ir, a third of this function — and it is still a
            // LOSS: `free_local_at` +215,880 on the cross-thread op and
            // cfrac +232,152. The arms are not rare where this function is
            // hot. A cross-thread free lands on the owner's delayed list, and
            // when the owner drains it the pages it touches are exactly the
            // ones that were parked or have just emptied — so the outlined
            // call fires on most of them. Third time this document records it:
            // "cold" is a property of the WORKLOAD, not of the code.
            match (*seg).kind {
                SegmentKind::Huge => {
                    self.stat_free();
                    if self.remove_huge_segment(seg) {
                        let _ = huge_free(seg); // terminal: seg is gone after this
                    }
                }
                SegmentKind::Normal => {
                    self.stat_free();
                    if (*pg).bin as usize == BIN_HUGE {
                        let _ = self.retire_span(seg, pg); // returns below
                        return;
                    }
                    let used_now = page_push_local(pg, p.cast());
                    if (used_now as i32) < 0 {
                        crate::page::double_free_abort();
                    }
                    if (*pg).flags.load(Ordering::Relaxed) & pflags::IN_FULL != 0 {
                        (*pg).flags.fetch_and(!pflags::IN_FULL, Ordering::Relaxed);
                        queue_remove(&raw mut self.pages[BIN_FULL], pg);
                        page_set_flag(pg, XFLAG_NORMAL);
                        let bin = (*pg).bin as usize;
                        // `update_direct` is needed ONLY when the queue was
                        // empty, because that is the only way a push to the
                        // BACK can move the head the table tracks.
                        let q: *mut PageQueue = &raw mut self.pages[bin];
                        let was_empty = (*q).first.is_null();
                        queue_push_back(q, pg);
                        if was_empty {
                            self.update_direct(bin);
                        }
                    }
                    // REFUTED (2026-08-22): `page_all_free` is `used == 0`
                    // and `used_now` already holds that value, so this re-read
                    // looks free to remove. It is +147,456 on the cross-thread
                    // op — keeping `used_now` live across the un-park arm's
                    // queue calls costs a register, and re-loading a field
                    // that is already hot in L1 costs less than spilling.
                    if page_all_free(pg) {
                        let bin = (*pg).bin as usize;
                        let q: *mut PageQueue = &raw mut self.pages[bin];
                        // `first == pg` is also exactly the condition under
                        // which `queue_remove` moves the head.
                        let was_first = (*q).first == pg;
                        if !(was_first && (*q).last == pg) {
                            queue_remove(q, pg);
                            if was_first {
                                self.update_direct(bin);
                            }
                            let _ = self.retire_span(seg, pg); // `seg` unused after
                        }
                    }
                }
            }
        }
    }

    /// Return a span to its segment; release the segment when it empties.
    ///
    /// **Returns `false` when the SEGMENT was released** — `seg` is unmapped and
    /// the caller must not touch it again. Retiring the last span of a segment
    /// is the normal way a segment dies, so this is a routine outcome, not an
    /// error path.
    ///
    /// # Safety
    /// Heap lock held; `pg` is an unqueued span-start of `seg` with no live
    /// blocks.
    #[must_use = "false means the segment was released — touching it then is a use-after-free"]
    unsafe fn retire_span(&mut self, seg: *mut Segment, pg: *mut Page) -> bool {
        // SAFETY: forwarded contract.
        unsafe {
            // span_free coalesces, then purges the merged span (the RSS
            // lever) and reports whether it did.
            if span_free(seg, pg) {
                self.stats.purges += 1;
            }
            self.stats.pages_retired += 1;
            if (*seg).used_pages == 0 {
                if self.empty_segments == 0 {
                    // Park one empty segment for reuse (stays in the list —
                    // span_from_segments will find its full free span).
                    self.empty_segments = 1;
                } else if self.remove_segment(seg) {
                    // Release ONLY what we actually unlinked. If it was not in
                    // our list it belongs to someone else (or was already
                    // released) and freeing it would dangle their pointer; the
                    // rightful owner will retire it.
                    let _ = segment::segment_free(seg);
                    self.stats.segments_freed += 1;
                    return false; // RELEASED — `seg` is unmapped from here
                }
            }
            true
        }
    }

    /// Unlink `seg` from the heap's segment list. **Returns whether it was
    /// actually found and unlinked.**
    ///
    /// A `false` here means `seg` is not ours — so it must NOT be released.
    /// This used to be a bare `debug_assert!(false, …)`, which is compiled out
    /// in release: the caller then fell through and `segment_free`d a segment
    /// that was still linked in some list, leaving a dangling pointer behind
    /// (the residual aarch64 crash in `thread_done`'s walk). Returning the fact
    /// makes "release only what we unlinked" enforceable at each call site
    /// instead of assumed.
    ///
    /// # Safety
    /// Heap lock held.
    #[must_use = "false means the segment is not ours — releasing it corrupts another list"]
    unsafe fn remove_segment(&mut self, seg: *mut Segment) -> bool {
        // SAFETY: list owned by this heap under the lock.
        unsafe {
            let mut cur = &raw mut self.segments;
            while !(*cur).is_null() {
                if *cur == seg {
                    *cur = (*seg).next;
                    return true;
                }
                cur = &raw mut (**cur).next;
            }
            debug_assert!(false, "segment not in heap list"); // nosemgrep: debug-assert-false-as-error-path -- diagnostic; the `false` return below is what callers act on
            false
        }
    }

    // realloc/expand/usable_size moved to `alloc` in M4: they operate on
    // blocks that may be owned by OTHER threads, so they must not require
    // `&mut` on any particular heap.

    /// `mi_heap_visit_blocks` core: walk every live page area of this heap
    /// (Normal spans + huge blocks); optionally enumerate allocated blocks.
    /// The visitor returns false to stop; the return mirrors that.
    ///
    /// # Safety
    /// Owner thread.
    pub unsafe fn visit_blocks(
        &mut self,
        visit_blocks: bool,
        f: &mut dyn FnMut(&AreaInfo, *mut u8, usize) -> bool,
    ) -> bool {
        // SAFETY: owner thread; collect keeps free-list snapshots exact.
        unsafe {
            let mut seg = self.segments;
            while !seg.is_null() {
                if !visit_segment_blocks(seg, -1, visit_blocks, true, f) {
                    return false;
                }
                seg = (*seg).next;
            }
            let mut seg = self.huge_segments;
            while !seg.is_null() {
                if !visit_segment_blocks(seg, -1, visit_blocks, true, f) {
                    return false;
                }
                seg = (*seg).next;
            }
        }
        true
    }

    /// `mi_unsafe_heap_page_is_under_utilized`.
    ///
    /// # Safety
    /// Owner thread; `p` a live pointer of this heap.
    pub unsafe fn page_under_utilized(&mut self, p: *mut u8, perc: usize) -> bool {
        let seg = segment_of(p);
        // SAFETY: p ours per contract.
        unsafe {
            let pg = segment::page_of(seg, p);
            page_collect(pg);
            // Saturating: a huge caller `perc` is "always under", not a debug
            // overflow or a wrapped comparison.
            ((*pg).used as usize).saturating_mul(100)
                < ((*pg).capacity as usize).saturating_mul(perc)
        }
    }

    /// Refresh the direct table for `bin` to its current queue front.
    fn update_direct(&mut self, bin: usize) {
        // The queue already CARRIES its block size (`PageQueue::block_size`,
        // filled once at heap construction). Re-deriving it through
        // `bins::bin_size`'s shift-and-mask arithmetic on every call was work
        // the struct had already done — and the load is free, because the very
        // next line touches `self.pages[bin]` anyway.
        let bsize = self.pages[bin].block_size;
        if bsize > SMALL_SIZE_MAX {
            return;
        }
        // An empty queue publishes the sentinel, not null, so `malloc` never
        // has to test for a missing page.
        let page = match self.pages[bin].first {
            p if p.is_null() => crate::page::empty_page_ptr(),
            p => p,
        };
        let w_hi = bsize / crate::types::INTPTR_SIZE;
        // Nothing to do when the range already points at this page. Every
        // entry in `[w_lo, w_hi]` is written together by the loop below and
        // this function is their only writer, so the top of the range answers
        // for all of it in one load. The steady state — a bin whose queue
        // front does not change between two slow-path allocations — then costs
        // a compare instead of the `w_lo` derivation plus a store per wsize.
        if self.direct[w_hi] == page {
            return;
        }
        // With ALIGN2W, bins ≤ 8 exist only at 1 and even indices; each even
        // bin also serves the odd wsize below it. Bins > 8 are contiguous.
        let w_lo = match bin {
            0 | 1 => 0,
            2 => 2,
            3..=8 => bin - 1,
            // Stored, not recomputed — same reason as `bsize` above.
            _ => self.pages[bin - 1].block_size / crate::types::INTPTR_SIZE + 1,
        };
        // REFUTED (larson-sized, 2026-08-21): this loop is 32.17 Ir per queue
        // walk — 21% of the walk — because the wide bins span up to 32 word
        // sizes and it writes every slot. Both slice forms that would drop the
        // per-element bound are WORSE, on an instrument exact to the unit:
        //
        //     indexed `while`                   53.105 Ir/op   (this)
        //     `self.direct[w_lo..=w_hi].fill()` 53.360   +0.255
        //     `for slot in &mut self.direct[..]` 53.375  +0.270
        //
        // LLVM already compiles the indexed form without a per-store check;
        // what the slice forms add is the range's own bounds computation. Do
        // not retry without changing the DATA STRUCTURE — the cost here is the
        // width of a bin's word-size range, not the loop.
        let mut w = w_lo;
        while w <= w_hi {
            self.direct[w] = page;
            w += 1;
        }
    }
}

/// Area descriptor handed to block visitors (mirrors `mi_heap_area_t`).
pub struct AreaInfo {
    /// Start of the page's block area.
    pub blocks: *mut u8,
    /// Bytes reserved for this area.
    pub reserved: usize,
    /// Bytes currently usable (committed; == reserved under eager commit).
    pub committed: usize,
    /// Allocated blocks in the area.
    pub used: usize,
    /// Block size.
    pub block_size: usize,
    /// Block size including padding/metadata (== block_size here).
    pub full_block_size: usize,
    /// The owning heap's tag at stamp time.
    pub heap_tag: i32,
}

/// Visit every live page of ONE segment; optionally each allocated block.
/// `tag_filter >= 0` skips pages with a different heap tag. `owner` selects
/// exact snapshots (collect) vs read-only walks (abandoned segments).
///
/// # Safety
/// `seg` live; if `owner`, calling thread owns it; else it must be pinned
/// (e.g. the abandoned-list lock is held).
pub unsafe fn visit_segment_blocks(
    seg: *mut Segment,
    tag_filter: i32,
    visit_blocks: bool,
    owner: bool,
    f: &mut dyn FnMut(&AreaInfo, *mut u8, usize) -> bool,
) -> bool {
    // SAFETY: per contract; span walk follows the segment invariants.
    unsafe {
        if (*seg).kind == SegmentKind::Huge {
            let pg: *mut Page = &raw mut (*seg).pages[1];
            if (*pg).used == 0 || (tag_filter >= 0 && (*pg).heap_tag != tag_filter) {
                return true;
            }
            let bsize = (*pg).block_size;
            let block = seg.cast::<u8>().add((*seg).total_size - bsize);
            let area = AreaInfo {
                blocks: block,
                reserved: bsize,
                committed: bsize,
                used: 1,
                block_size: bsize,
                full_block_size: bsize,
                heap_tag: (*pg).heap_tag,
            };
            if !f(&area, ptr::null_mut(), 0) {
                return false;
            }
            if visit_blocks && !f(&area, block, bsize) {
                return false;
            }
            return true;
        }
        let end = (*seg).next_free_slice as usize;
        let mut idx = segment::HEADER_SLICES;
        while idx < end {
            let slot: *mut Page = &raw mut (*seg).pages[idx];
            let len = ((*slot).slice_count as usize).max(1);
            if (*slot).block_size == 0 || (tag_filter >= 0 && (*slot).heap_tag != tag_filter) {
                idx += len;
                continue;
            }
            if owner {
                page_collect(slot);
            }
            let bsize = (*slot).block_size;
            let area_ptr = page_area(seg, idx);
            let area = AreaInfo {
                blocks: area_ptr,
                reserved: len * SEGMENT_SLICE_SIZE,
                committed: len * SEGMENT_SLICE_SIZE,
                used: (*slot).used as usize,
                block_size: bsize,
                full_block_size: bsize,
                heap_tag: (*slot).heap_tag,
            };
            if !f(&area, ptr::null_mut(), 0) {
                return false;
            }
            if visit_blocks && (*slot).used > 0 {
                // Free-mark bitmap over capacity blocks (≤ 8192 → 1 KiB stack).
                let cap = (*slot).capacity as usize;
                let mut freemap = [0u64; 128];
                // A SINGLE_BLOCK page's `block_size` is `slices *
                // SEGMENT_SLICE_SIZE`, which carries no bin geometry, so
                // `div_by_block_size` does not apply — but such a page holds
                // exactly one block, at offset 0, so it needs no division at
                // all. Hoisted out of the closure: one flags load for the whole
                // walk in place of a `div` per block.
                let single = (*slot).flags.load(Ordering::Relaxed) & pflags::SINGLE_BLOCK != 0;
                // Bounds-checked marking: a link that does not resolve to a
                // block of THIS page means the list is corrupt — skip it
                // rather than index the bitmap out of range.
                let mut mark = |b: *mut Block| {
                    let addr = b.addr();
                    if addr < area_ptr.addr() {
                        return;
                    }
                    let off = if single {
                        0
                    } else {
                        // `b` comes off a free list, so it is a block START:
                        // the offset is an exact multiple of `bsize` and the
                        // cheaper inverse applies. The `off < cap` bound below
                        // still catches a corrupt link either way.
                        crate::bins::exact_div_by_block_size(addr - area_ptr.addr(), bsize)
                    };
                    if off < cap {
                        freemap[off / 64] |= 1 << (off % 64);
                    }
                };
                // Links must be read through block_next: they are ENCODED in
                // secure builds (reading them raw here indexed the bitmap
                // with garbage — found by the M8 parallel gate).
                let mut b = (*slot).free;
                while !b.is_null() {
                    mark(b);
                    b = block_next(slot, b);
                }
                let mut b = (*slot).local_free;
                while !b.is_null() {
                    mark(b);
                    b = block_next(slot, b);
                }
                // Cross-thread chain: snapshot the head; nodes are stable
                // once pushed (never unlinked until a collect).
                let x = (*slot).xthread_free.load(Ordering::Acquire);
                let mut b = crate::ptr_with_addr(slot.cast::<Block>(), x & !crate::page::XMASK);
                while !b.is_null() {
                    mark(b);
                    b = block_next(slot, b);
                }
                for i in 0..cap {
                    if freemap[i / 64] & (1 << (i % 64)) == 0 {
                        let block = area_ptr.add(i * bsize);
                        if !f(&area, block, bsize) {
                            return false;
                        }
                    }
                }
            }
            idx += len;
        }
        true
    }
}

/// Push `page` at the queue front.
///
/// # Safety
/// Heap lock held; `page` not currently in any queue.
unsafe fn queue_push_front(q: *mut PageQueue, page: *mut Page) {
    // SAFETY: caller contract — page is a live slot.
    unsafe { crate::page::debug_validate_page(page, "queue_push_front") };
    // SAFETY: caller contract.
    unsafe {
        (*page).prev = ptr::null_mut();
        (*page).next = (*q).first;
        if (*q).first.is_null() {
            (*q).last = page;
        } else {
            (*(*q).first).prev = page;
        }
        (*q).first = page;
    }
}

/// Append `page` to the BACK of `q`.
///
/// The un-park path's enqueue. Keeping the queue HEAD stable is the whole
/// point — see the call site in [`Heap::free_local_at`].
///
/// # Safety
/// Heap lock held; `page` is a live slot not currently linked in any queue.
unsafe fn queue_push_back(q: *mut PageQueue, page: *mut Page) {
    // SAFETY: caller contract — page is a live slot.
    unsafe { crate::page::debug_validate_page(page, "queue_push_back") };
    // SAFETY: caller contract.
    unsafe {
        (*page).next = ptr::null_mut();
        (*page).prev = (*q).last;
        if (*q).last.is_null() {
            (*q).first = page;
        } else {
            (*(*q).last).next = page;
        }
        (*q).last = page;
    }
}

/// Unlink `page` from its queue.
///
/// # Safety
/// Heap lock held; `page` currently linked in `q`.
unsafe fn queue_remove(q: *mut PageQueue, page: *mut Page) {
    // SAFETY: caller contract.
    unsafe {
        if (*page).prev.is_null() {
            (*q).first = (*page).next;
        } else {
            (*(*page).prev).next = (*page).next;
        }
        if (*page).next.is_null() {
            (*q).last = (*page).prev;
        } else {
            (*(*page).next).prev = (*page).prev;
        }
        (*page).next = ptr::null_mut();
        (*page).prev = ptr::null_mut();
    }
}

#[cfg(test)]
mod unlink_tests {
    use super::*;

    /// The 0.4.0 use-after-free family, pinned as a REGRESSION TEST rather
    /// than only as a type signature.
    ///
    /// `remove_huge_segment` must answer truthfully about whether it unlinked
    /// anything, because its caller uses that answer to decide whether to
    /// RELEASE the segment. When it returned `()`, a failed unlink was
    /// invisible in release and the caller freed the segment regardless,
    /// leaving a dangling head in whatever list still held it — defect #3 of
    /// the 0.4.0 family. That was fixed for NORMAL segments at the time and
    /// survived on the huge path until a semgrep rule written from the same
    /// incident found it (2026-08-19).
    ///
    /// The ledger's standing complaint about that era is that the fixes were
    /// *reasoned* but never *reproduced* (`teardown_reclaim.rs` still passes
    /// with its bug deliberately reintroduced). This reproduces the decision.
    ///
    /// No real memory is needed, and that is a property of the shape rather
    /// than a shortcut: when the walk finds nothing it never dereferences the
    /// candidate, so a distinctive address stands in for "a segment belonging
    /// to another heap". Building a genuine 32 MiB huge segment would test the
    /// allocator, not the decision.
    #[test]
    fn unlink_reports_whether_it_actually_unlinked() {
        let mut h = Heap::new();
        assert!(
            h.huge_segments.is_null(),
            "a fresh heap should own no huge segments"
        );

        // --- not ours: must report false, and must not touch the list ---
        let foreign = ptr::without_provenance_mut::<Segment>(0x5EE0_BAD0);
        // SAFETY: the list is empty, so the walk terminates immediately and
        // `foreign` is compared but never read through.
        let unlinked = unsafe { h.try_unlink_huge_segment(foreign) };
        assert!(
            !unlinked,
            "claimed to unlink a segment that was never in the list — the              caller would now free it (0.4.0 defect #3)"
        );
        assert!(h.huge_segments.is_null(), "the empty list was mutated");
    }
}
