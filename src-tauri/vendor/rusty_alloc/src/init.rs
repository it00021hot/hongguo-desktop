//! Thread + heap lifecycle (mirrors upstream `init.c` + the abandonment side
//! of `arena-abandon.c`).
//!
//! Every thread gets a lazily-created [`HeapBox`] (one OS page from the prim
//! layer — never from the allocator itself, so bootstrap can't recurse). The
//! fast path reaches it through a const-init `thread_local!` `Cell` pointer,
//! which the R1 spike measured at atomic-load parity (0.3 ns) — no dtor is
//! registered on that key (Cell is !Drop), so accessing it never allocates.
//!
//! Thread EXIT runs through the prim TLS destructor (`FlsAlloc` /
//! `pthread_key_create`, built in M1 for exactly this): drain delayed frees,
//! retire what died, transition every surviving page to `NEVER` (spinning out
//! in-flight `FREEING` remotes — the loom-modeled teardown order), zero the
//! segments' thread id, and publish them on the global abandoned list. Any
//! other thread's allocation slow path adopts them later.

use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

use crate::heap::Heap;
use crate::page::{
    DelayedList, XFLAG_NEVER, page_collect, page_collect_and_set_flag, page_set_flag,
};
use crate::segment::{self, Segment};
use crate::{os, prim};

/// A heap plus the cross-thread fields remote threads may touch. Remote
/// threads only ever dereference `delayed` (via page `xheap` addresses) —
/// never the heap itself — which is what makes `&mut` on the inner heap sound.
#[repr(C)]
pub struct HeapBox {
    /// Cross-thread delayed-free list (remote-reachable). MUST stay the first
    /// field: pages store `&delayed` in `xheap`, and owner routing recovers
    /// the box by container-of (delayed at offset 0 ⇒ the addresses coincide).
    pub delayed: DelayedList,
    /// Next box in the global registry (guarded by the registry lock).
    /// AtomicPtr, not usize: reachability (miri's leak check and honesty
    /// about what is cached vs leaked) follows pointers, not integers.
    pub next_box: AtomicPtr<HeapBox>,
    /// Owning thread id (heaps allocate only from their creating thread).
    pub owner_tid: usize,
    /// The subprocess this heap's segments are abandoned into.
    ///
    /// Mirrored from the `SUBPROC` thread-local so that TEARDOWN never has to
    /// read it. `thread_done` runs from a platform TLS destructor
    /// (`pthread_key_create` / `FlsAlloc`), and the destruction order between
    /// *that* callback and Rust's own `thread_local!` storage is unspecified on
    /// every platform. On aarch64-apple-darwin it is observably wrong:
    /// `my_subproc()` reads back 0 inside `thread_done`, so a tagged thread's
    /// segments landed in the MAIN subprocess and `mi_subproc_*` isolation was
    /// silently lost — the abandon fired correctly, only its destination was
    /// wrong. The heap box is our own mapping and is alive for the whole of
    /// teardown (it is the value passed *to* the destructor), so reading the tag
    /// from here is correct on every target instead of accidentally correct on
    /// one.
    pub subproc: AtomicUsize,
    /// Heap tag (`mi_heap_new_ex`; reported by the block visitor).
    pub tag: i32,
    /// Whether `mi_heap_destroy` may drop live blocks (else it delegates to
    /// delete semantics).
    pub allow_destroy: bool,
    /// The owner-only heap state.
    pub heap: UnsafeCell<Heap>,
}

const _: () = assert!(core::mem::offset_of!(HeapBox, delayed) == 0);

/// Wrapper so the empty sentinel box can be a `static`.
#[repr(transparent)]
pub struct EmptyHeapBox(HeapBox);

// SAFETY: the sentinel is never written and never escapes to remote threads:
// no page ever stores its `delayed` address in `xheap` (pages are re-homed
// only by REAL heaps), its `owner_tid` of 0 matches no live thread, and every
// reader goes through raw-pointer READS — `heap_box_fast` documents that no
// `&mut` may ever be formed to it. It exists purely so the malloc fast path
// has a valid heap to read before this thread has created one.
unsafe impl Sync for EmptyHeapBox {}

/// The "no heap yet" sentinel: every `direct` slot holds the empty PAGE
/// sentinel, so a fresh thread's first malloc falls through to the generic
/// path — which detects this box and performs the real thread init — without
/// the fast path ever testing for it. This is upstream's `_mi_heap_empty`
/// trick, and the exact design our own [`crate::page::EMPTY_PAGE`] already
/// uses one level down: replace a null-test on the hot path with a sentinel
/// whose data routes the miss to the slow path for free.
///
/// `export_name` (not mangled) because the x86-64 Linux TLS slot's `.tdata`
/// initializer below names it in assembly; `.hidden` there keeps it out of
/// the cdylib's export table.
///
/// **In flash on a one-region target.** The sentinel is read and never
/// written — `heap_box_fast` documents that contract, the fast path only
/// writes after a non-null pop and every `direct` slot here points at the
/// empty page whose free list is null, and no page ever carries its address
/// in `xheap`. Its type has interior mutability (a `DelayedList`, an
/// `AtomicPtr`, the `UnsafeCell<Heap>`), which is why rustc places it in
/// `.data` by default: 1,752 bytes paid once in flash for the image and once
/// more in RAM for the loader's copy, the largest single item left on the
/// ESP32-S3 after 2.0.2 and RAM the linker takes from the stack. Placing it
/// in `.rodata.*` keeps the flash copy and drops the RAM one. Any write
/// would fault loudly against the flash cache, which is the right way for
/// the contract to fail. Bare metal only — a hosted target may need to
/// relocate the pointers inside it at load, and has RAM to spare.
#[cfg_attr(
    all(
        ra_single_threaded,
        not(miri),
        not(windows),
        not(unix),
        not(target_arch = "wasm32")
    ),
    unsafe(link_section = ".rodata.rusty_alloc_empty_heap_box")
)]
#[unsafe(export_name = "__ra_empty_heap_box")]
pub static EMPTY_HEAP_BOX: EmptyHeapBox = EmptyHeapBox(HeapBox {
    delayed: DelayedList::new(),
    next_box: AtomicPtr::new(ptr::null_mut()),
    owner_tid: 0,
    subproc: AtomicUsize::new(0),
    tag: 0,
    allow_destroy: false,
    heap: UnsafeCell::new(Heap::new()),
});

/// Pointer to the shared empty heap box.
#[inline]
pub const fn empty_heap_box_ptr() -> *mut HeapBox {
    (&raw const EMPTY_HEAP_BOX.0).cast_mut()
}

/// Recover the owning HeapBox from a page's `xheap` value (the container-of
/// trick over the offset-0 delayed list).
///
/// # Safety
/// `xheap` must be a live HeapBox's delayed-list address (nonzero).
#[inline]
pub unsafe fn box_of_xheap(xheap: usize) -> *mut HeapBox {
    debug_assert!(xheap != 0);
    xheap as *mut HeapBox
}

// Global registry of live heap boxes: keeps every heap (and through it every
// segment) reachable-from-static — the process-wide stats walk of M7, and
// what makes miri's reachability-based leak check reflect reality (an
// allocator's arenas at process exit are cached state, not leaks).
static HEAPS_LOCK: AtomicBool = AtomicBool::new(false);
static HEAPS_HEAD: AtomicPtr<HeapBox> = AtomicPtr::new(ptr::null_mut());

fn heaps_lock() {
    while HEAPS_LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn heaps_unlock() {
    HEAPS_LOCK.store(false, Ordering::Release);
}

fn heaps_register(hb: *mut HeapBox) {
    heaps_lock();
    // SAFETY: hb is fully initialized and not yet published.
    unsafe {
        (*hb)
            .next_box
            .store(HEAPS_HEAD.load(Ordering::Relaxed), Ordering::Relaxed);
    }
    HEAPS_HEAD.store(hb, Ordering::Relaxed);
    heaps_unlock();
}

fn heaps_unregister(hb: *mut HeapBox) {
    heaps_lock();
    let mut cur = HEAPS_HEAD.load(Ordering::Relaxed);
    if cur == hb {
        // SAFETY: links guarded by the lock.
        unsafe {
            HEAPS_HEAD.store((*hb).next_box.load(Ordering::Relaxed), Ordering::Relaxed);
        }
    } else {
        while !cur.is_null() {
            // SAFETY: links guarded by the lock; nodes are live boxes.
            unsafe {
                let nxt = (*cur).next_box.load(Ordering::Relaxed);
                if nxt == hb {
                    (*cur)
                        .next_box
                        .store((*nxt).next_box.load(Ordering::Relaxed), Ordering::Relaxed);
                    break;
                }
                cur = nxt;
            }
        }
    }
    heaps_unlock();
}

/// Unlink and return one live heap box owned by `tid`, or null.
///
/// The lock is released before return so the caller can abandon the heap
/// without re-entering `HEAPS_LOCK` (abandon walks pages and can allocate).
fn take_heap_owned_by(tid: usize) -> *mut HeapBox {
    heaps_lock();
    let mut prev: *mut HeapBox = ptr::null_mut();
    let mut cur = HEAPS_HEAD.load(Ordering::Relaxed);
    while !cur.is_null() {
        // SAFETY: links and boxes are live under the lock.
        unsafe {
            let nxt = (*cur).next_box.load(Ordering::Relaxed);
            if (*cur).owner_tid == tid {
                if prev.is_null() {
                    HEAPS_HEAD.store(nxt, Ordering::Relaxed);
                } else {
                    (*prev).next_box.store(nxt, Ordering::Relaxed);
                }
                heaps_unlock();
                return cur;
            }
            prev = cur;
            cur = nxt;
        }
    }
    heaps_unlock();
    ptr::null_mut()
}

/// Storage for the calling thread's default-heap pointer.
///
/// On x86-64 Linux this is a RAW ELF TLS symbol reached with the
/// **initial-exec** model rather than `thread_local!`'s general-dynamic. That
/// was the last `__tls_get_addr` call on the malloc fast path — 7.3 M
/// instructions on perl, a third of everything still separating us from
/// mimalloc after the M10b free-path bricks. Stable Rust has no
/// `-Z tls-model`, so the symbol is declared in `global_asm!` and the linker
/// resolves its offset.
///
/// **Why this is sound where a thread-pointer-keyed side table is not.** The
/// storage IS the thread's own TLS block. Every thread gets a fresh block
/// initialised from the `.tdata` template (the empty-heap sentinel) when it
/// is created, so a recycled TCB cannot hand a new thread a dead thread's
/// heap — the staleness
/// question that made the keyed-cache design a P0 risk simply does not arise
/// here. What initial-exec costs instead is a LOAD-TIME constraint: it needs a
/// slot in the static TLS block, so a `dlopen` long after startup could fail
/// to load us. That fails loudly at load time rather than corrupting memory,
/// and it is the same trade upstream mimalloc ships
/// (`__attribute__((tls_model("initial-exec")))` on `_mi_heap_default`).
#[cfg(all(target_arch = "x86_64", target_os = "linux", not(miri)))]
mod heap_tls {
    use super::HeapBox;

    // An 8-byte thread-local slot. `.tdata` (not `.tbss`): the slot is
    // INITIALISED to the empty-heap sentinel, so every new thread reads a
    // valid heap pointer from its very first malloc and the fast path needs
    // no "not yet created" test at all — the sentinel's empty direct table
    // routes the first allocation to the generic path, which performs the
    // real init there (upstream's `_mi_heap_empty` design). The loader
    // relocates the `.quad` in the TLS template once at load time; every
    // thread's block is then copied from that template. `.hidden` makes both
    // symbols non-preemptible, which is what permits the initial-exec
    // (GOTTPOFF) relocation instead of a general-dynamic call, and keeps the
    // sentinel out of the cdylib's export table.
    core::arch::global_asm!(
        ".section .tdata,\"awT\",@progbits",
        ".globl __ra_tls_heap",
        ".hidden __ra_tls_heap",
        ".hidden __ra_empty_heap_box",
        ".type __ra_tls_heap,@object",
        ".p2align 3",
        "__ra_tls_heap:",
        ".quad __ra_empty_heap_box",
        ".size __ra_tls_heap,8",
        ".text",
    );

    /// Address of THIS thread's slot: the thread pointer plus the offset the
    /// dynamic linker wrote into the GOT.
    #[inline(always)]
    fn slot() -> *mut *mut HeapBox {
        let off: usize;
        // SAFETY: reads the GOT entry the dynamic linker relocated at load
        // time, before any of our code can run, and never writes. It is
        // constant for the life of the process, hence `pure` + `readonly` —
        // that is what lets LLVM hoist and CSE the address computation
        // instead of treating the block as an optimisation barrier.
        unsafe {
            core::arch::asm!(
                "mov {o}, qword ptr [rip + __ra_tls_heap@GOTTPOFF]",
                o = out(reg) off,
                options(nostack, preserves_flags, readonly, pure),
            );
        }
        // `thread_id()` IS the fs base on this target. TLS offsets are
        // negative there (the block grows down from the thread pointer), so
        // this adds a two's-complement offset and must wrap.
        core::ptr::with_exposed_provenance_mut(super::thread_id().wrapping_add(off))
    }

    /// Read this thread's heap pointer in TWO instructions.
    ///
    /// The obvious form — `slot().read()` — costs FOUR: load the offset from
    /// the GOT, read the thread pointer from `fs:0`, add them, dereference.
    /// But x86 performs that addition IN THE ADDRESSING MODE: `fs:[reg]` is a
    /// segment-relative load, so the explicit `fs:0` read and the add both
    /// disappear. Measured at 4.00 Ir per malloc before this change — a
    /// quarter of our entire malloc-side deficit spent just locating the heap.
    #[inline(always)]
    pub fn get() -> *mut HeapBox {
        let v: *mut HeapBox;
        // SAFETY: reads the GOT entry the loader relocated at startup, then
        // loads 8 aligned bytes of this thread's own TLS block through the fs
        // segment. Both are reads; nothing is written.
        unsafe {
            core::arch::asm!(
                "mov {t}, qword ptr [rip + __ra_tls_heap@GOTTPOFF]",
                "mov {o}, qword ptr fs:[{t}]",
                t = out(reg) _,
                o = out(reg) v,
                options(nostack, preserves_flags, readonly),
            );
        }
        v
    }

    #[inline(always)]
    pub fn set(hb: *mut HeapBox) {
        // SAFETY: as `get` — the slot is owned exclusively by this thread.
        unsafe { slot().write(hb) }
    }
}

/// Every other target keeps the `thread_local!`: Windows TLS is an index into
/// a per-thread array (no `__tls_get_addr` to remove), and miri models the
/// macro but not a hand-rolled TLS symbol.
#[cfg(not(all(target_arch = "x86_64", target_os = "linux", not(miri))))]
mod heap_tls {
    use super::HeapBox;
    use core::cell::Cell;

    ra_thread_local! {
        /// Fast-path heap pointer. Const-init + !Drop ⇒ plain TLS access, no
        /// lazy-init branch, no allocation ever. Initialised to the
        /// empty-heap SENTINEL (never null) so the malloc fast path can read
        /// through it unconditionally; see `EMPTY_HEAP_BOX`.
        static HEAP_PTR: Cell<*mut HeapBox> = const { Cell::new(super::empty_heap_box_ptr()) };
    }

    #[inline(always)]
    pub fn get() -> *mut HeapBox {
        HEAP_PTR.with(|c| c.get())
    }

    #[inline(always)]
    pub fn set(hb: *mut HeapBox) {
        HEAP_PTR.with(|c| c.set(hb));
    }
}

ra_thread_local! {
    /// Cached OS thread id. `free` needs the calling thread's id on EVERY
    /// call to route local-vs-remote; the raw `prim::thread_id()` is a libc
    /// call (`pthread_self` through the PLT from a cdylib) measured at
    /// 1.41 ns vs 0.25 ns for this cache — ~18-20% of a whole malloc+free
    /// pair (M9 probe, both platforms). 0 = not yet cached; no real thread id
    /// is 0 on either platform.
    static TID: Cell<usize> = const { Cell::new(0) };
}

/// The calling thread's id — read STRAIGHT FROM THE THREAD-POINTER REGISTER
/// where the target allows it.
///
/// Why this and not `thread_local!`: the shipping artifact is a **cdylib**
/// (LD_PRELOAD). Rust's `thread_local!` in a shared library compiles to the
/// general-dynamic TLS model, so every access is a CALL into `ld.so`'s
/// `__tls_get_addr` — the per-function instruction profile put that call at
/// 12.97 M Ir, 1.96% of a whole lua run, comparable to our entire `free`.
/// (The earlier ns-level probe missed it because it measured TLS inside an
/// EXECUTABLE, where the model is local-exec: a register offset, no call.
/// Measure the artifact you ship.)
///
/// A ceiling probe with `-Z tls-model=initial-exec` valued removing those
/// calls at 2.00% of our instructions and 33% of the gap to mimalloc; this
/// gets the same effect on stable, for the id, with no linker involvement —
/// the thread pointer is one register read. It is exactly what mimalloc's
/// `_mi_thread_id()` does.
///
/// Uniqueness: the value is the thread-control-block address, unique among
/// LIVE threads. It can be recycled after a thread dies, which is sound here
/// for the same reason it is in mimalloc: a dying thread abandons its
/// segments (`thread_id` stored as 0) before its TCB can be reused, so a
/// recycled id can never match a stale segment.
#[inline(always)]
pub fn thread_id() -> usize {
    #[cfg(all(target_arch = "x86_64", target_os = "linux", not(miri)))]
    {
        let tp: usize;
        // SAFETY: reads the TCB self-pointer at fs:0, which the x86-64 Linux
        // ABI guarantees is present and thread-unique. No memory is written.
        unsafe {
            core::arch::asm!("mov {}, fs:0", out(reg) tp,
                             options(nostack, preserves_flags, readonly));
        }
        tp
    }
    #[cfg(all(target_arch = "x86_64", target_os = "windows", not(miri)))]
    {
        let teb: usize;
        // SAFETY: gs:0x30 is the TEB self-pointer on x86-64 Windows —
        // present in every thread, unique, read-only here.
        unsafe {
            core::arch::asm!("mov {}, gs:0x30", out(reg) teb,
                             options(nostack, preserves_flags, readonly));
        }
        teb
    }
    // AArch64 splits by OS: the register holding the thread pointer is NOT the
    // same one on Darwin as everywhere else.
    #[cfg(all(target_arch = "aarch64", not(target_vendor = "apple"), not(miri)))]
    {
        let tp: usize;
        // SAFETY: on the standard AArch64 ABI (Linux, Android, BSD) tpidr_el0
        // IS the thread pointer; read-only.
        unsafe {
            core::arch::asm!("mrs {}, tpidr_el0", out(reg) tp,
                             options(nostack, preserves_flags, readonly));
        }
        tp
    }
    #[cfg(all(target_arch = "aarch64", target_vendor = "apple", not(miri)))]
    {
        let tp: usize;
        // SAFETY: read-only system-register read.
        //
        // DARWIN IS NOT THE STANDARD AArch64 ABI. Apple puts the thread pointer
        // in tpidrRO_el0 and uses tpidr_el0 for the CPU/cluster id. Reading
        // tpidr_el0 here — as every other AArch64 target correctly does — was a
        // memory-safety bug, measured on macOS 26 / M-series:
        //
        //   * tpidr_el0 returns small values (0x1002, 0x2005, …), not pointers;
        //   * it CHANGES under a single thread as that thread migrates between
        //     cores (5 distinct values over 3M reads on the main thread);
        //   * and 8 live threads produced only 5 distinct values — DIFFERENT
        //     THREADS COLLIDED.
        //
        // thread_id() is the allocator's ownership identity: `segment.thread_id`
        // decides whether a free takes the local (unsynchronised) path or the
        // remote one. Colliding ids let one thread free into another thread's
        // segment through the owner path, racing non-atomic page state — heap
        // corruption, not merely a lost optimisation. A drifting id separately
        // makes a thread stop recognising its own segments.
        //
        // tpidrro_el0 is the real thread pointer: stable within a thread and
        // unique across live threads (it is `pthread_self() + 0xe0`). Apple
        // documents the LOW 3 BITS as the current CPU number, so they must be
        // masked off — that is precisely the drift observed above.
        unsafe {
            core::arch::asm!("mrs {}, tpidrro_el0", out(reg) tp,
                             options(nostack, preserves_flags, readonly));
        }
        tp & !0b111
    }
    // Every other target: the cached-TLS path (still far cheaper than asking
    // the OS on each call).
    #[cfg(not(any(
        all(target_arch = "x86_64", target_os = "linux", not(miri)),
        all(target_arch = "x86_64", target_os = "windows", not(miri)),
        all(target_arch = "aarch64", not(miri))
    )))]
    {
        let t = TID.with(|c| c.get());
        if t != 0 { t } else { init_tid() }
    }
}

#[cold]
#[cfg_attr(
    any(
        all(target_arch = "x86_64", target_os = "linux", not(miri)),
        all(target_arch = "x86_64", target_os = "windows", not(miri)),
        all(target_arch = "aarch64", not(miri))
    ),
    allow(dead_code)
)]
fn init_tid() -> usize {
    let t = prim::thread_id();
    TID.with(|c| c.set(t));
    t
}

/// Snapshot-visit every registered heap (stats aggregation). The visitor
/// receives a COPY of the heap's counters.
///
/// **This read is racy by design and `volatile` does not make it less so** —
/// clippy's `volatile_composites` is right to flag it, and the lint is
/// allowed HERE rather than workspace-wide so the reasoning stays attached to
/// the one site that does it. `read_volatile` on a composite gives no
/// per-field atomicity and no ordering, so a concurrent owner mutating its
/// counters can yield a snapshot whose fields disagree with each other. That
/// is accepted: these are `#[cfg(debug_assertions)]` diagnostic counters
/// (upstream's `MI_STAT` rule), nothing branches on them, and the alternative
/// — taking `&` to a struct another thread is writing — would be UB rather
/// than merely imprecise. The real fix is making the counters atomics, which
/// is the tracked follow-up (R-005 in the hardening audit).
#[allow(
    clippy::volatile_composites,
    reason = "racy diagnostic snapshot; see the doc comment. Tracked as R-005."
)]
pub fn for_each_heap(f: &mut dyn FnMut(&crate::heap::Heap)) {
    heaps_lock();
    let mut hb = HEAPS_HEAD.load(Ordering::Acquire);
    while !hb.is_null() {
        // SAFETY: registry entries are live boxes. The read is unsynchronised
        // with the owning thread's writes (see above) but stays within a live,
        // correctly-aligned allocation for the whole of the copy.
        unsafe {
            let snapshot = core::ptr::read_volatile((*hb).heap.get());
            f(&snapshot);
            hb = (*hb).next_box.load(Ordering::Acquire);
        }
    }
    heaps_unlock();
}

/// Get (or create) the calling thread's heap box. NULL when creation fails
/// (memory exhaustion) — callers translate that into their own failure value
/// (a null allocation, a no-op collect); the TLS slot keeps the sentinel so a
/// later call retries.
#[inline]
pub fn heap_box() -> *mut HeapBox {
    let hb = heap_tls::get();
    if hb == empty_heap_box_ptr() {
        init_thread_heap()
    } else {
        hb
    }
}

/// This thread's heap box WITHOUT the created-yet test — may return the
/// shared empty sentinel. For the malloc fast path only.
///
/// # Safety (for callers)
/// The returned box may be [`EMPTY_HEAP_BOX`], which is shared between every
/// uninitialised thread: callers must perform RAW-POINTER READS ONLY and must
/// never form a `&mut` (or write) through it. Route any miss to
/// [`ensure_heap`] before mutating anything.
#[inline]
pub fn heap_box_fast() -> *mut HeapBox {
    heap_tls::get()
}

/// Upgrade a possibly-sentinel box from [`heap_box_fast`] to this thread's
/// real, initialised heap box. NULL when creation fails (see [`heap_box`]).
#[inline]
pub fn ensure_heap(hb: *mut HeapBox) -> *mut HeapBox {
    if hb == empty_heap_box_ptr() {
        init_thread_heap()
    } else {
        hb
    }
}

/// Allocate + initialize a HeapBox (shared by thread bootstrap and
/// `mi_heap_new*`). Registered in the global registry; NOT installed in TLS.
///
/// Returns NULL when the OS refuses the mapping (memory exhaustion). This
/// used to `expect()` — which, under the release profile's `panic = "abort"`,
/// turned OOM into a process abort on a path reachable from any fresh
/// thread's first `malloc`. The malloc contract on OOM is a null return, and
/// upstream returns NULL from `mi_heap_new` for the same reason (H-18/R-003
/// in the hardening audit).
pub fn create_heap(tag: i32, allow_destroy: bool, arena_id: i32) -> *mut HeapBox {
    // First-class heaps are not stored in `done_slot`. A thread that only
    // ever called `mi_heap_new` / `heap_malloc` never ran `init_thread_heap`,
    // so the TLS destructor never fired and leftover heaps stayed DELAYED
    // (OH-rusty_alloc-13). Bootstrap the backing heap (and the exit hook)
    // first. `init_thread_heap` calls `create_heap_uninstalled`, not this
    // function, so there is no recursion.
    if !crate::ONE_THREAD && heap_tls::get() == empty_heap_box_ptr() {
        let _ = heap_box();
    }
    create_heap_uninstalled(tag, allow_destroy, arena_id)
}

fn create_heap_uninstalled(tag: i32, allow_destroy: bool, arena_id: i32) -> *mut HeapBox {
    let size = core::mem::size_of::<HeapBox>();
    // On a one-region target the FIRST descriptor is a static of the fixed
    // backend's, so that a firmware's region is whole segments and needs no
    // page carved out of it — the page is what made an aligned container of
    // the region pad itself out to the next segment
    // (`docs/plans/finished/region-alignment-bug.md` §7). Every later heap,
    // and every heap on a hosted target, takes a page as before.
    let first = if crate::ONE_REGION {
        prim::fixed::take_first_heap_box()
    } else {
        None
    };
    let hb: *mut HeapBox = match first {
        Some(p) => p,
        None => {
            let Ok(block) = os::alloc_aligned(size, os::page_size(), true, false) else {
                return ptr::null_mut();
            };
            block.ptr.cast()
        }
    };
    // SAFETY: fresh committed mapping large enough for HeapBox; we initialize
    // every field before publishing the pointer.
    unsafe {
        ptr::write(
            hb,
            HeapBox {
                delayed: DelayedList::new(),
                next_box: AtomicPtr::new(ptr::null_mut()),
                owner_tid: thread_id(),
                // Captured HERE, on the live creating thread, where the
                // thread-local is unambiguously valid.
                subproc: AtomicUsize::new(my_subproc()),
                tag,
                allow_destroy,
                // A new heap starts as a copy of the empty one. On a
                // one-region target that copy is taken from the sentinel
                // itself, which already holds the whole template in flash;
                // materialising `Heap::new()` here as well cost a second
                // ~900-byte `.rodata` blob for the bin table. Hosted builds
                // keep the constant, whose codegen was measured in Ir.
                heap: UnsafeCell::new(if crate::ONE_REGION {
                    // SAFETY (within the enclosing block): a raw,
                    // non-overlapping read of the never-written sentinel's
                    // heap — the only access `heap_box_fast`'s contract
                    // permits, and `Heap` owns nothing that a bitwise copy
                    // could double-free.
                    ptr::read((*empty_heap_box_ptr()).heap.get())
                } else {
                    Heap::new()
                }),
            },
        );
        (*(*hb).heap.get()).delayed = &raw const (*hb).delayed;
        (*(*hb).heap.get()).arena_id = arena_id;
        (*(*hb).heap.get()).tag = tag;
        // Per-heap CSPRNG stream (free-list keys, guarded sampling) -- seeded
        // only where something draws from it. A build with neither `secure`
        // nor guard pages has no consumer, and seeding still cost the ChaCha
        // block function plus the splitmix fallback in flash.
        if crate::RNG_USED {
            (*(*hb).heap.get()).rng.reseed();
        }
        // Guarded objects follow the option table (MI_GUARDED-equivalent) --
        // where a guard page can exist at all. `GUARD_PAGES` first, so that on
        // a target without one the option reads fold away with the sampler
        // (`docs/plans/finished/firmware-code-size.md`, lever 3).
        let rate = crate::options::get(33).max(0) as usize; // guarded_sample_rate
        let gmin = crate::options::get(30).max(0) as usize; // guarded_min
        let gmax = crate::options::get(31).max(0) as usize; // guarded_max
        if crate::GUARD_PAGES && gmax > 0 {
            (*(*hb).heap.get()).guarded_set_size_bound(gmin, gmax);
            (*(*hb).heap.get())
                .guarded_set_sample_rate(rate, crate::options::get(34).max(0) as usize);
        }
    }
    heaps_register(hb);
    hb
}

#[cold]
fn init_thread_heap() -> *mut HeapBox {
    let hb = create_heap_uninstalled(0, false, -1);
    if hb.is_null() {
        // OOM: install nothing — the TLS slot keeps the sentinel, so the
        // NEXT allocation retries creation instead of dereferencing null.
        return ptr::null_mut();
    }
    heap_tls::set(hb);
    BACKING_PTR.with(|c| c.set(hb));
    // The exit hook exists to abandon this heap when its thread ENDS. On a
    // single-context build no thread ends, the backend's TLS never runs a
    // destructor anyway (`prim::fixed::tls_new` discards it), and creating
    // the slot was a compare-and-swap, a spin and a table entry paid once for
    // nothing (`docs/plans/finished/firmware-code-size.md`, lever 2).
    if !crate::ONE_THREAD {
        done_slot().set(hb.cast::<c_void>());
    }
    hb
}

ra_thread_local! {
    /// The thread's original (backing) heap — what `mi_heap_get_backing`
    /// returns regardless of `mi_heap_set_default` swaps.
    static BACKING_PTR: Cell<*mut HeapBox> = const { Cell::new(ptr::null_mut()) };
}

/// `mi_heap_get_backing`.
pub fn backing_heap() -> *mut HeapBox {
    let b = BACKING_PTR.with(|c| c.get());
    if b.is_null() {
        let _ = heap_box(); // bootstraps and sets BACKING_PTR
        BACKING_PTR.with(|c| c.get())
    } else {
        b
    }
}

/// `mi_heap_set_default`: install `hb` as this thread's default; returns the
/// previous default.
///
/// # Safety
/// `hb` must be a live heap owned by the calling thread.
pub unsafe fn set_default_heap(hb: *mut HeapBox) -> *mut HeapBox {
    let prev = heap_box();
    heap_tls::set(hb);
    prev
}

/// The process-wide TLS slot whose destructor abandons a dying thread's heap.
fn done_slot() -> prim::TlsSlot {
    static RAW: AtomicUsize = AtomicUsize::new(0);
    static INIT: AtomicBool = AtomicBool::new(false);
    let raw = RAW.load(Ordering::Acquire);
    if raw != 0 {
        // SAFETY: RAW holds an into_raw of a slot created below.
        return unsafe { prim::TlsSlot::from_raw(raw - 1) };
    }
    if INIT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        // A panic here would be wrong twice over. An allocator must not unwind
        // into its C callers (which is why the release profile is
        // panic=abort), AND the winner of this race is the only thread that
        // ever publishes RAW — so unwinding past the store below would strand
        // every other thread in the spin loop FOREVER, turning a rare
        // resource failure into a silent process-wide hang. Fail loudly and
        // deterministically instead, identically in debug and release.
        let Some(slot) = prim::TlsSlot::new(Some(thread_done_cb)) else {
            crate::abort();
        };
        RAW.store(slot.into_raw() + 1, Ordering::Release); // +1: 0 is the unset sentinel
    }
    loop {
        let raw = RAW.load(Ordering::Acquire);
        if raw != 0 {
            // SAFETY: as above.
            return unsafe { prim::TlsSlot::from_raw(raw - 1) };
        }
        core::hint::spin_loop();
    }
}

#[cfg(all(windows, not(miri)))]
unsafe extern "system" fn thread_done_cb(v: *const c_void) {
    if !v.is_null() {
        // SAFETY: v is the HeapBox this thread stored in the done slot.
        unsafe { thread_done(v.cast_mut().cast()) };
    }
}

#[cfg(any(not(windows), miri))]
unsafe extern "C" fn thread_done_cb(v: *mut c_void) {
    if !v.is_null() {
        // SAFETY: v is the HeapBox this thread stored in the done slot.
        unsafe { thread_done(v.cast()) };
    }
}

/// Abandon the dying thread's heap. Teardown order is the loom-modeled one:
/// (1) every surviving page → NEVER (spins out FREEING remotes), (2) THEN
/// drain the delayed list, (3) THEN release the heap storage.
///
/// # Safety
/// Must run on the dying thread with `hb` its live heap box, exactly once.
pub unsafe fn thread_done(hb: *mut HeapBox) {
    // SAFETY: per contract `hb` is this thread's live box; `owner_tid` is a
    // plain field.
    let tid = unsafe { (*hb).owner_tid };
    // SAFETY: per contract we are the owner and this runs exactly once;
    // sequencing per the model.
    unsafe { thread_done_one(hb) };
    // Back to the SENTINEL, not null — the slot is never null, so a
    // post-teardown malloc re-enters the generic path and re-initialises.
    heap_tls::set(empty_heap_box_ptr());
    // First-class heaps this thread created are not stored in `done_slot`
    // (`set_default_heap` does not update it). Leaving them DELAYED meant a
    // second remote free after thread exit returned (OH-rusty_alloc-13).
    // Abandon every remaining heap we still own so pages go NEVER. Iterative:
    // a dying thread can own many first-class heaps; do not recurse.
    if !crate::ONE_THREAD {
        // Stall guard: a dying thread that keeps re-initialising (empty TLS
        // malloc) must not loop forever. Same order as `collect_inner`'s
        // `MAX_RECLAIM`.
        const MAX_OWNED_HEAPS: usize = 1 << 20;
        for _ in 0..MAX_OWNED_HEAPS {
            let extra = take_heap_owned_by(tid);
            if extra.is_null() {
                break;
            }
            // SAFETY: extra is a live box this dying thread still owns.
            unsafe { thread_done_one(extra) };
        }
    }
}

/// Abandon one heap box (NEVER + delayed drain + unregister + free the box).
///
/// # Safety
/// `hb` is a live heap box owned by the calling thread, not used again.
unsafe fn thread_done_one(hb: *mut HeapBox) {
    // SAFETY: per contract we are the owner; sequencing per the model.
    unsafe {
        // Read the subprocess tag from the BOX, not from `my_subproc()`. We are
        // inside a platform TLS destructor and Rust's `thread_local!` storage
        // may already be torn down — on aarch64-apple-darwin it reads back 0,
        // which silently dropped every tagged thread's segments into the main
        // subprocess. Clamped because a corrupt tag must not index out of
        // bounds on the teardown path.
        let sp = (*hb).subproc.load(Ordering::Acquire).min(MAX_SUBPROCS - 1);
        let h = &mut *(*hb).heap.get();
        // Retire everything that is already dead (also drains delayed once).
        // TEARDOWN variant: must NOT adopt orphans into a heap we are about to
        // destroy — that was the 0.3.1 segfault.
        h.collect_for_teardown();
        // Walk remaining segments: transition pages to NEVER, disown, publish.
        let mut seg = h.segments;
        h.segments = ptr::null_mut();
        while !seg.is_null() {
            let next = (*seg).next;
            let end = (*seg).next_free_slice as usize;
            let mut idx = segment::HEADER_SLICES;
            // Running pointer, not `pages[idx]`: `Page` is not a power-of-two
            // size, so indexing costs a multiply plus a bounds check on every
            // carved slice of every segment a dying thread owns.
            let base: *mut crate::page::Page = (&raw mut (*seg).pages).cast();
            let mut slot: *mut crate::page::Page = base.add(idx);
            while idx < end {
                let len = ((*slot).slice_count as usize).max(1);
                if (*slot).block_size > 0 {
                    // ONE locked read-modify-write, not two: taking the
                    // cross-thread chain and publishing NEVER are the same
                    // word, and doing them separately paid two CAS loops per
                    // live page of every segment a dying thread owns.
                    page_collect_and_set_flag(slot, XFLAG_NEVER);
                    (*slot).xheap.store(0, Ordering::Release);
                }
                idx += len;
                // `wrapping_add`, not `add`: the pointer is only ever
                // DEREFERENCED while `idx < end` holds, which the span-partition
                // invariant makes in-bounds — but the final advance of the loop
                // computes a pointer that may be one past the carved region, and
                // forming that with `add` would be UB even unread.
                slot = slot.wrapping_add(len);
            }
            if (*seg).used_pages == 0 {
                let _ = segment::segment_free(seg);
                h.stats.segments_freed += 1;
            } else {
                (*seg).thread_id.store(0, Ordering::Release);
                abandoned_push(seg, sp);
            }
            seg = next;
        }
        // Huge segments: same teardown shape (collect NEVER-bound frees,
        // release dead ones, abandon the rest).
        let mut hseg = h.huge_segments;
        h.huge_segments = ptr::null_mut();
        while !hseg.is_null() {
            let next = (*hseg).next;
            let pg = &raw mut (*hseg).pages[1];
            page_collect(pg);
            if (*pg).used == 0 {
                let _ = segment::huge_free(hseg);
                h.stats.segments_freed += 1;
            } else {
                page_set_flag(pg, XFLAG_NEVER);
                (*pg).xheap.store(0, Ordering::Release);
                (*hseg).thread_id.store(0, Ordering::Release);
                abandoned_push(hseg, sp);
            }
            hseg = next;
        }
        // Post-NEVER drain: blocks a FREEING remote landed while we walked.
        // Their pages are abandoned now, so free_local would be wrong — but
        // these blocks belong to pages we JUST abandoned; push them back on
        // their pages' own lists (flag NEVER routes them there).
        let mut b = (*hb).delayed.head.swap(0, Ordering::AcqRel) as *mut crate::page::Block;
        while !b.is_null() {
            let next = (*b).next;
            let seg = segment::segment_of(b.cast());
            let pg = segment::page_of(seg, b.cast());
            crate::page::remote_free(pg, b);
            b = next;
        }
        // Release the heap storage and clear the fast-path pointer. The
        // first descriptor on a one-region target is a static, not a page:
        // nothing to return (and this path is unreachable there anyway).
        heaps_unregister(hb);
        if !(crate::ONE_REGION && prim::fixed::is_first_heap_box(hb)) {
            let size = core::mem::size_of::<HeapBox>();
            let blockdesc = os::OsBlock {
                ptr: hb.cast(),
                size: os::page_align_up(size),
                is_large: false,
                is_zero: false,
            };
            let _ = os::free(blockdesc);
        }
    }
}

/// `mi_heap_delete`: free the heap structure, MIGRATING its live pages and
/// segments into the thread's backing heap (blocks stay valid).
///
/// # Safety
/// `hb` must be a live first-class heap owned by the calling thread, not the
/// backing heap, used never again.
pub unsafe fn heap_delete(hb: *mut HeapBox) {
    let backing = backing_heap();
    if hb == backing {
        return; // deleting the backing heap is a no-op (upstream-compatible)
    }
    // SAFETY: owner thread per contract; absorb everything into backing.
    unsafe {
        debug_assert_eq!((*hb).owner_tid, thread_id());
        let h = &mut *(*hb).heap.get();
        let bh = &mut *(*backing).heap.get();
        // Teardown: this heap is being deleted, so it must not adopt orphans.
        h.collect_for_teardown();
        let mut seg = h.segments;
        h.segments = ptr::null_mut();
        while !seg.is_null() {
            let next = (*seg).next;
            // `next` is read BEFORE adopting and `seg` is not touched after, so
            // a release during adoption is safe here.
            let _ = bh.adopt_segment(seg); // re-homes queues + xheap (tid ours)
            seg = next;
        }
        let mut hseg = h.huge_segments;
        h.huge_segments = ptr::null_mut();
        while !hseg.is_null() {
            let next = (*hseg).next;
            let pg = &raw mut (*hseg).pages[1];
            (*pg).xheap.store(bh.delayed as usize, Ordering::Release);
            (*hseg).next = bh.huge_segments;
            bh.huge_segments = hseg;
            hseg = next;
        }
        // Fold the dying heap's counters into backing so process totals hold.
        merge_stats(&mut bh.stats, &h.stats);
        release_heap_box(hb);
    }
}

/// `mi_heap_destroy`: drop EVERY block of the heap at once and release its
/// memory. Falls back to delete semantics when the heap was created with
/// `allow_destroy = false`.
///
/// # Safety
/// As [`heap_delete`], and additionally no block of this heap may be used
/// afterwards (the C contract).
pub unsafe fn heap_destroy(hb: *mut HeapBox) {
    // SAFETY: owner thread per contract.
    unsafe {
        if !(*hb).allow_destroy {
            heap_delete(hb);
            return;
        }
        debug_assert_eq!((*hb).owner_tid, thread_id());
        let h = &mut *(*hb).heap.get();
        // Spin out any in-flight FREEING remotes before the box dies; blocks
        // they queued are dying with the heap, so the delayed list is simply
        // discarded.
        let mut seg = h.segments;
        while !seg.is_null() {
            let end = (*seg).next_free_slice as usize;
            let mut idx = segment::HEADER_SLICES;
            // Same running-pointer walk as `thread_done`'s — the sibling site,
            // checked because a defect in one slice walk is a defect in every
            // slice walk. Not exercised by `mleak` (this is `heap_destroy`, not
            // thread teardown), so it carries no measurement of its own; it is
            // the identical transformation on identical code.
            let base: *mut crate::page::Page = (&raw mut (*seg).pages).cast();
            let mut slot: *mut crate::page::Page = base.add(idx);
            while idx < end {
                let len = ((*slot).slice_count as usize).max(1);
                if (*slot).block_size > 0 {
                    page_set_flag(slot, XFLAG_NEVER);
                    (*slot).xheap.store(0, Ordering::Release);
                }
                idx += len;
                slot = slot.wrapping_add(len);
            }
            let next = (*seg).next;
            let _ = segment::segment_free(seg);
            seg = next;
        }
        let mut hseg = h.huge_segments;
        while !hseg.is_null() {
            let pg = &raw mut (*hseg).pages[1];
            page_set_flag(pg, XFLAG_NEVER);
            (*pg).xheap.store(0, Ordering::Release);
            let next = (*hseg).next;
            let _ = segment::huge_free(hseg);
            hseg = next;
        }
        release_heap_box(hb);
    }
}

fn merge_stats(into: &mut crate::heap::Stats, from: &crate::heap::Stats) {
    into.allocs += from.allocs;
    into.frees += from.frees;
    into.generic += from.generic;
    into.pages_fresh += from.pages_fresh;
    into.segments += from.segments;
    into.huge_allocs += from.huge_allocs;
    into.extends += from.extends;
    into.large_allocs += from.large_allocs;
    into.pages_retired += from.pages_retired;
    into.segments_freed += from.segments_freed;
    into.realloc_in_place += from.realloc_in_place;
    into.realloc_moved += from.realloc_moved;
    into.delayed_frees += from.delayed_frees;
    into.reclaims += from.reclaims;
}

/// Unregister and release a HeapBox, repairing the TLS default if needed.
///
/// # Safety
/// `hb` live, owned by the calling thread, all contents already migrated or
/// released.
unsafe fn release_heap_box(hb: *mut HeapBox) {
    heaps_unregister(hb);
    if heap_tls::get() == hb {
        heap_tls::set(backing_heap());
    }
    // A first-class heap created before the first allocation would have been
    // handed the fixed backend's static descriptor; that is not a page and
    // must not be returned to the region.
    if crate::ONE_REGION && prim::fixed::is_first_heap_box(hb) {
        return;
    }
    let size = core::mem::size_of::<HeapBox>();
    let blockdesc = os::OsBlock {
        ptr: hb.cast(),
        size: os::page_align_up(size),
        is_large: false,
        is_zero: false,
    };
    // SAFETY: hb's mapping came from create_heap's os::alloc_aligned with this
    // exact size; unregistered above, no references remain.
    unsafe {
        let _ = os::free(blockdesc);
    }
}

// ---------------------------------------------------------------------------
// Global abandoned-segment list (spin-locked; cold path).
// ---------------------------------------------------------------------------

/// Subprocess isolation (M6, plan §5.9): threads tagged with a subproc id
/// abandon/reclaim only within it — separate interpreters never exchange
/// segments. Id 0 is the main subprocess.
const MAX_SUBPROCS: usize = 64;

static ABANDONED_LOCK: AtomicBool = AtomicBool::new(false);
static ABANDONED_HEADS: [AtomicPtr<Segment>; MAX_SUBPROCS] =
    [const { AtomicPtr::new(ptr::null_mut()) }; MAX_SUBPROCS];
static SUBPROC_NEXT: AtomicUsize = AtomicUsize::new(1);
/// Diagnostic: segments currently abandoned (all subprocs).
pub static ABANDONED_COUNT: AtomicUsize = AtomicUsize::new(0);

ra_thread_local! {
    static SUBPROC: Cell<usize> = const { Cell::new(0) };
}

/// `mi_subproc_main` → 0.
pub fn subproc_main() -> usize {
    0
}

/// `mi_subproc_new`: allocate a fresh subprocess id.
///
/// Exhaustion is an invalid id (`MAX_SUBPROCS`), not an assert (OH-rusty_alloc-21).
pub fn subproc_new() -> usize {
    loop {
        let id = SUBPROC_NEXT.load(Ordering::Acquire);
        if id >= MAX_SUBPROCS {
            return MAX_SUBPROCS;
        }
        if SUBPROC_NEXT
            .compare_exchange_weak(id, id + 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return id;
        }
    }
}

/// `mi_subproc_delete`: ids are not recycled in v1; abandoned segments of the
/// subproc are drained into the main subproc so they stay reclaimable.
pub fn subproc_delete(id: usize) {
    if id == 0 || id >= MAX_SUBPROCS {
        return;
    }
    abandoned_lock();
    // SAFETY: list surgery under the lock.
    unsafe {
        let mut seg = ABANDONED_HEADS[id].swap(ptr::null_mut(), Ordering::AcqRel);
        while !seg.is_null() {
            let next = (*seg).next;
            (*seg).next = ABANDONED_HEADS[0].load(Ordering::Relaxed);
            ABANDONED_HEADS[0].store(seg, Ordering::Relaxed);
            seg = next;
        }
    }
    abandoned_unlock();
}

/// `mi_subproc_add_current_thread`.
pub fn subproc_add_current_thread(id: usize) {
    if id >= MAX_SUBPROCS {
        return;
    }
    SUBPROC.with(|c| c.set(id));
    // Mirror onto this thread's backing heap so TEARDOWN can read the tag
    // without touching the thread-local (see `HeapBox::subproc`). Tagging
    // before the first allocation is the common order and is covered by
    // `create_heap` reading `my_subproc()`; this covers tagging AFTER the heap
    // already exists. Deliberately does NOT bootstrap a heap — a thread that
    // tags itself and never allocates should not be given one.
    let hb = BACKING_PTR.with(|c| c.get());
    if !hb.is_null() {
        // SAFETY: hb is this thread's own live backing box; `subproc` is
        // atomic, so the teardown read needs no other synchronisation.
        unsafe { (*hb).subproc.store(id, Ordering::Release) };
    }
}

fn my_subproc() -> usize {
    SUBPROC.with(|c| c.get())
}

/// `mi_abandoned_visit_blocks`: visit live pages/blocks of a subprocess's
/// abandoned segments, holding the list lock (segments are pinned meanwhile).
pub fn abandoned_visit_blocks(
    subproc_id: usize,
    heap_tag: i32,
    visit_blocks: bool,
    f: &mut dyn FnMut(&crate::heap::AreaInfo, *mut u8, usize) -> bool,
) -> bool {
    if subproc_id >= MAX_SUBPROCS {
        return false;
    }
    abandoned_lock();
    let mut ok = true;
    let mut seg = ABANDONED_HEADS[subproc_id].load(Ordering::Acquire);
    while !seg.is_null() && ok {
        // SAFETY: the lock pins abandoned segments (adopt also takes it).
        unsafe {
            ok = crate::heap::visit_segment_blocks(seg, heap_tag, visit_blocks, false, f);
            seg = (*seg).next;
        }
    }
    abandoned_unlock();
    ok
}

fn abandoned_lock() {
    while ABANDONED_LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn abandoned_unlock() {
    ABANDONED_LOCK.store(false, Ordering::Release);
}

/// Publish `seg` to subprocess `sp`'s abandoned list.
///
/// `sp` is passed in rather than read from `my_subproc()`: every caller is on
/// the teardown path, where the `SUBPROC` thread-local may already be gone (see
/// `HeapBox::subproc`). The owning heap box carries the tag instead.
fn abandoned_push(seg: *mut Segment, sp: usize) {
    // The correctness argument behind `abandoned_pop`'s early return: if this
    // ever runs on a single-context build, a thread ended, and the target's
    // single-thread assertion was false all along. Worth knowing for its own
    // sake -- `SingleThreadCell` and `prim::fixed` rest on the same claim.
    if crate::ONE_THREAD {
        unreachable!("a segment was abandoned on a build that asserted a single thread");
    }
    debug_assert!(sp < MAX_SUBPROCS);
    // PURGE BEFORE ORPHANING (`abandoned_page_purge`, on by default). Until
    // some other thread adopts this segment it has no owner, so every page it
    // touched stays resident for an unbounded time and nothing can reuse it.
    // Done here, before publishing to the list, because this is the last
    // instant the caller still exclusively owns the segment.
    if crate::options::is_enabled(30) {
        // SAFETY: still owned by the dying thread; free spans hold no live
        // blocks, and each purged span is marked so reuse re-commits it.
        unsafe { crate::segment::purge_free_spans(seg) };
    }
    abandoned_lock();
    // SAFETY: seg is disowned; its `next` is ours to use as the list link.
    unsafe {
        (*seg).next = ABANDONED_HEADS[sp].load(Ordering::Relaxed);
    }
    ABANDONED_HEADS[sp].store(seg, Ordering::Relaxed);
    ABANDONED_COUNT.fetch_add(1, Ordering::Relaxed);
    abandoned_unlock();
}

/// Pop one abandoned segment from the CALLER's subprocess and take ownership.
/// Returns null when none are available.
pub fn abandoned_pop() -> *mut Segment {
    // A single-context build has nothing to pop, by construction: a segment is
    // only ever abandoned by a thread that ENDS (`thread_done`), and the
    // target has asserted there is exactly one thread for the life of the
    // program. Folding this to null is what makes `adopt_segment` unreachable
    // from the allocation path, so the linker drops it -- 1,154 bytes on an
    // ESP32-S3 that could never have run (`docs/plans/finished/firmware-code-size.md`,
    // lever 1). `heap_delete` still calls it directly, and a firmware that
    // deletes first-class heaps keeps it; that is a real path, not a
    // cross-thread one.
    if crate::ONE_THREAD {
        return ptr::null_mut();
    }
    let sp = my_subproc();
    if ABANDONED_HEADS[sp].load(Ordering::Acquire).is_null() {
        return ptr::null_mut(); // fast no-lock exit for the common case
    }
    abandoned_lock();
    let seg = ABANDONED_HEADS[sp].load(Ordering::Relaxed);
    if !seg.is_null() {
        // SAFETY: list links are protected by the lock.
        unsafe {
            ABANDONED_HEADS[sp].store((*seg).next, Ordering::Relaxed);
            (*seg).next = ptr::null_mut();
            (*seg).thread_id.store(thread_id(), Ordering::Release);
        }
        ABANDONED_COUNT.fetch_sub(1, Ordering::Relaxed);
    }
    abandoned_unlock();
    seg
}
