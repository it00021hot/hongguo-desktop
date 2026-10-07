//! WebAssembly prim backend (`memory.grow`), the analogue of upstream's
//! `src/prim/wasi/prim.c`.
//!
//! Wasm has no `mmap`/`VirtualAlloc`: there is ONE linear memory that can only
//! GROW. Everything below follows from that, and each consequence is a real
//! semantic difference the layers above must be able to live with:
//!
//! - **`free` is a no-op.** Linear memory cannot shrink, so a released
//!   reservation can never reach the host. This is the same trade upstream
//!   documents for wasi ("wasi heap cannot be shrunk"). What makes it safe is
//!   `os::free` handing every such block to `arena::adopt_os_block` BEFORE the
//!   no-op is reached, which turns the arena into the free list this platform
//!   otherwise lacks. (An earlier version of this comment attributed that role
//!   to a "segment cache" — no such cache exists in this remake, upstream v2
//!   deleted it when arenas replaced it, and with arenas also disabled on wasm
//!   the no-op stranded one whole 32 MiB segment per large alloc/free cycle.
//!   The doc asserted a recycling layer the code did not have.)
//! - **Alignment costs a one-time pad.** `memory.grow` returns 64 KiB-aligned
//!   memory, but a segment needs `SEGMENT_SIZE` (32 MiB) alignment. We read the
//!   current end of memory, grow by `pad + size`, and hand back the aligned
//!   base — so only the padding is lost, and only ONCE: a 32 MiB-aligned
//!   32 MiB request leaves the end of memory 32 MiB-aligned, so every
//!   subsequent segment needs no pad at all.
//! - **There is no page protection.** [`protect`] returns an error rather than
//!   silently succeeding, because a guard page that does not trap is worse than
//!   no guard page: it would let `secure` builds report a hardening they do not
//!   have.
//! - **There is no clock.** `wasm32-unknown-unknown` has no host time without
//!   JS bindings, so [`clock_now`] is a monotonic counter. Purge *delays*
//!   therefore degenerate to purge-ordering; nothing depends on real duration.
//! - **Single-threaded.** Without the atomics+threads proposal there is exactly
//!   one thread, so the thread id is a constant and TLS is a plain static
//!   table. Thread-exit destructors never fire — there is no thread exit.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use super::{Alloc, MemConfig, PrimError, TlsDtor, align_up};

/// A wasm page is 64 KiB, fixed by the spec.
const WASM_PAGE: usize = 65536;
/// Synthetic error code — wasm surfaces no errno, so any non-zero sentinel
/// does; this one just needs to be distinct from the mock backend's.
const WERR: PrimError = 0xBEEF;

pub(super) fn mem_init() -> MemConfig {
    MemConfig {
        page_size: WASM_PAGE,
        alloc_granularity: WASM_PAGE,
        large_page_size: 0,
        has_overcommit: false,
        // Linear memory cannot be partially released - or released at all.
        has_partial_free: false,
    }
}

/// Current end of linear memory, in bytes.
#[inline]
fn memory_end() -> usize {
    core::arch::wasm32::memory_size::<0>() * WASM_PAGE
}

pub(super) unsafe fn alloc(
    size: usize,
    try_alignment: usize,
    _commit: bool,
    _allow_large: bool,
) -> Result<Alloc, PrimError> {
    let align = try_alignment.max(WASM_PAGE);
    // Single-threaded: no other thread can grow memory between the read and
    // the grow, so the pad computed here is exactly the pad we get. (Under the
    // threads proposal this pair would need a lock, as upstream's wasi backend
    // takes one around sbrk.)
    let cur = memory_end();
    let pad = align_up(cur, align) - cur;
    let Some(need) = pad.checked_add(size) else {
        return Err(WERR);
    };
    let pages = need.div_ceil(WASM_PAGE);

    let prev = core::arch::wasm32::memory_grow::<0>(pages);
    if prev == usize::MAX {
        return Err(WERR);
    }
    let base = prev * WASM_PAGE;
    debug_assert_eq!(base, cur, "memory grew under us in a single-threaded build");

    Ok(Alloc {
        // Freshly grown linear memory is specified to be zero, and because
        // `free` never returns anything to the host this range has never been
        // handed out before - so the claim holds for every allocation, not
        // just the first.
        ptr: core::ptr::with_exposed_provenance_mut(base + pad),
        is_large: false,
        is_zero: true,
    })
}

/// No-op: wasm linear memory cannot shrink. The range stays mapped — and is
/// NOT recycled here: recycling happens one layer up, where `os::free` adopts
/// the block into the arena (`arena::adopt_os_block`) before this no-op is
/// ever reached (`prim::FREE_RETURNS_MEMORY == false` is the switch). A block
/// that does arrive here is genuinely lost, which is why `os::alloc_aligned`
/// makes every segment-aligned reservation chunk-granular on this platform.
#[allow(
    clippy::unnecessary_wraps,
    clippy::unused_self,
    reason = "the prim backends share one signature; a no-op backend still returns the contract's Result"
)]
pub(super) unsafe fn free(_ptr: *mut u8, _size: usize) -> Result<(), PrimError> {
    Ok(())
}

/// Already backed. Reports NOT-known-zero conservatively: `decommit` preserves
/// contents here, so a re-committed range may hold stale bytes and the caller
/// must zero it itself when it needs zeros.
#[allow(
    clippy::unnecessary_wraps,
    clippy::unused_self,
    reason = "the prim backends share one signature; a no-op backend still returns the contract's Result"
)]
pub(super) unsafe fn commit(_ptr: *mut u8, _size: usize) -> Result<bool, PrimError> {
    Ok(false)
}

/// No-op. Returns `false` = no re-commit needed before touching the range
/// again, and contents are preserved (as on unix `MADV_DONTNEED`, not Windows).
#[allow(
    clippy::unnecessary_wraps,
    clippy::unused_self,
    reason = "the prim backends share one signature; a no-op backend still returns the contract's Result"
)]
pub(super) unsafe fn decommit(_ptr: *mut u8, _size: usize) -> Result<bool, PrimError> {
    Ok(false)
}

#[allow(
    clippy::unnecessary_wraps,
    clippy::unused_self,
    reason = "the prim backends share one signature; a no-op backend still returns the contract's Result"
)]
pub(super) unsafe fn reset(_ptr: *mut u8, _size: usize) -> Result<(), PrimError> {
    Ok(())
}

/// Wasm has no page protection. Fail loudly rather than pretend: a guard page
/// that cannot trap would make a `secure` build claim a protection it does not
/// provide.
pub(super) unsafe fn protect(_ptr: *mut u8, _size: usize, _on: bool) -> Result<(), PrimError> {
    Err(WERR)
}

pub(super) fn range_is_reserved(ptr: *const u8, size: usize) -> bool {
    let start = ptr as usize;
    let Some(end) = start.checked_add(size) else {
        return false;
    };
    let lim = memory_end();
    start < lim && end <= lim
}

pub(super) fn numa_node_count() -> usize {
    1
}

/// One thread, so one id. Must be non-zero: zero is the allocator's "segment
/// is abandoned" sentinel.
#[inline]
pub(super) fn thread_id() -> usize {
    1
}

/// No host clock without JS bindings. A monotonic counter preserves ordering,
/// which is all the purge policy actually reads.
pub(super) fn clock_now() -> u64 {
    static TICK: AtomicU64 = AtomicU64::new(1);
    TICK.fetch_add(1, Ordering::Relaxed)
}

/// TLS slots for a single-threaded world: a fixed static table. Destructors are
/// accepted and never run, because there is no thread exit to run them at.
const MAX_TLS: usize = 64;
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
