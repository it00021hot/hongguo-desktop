//! Registry-backed mock prim for miri (gate G4): the layers above (os, page,
//! segment logic) run under miri against this backend because real FFI cannot.
//!
//! Fidelity notes (what the mock does and does not model):
//! - alloc/free with real alignment via over-aligned `std::alloc` layouts;
//!   double/invalid frees are caught by the registry.
//! - commit/decommit/reset are bookkeeping no-ops (decommit reports
//!   `needs_recommit = false`, contents preserved — weaker than any real OS;
//!   zeroing invariants are therefore NOT provable under the mock and belong
//!   to the native integration tests).
//! - TLS destructors do not fire (thread-exit hooks are loom/native territory).

use core::ffi::c_void;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use super::{Alloc, MemConfig, PrimError, TlsDtor};

const MOCK_PAGE: usize = 4096;
/// Synthetic error code for mock failures.
const MERR: PrimError = 0xDEAD;

fn registry() -> &'static Mutex<HashMap<usize, std::alloc::Layout>> {
    static REG: std::sync::OnceLock<Mutex<HashMap<usize, std::alloc::Layout>>> =
        std::sync::OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn mem_init() -> MemConfig {
    MemConfig {
        page_size: MOCK_PAGE,
        alloc_granularity: MOCK_PAGE,
        large_page_size: 0,
        has_overcommit: false,
        has_partial_free: false,
    }
}

pub(super) unsafe fn alloc(
    size: usize,
    try_alignment: usize,
    _commit: bool,
    _allow_large: bool,
) -> Result<Alloc, PrimError> {
    let align = try_alignment.max(MOCK_PAGE);
    let layout = std::alloc::Layout::from_size_align(size, align).map_err(|_| MERR)?;
    // SAFETY: layout has non-zero size per the prim contract (size > 0).
    let p = unsafe { std::alloc::alloc_zeroed(layout) };
    if p.is_null() {
        return Err(MERR);
    }
    registry().lock().unwrap().insert(p as usize, layout);
    Ok(Alloc {
        ptr: p,
        is_large: false,
        is_zero: true,
    })
}

pub(super) unsafe fn free(ptr_: *mut u8, _size: usize) -> Result<(), PrimError> {
    let layout = registry()
        .lock()
        .unwrap()
        .remove(&(ptr_ as usize))
        .ok_or(MERR)?;
    // SAFETY: ptr/layout pair came from alloc_zeroed via the registry, freed once.
    unsafe { std::alloc::dealloc(ptr_, layout) };
    Ok(())
}

pub(super) unsafe fn commit(_ptr: *mut u8, _size: usize) -> Result<bool, PrimError> {
    Ok(false)
}

pub(super) unsafe fn decommit(_ptr: *mut u8, _size: usize) -> Result<bool, PrimError> {
    Ok(false)
}

pub(super) unsafe fn reset(_ptr: *mut u8, _size: usize) -> Result<(), PrimError> {
    Ok(())
}

pub(super) unsafe fn protect(_ptr: *mut u8, _size: usize, _on: bool) -> Result<(), PrimError> {
    Ok(())
}

pub(super) fn range_is_reserved(ptr: *const u8, size: usize) -> bool {
    let start = ptr as usize;
    let Some(end) = start.checked_add(size) else {
        return false;
    };
    let Ok(reg) = registry().lock() else {
        return false;
    };
    let mut a = start;
    while a < end {
        let Some((&base, layout)) = reg.iter().find(|&(&b, l)| a >= b && a - b < l.size()) else {
            return false;
        };
        a = base.saturating_add(layout.size());
        if a <= start {
            return false;
        }
    }
    true
}

pub(super) fn numa_node_count() -> usize {
    1
}

pub(super) fn thread_id() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    std::thread_local! {
        static ID: usize = NEXT.fetch_add(1, Ordering::Relaxed);
    }
    ID.with(|id| *id)
}

pub(super) fn clock_now() -> u64 {
    // Deterministic monotonic stand-in (miri forbids real clocks by default).
    static TICKS: AtomicU64 = AtomicU64::new(0);
    TICKS.fetch_add(1, Ordering::Relaxed)
}

pub(super) struct TlsSlotImpl(usize);

pub(super) fn tls_new(_dtor: Option<TlsDtor>) -> Option<TlsSlotImpl> {
    static NEXT_SLOT: AtomicUsize = AtomicUsize::new(1);
    Some(TlsSlotImpl(NEXT_SLOT.fetch_add(1, Ordering::Relaxed)))
}

std::thread_local! {
    static TLS: std::cell::RefCell<HashMap<usize, usize>> =
        std::cell::RefCell::new(HashMap::new());
}

pub(super) fn tls_get(slot: &TlsSlotImpl) -> *mut c_void {
    TLS.with(|m| *m.borrow().get(&slot.0).unwrap_or(&0)) as *mut c_void
}

pub(super) fn tls_set(slot: &TlsSlotImpl, value: *mut c_void) {
    TLS.with(|m| m.borrow_mut().insert(slot.0, value as usize));
}

pub(super) fn tls_raw(slot: &TlsSlotImpl) -> usize {
    slot.0
}

pub(super) fn tls_from_raw(raw: usize) -> TlsSlotImpl {
    TlsSlotImpl(raw)
}
