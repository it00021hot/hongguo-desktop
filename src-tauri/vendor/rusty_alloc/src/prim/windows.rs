//! Windows prim backend (mirrors upstream `src/prim/windows/prim.c`).
//!
//! Salient Windows facts encoded here:
//! - `VirtualAlloc(NULL, …)` is 64 KiB-granularity-aligned; stronger alignment
//!   uses the reserve-oversized → release → re-reserve-at-aligned race-retry
//!   dance (`win_alloc_aligned` upstream) because Windows cannot partially free.
//! - Fresh commits are zero pages; decommitted ranges fault until recommitted.
//! - Large pages need `SeLockMemoryPrivilege`; we attempt and fall back.
//! - Thread-exit callbacks come from fiber-local storage (`FlsAlloc`).

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::Environment::GetEnvironmentVariableA;
use windows_sys::Win32::System::Memory::{
    GetLargePageMinimum, MEM_COMMIT, MEM_DECOMMIT, MEM_FREE, MEM_LARGE_PAGES, MEM_RELEASE,
    MEM_RESERVE, MEM_RESET, MEMORY_BASIC_INFORMATION, PAGE_NOACCESS, PAGE_READWRITE, VirtualAlloc,
    VirtualFree, VirtualProtect, VirtualQuery,
};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
use windows_sys::Win32::System::Threading::{
    FlsAlloc, FlsGetValue, FlsSetValue, GetCurrentThreadId, GetNumaHighestNodeNumber,
};

use super::{Alloc, MemConfig, PrimError, TlsDtor, align_up};

pub(super) fn mem_init() -> MemConfig {
    // SAFETY: SYSTEM_INFO is plain-old-data; GetSystemInfo fully initializes it.
    let si = unsafe {
        let mut si: SYSTEM_INFO = core::mem::zeroed();
        GetSystemInfo(&mut si);
        si
    };
    // SAFETY: no preconditions; returns 0 when the privilege is unavailable.
    let large = unsafe { GetLargePageMinimum() };
    MemConfig {
        page_size: si.dwPageSize as usize,
        alloc_granularity: si.dwAllocationGranularity as usize,
        large_page_size: large,
        has_overcommit: false,
        has_partial_free: false,
    }
}

fn last_error() -> PrimError {
    // SAFETY: no preconditions.
    unsafe { GetLastError() }
}

pub(super) unsafe fn alloc(
    size: usize,
    try_alignment: usize,
    commit: bool,
    allow_large: bool,
) -> Result<Alloc, PrimError> {
    let flags = MEM_RESERVE | if commit { MEM_COMMIT } else { 0 };

    // Large-page attempt: requires commit (reserve-only large pages are not a
    // thing) and the privilege; never an error to lack it.
    if allow_large && commit {
        // SAFETY: no preconditions.
        let large_min = unsafe { GetLargePageMinimum() };
        if large_min > 0 && try_alignment <= large_min && size >= large_min {
            // Unrepresentable large-page round-up falls through to normal pages
            // (OH-rusty_alloc-29).
            if let Some(lsize) = super::align_up_checked(size, large_min) {
                // SAFETY: NULL base + valid flags; failure returns null, handled.
                let p = unsafe {
                    VirtualAlloc(ptr::null(), lsize, flags | MEM_LARGE_PAGES, PAGE_READWRITE)
                };
                // `is_multiple_of` on a RUNTIME divisor is a modulo, i.e. a real
                // `div`. Same substitution as D7/D12/D19; this site was missed
                // because it is Windows-only and every scan in that campaign read
                // the Linux `.so`. `is_aligned_to`'s conservative direction is the
                // safe one here: a `false` falls through to the aligned-reservation
                // retry below rather than accepting a block.
                if !p.is_null() && crate::bins::is_aligned_to(p as usize, try_alignment) {
                    return Ok(Alloc {
                        ptr: p.cast(),
                        is_large: true,
                        is_zero: true,
                    });
                }
                if !p.is_null() {
                    // SAFETY: p is a whole mapping we just made and never exposed.
                    unsafe { VirtualFree(p, 0, MEM_RELEASE) };
                }
            }
            // fall through to normal pages
        }
    }

    // Fast path: reservations are 64 KiB aligned already.
    // SAFETY: NULL base + valid flags; failure returns null, handled.
    let p = unsafe { VirtualAlloc(ptr::null(), size, flags, PAGE_READWRITE) };
    if !p.is_null() && crate::bins::is_aligned_to(p as usize, try_alignment) {
        return Ok(Alloc {
            ptr: p.cast(),
            is_large: false,
            is_zero: true,
        });
    }
    if !p.is_null() {
        // SAFETY: whole mapping we just made and never exposed.
        unsafe { VirtualFree(p, 0, MEM_RELEASE) };
    }

    // Aligned dance: reserve oversized to find an aligned address, release,
    // re-reserve exactly there. Another thread can steal the range between the
    // two calls, hence the retry loop (upstream uses 3 tries as well).
    for _ in 0..3 {
        let Some(over) = size.checked_add(try_alignment) else {
            return Err(12); // ENOMEM: unrepresentable aligned-reserve (OH-rusty_alloc-29)
        };
        // SAFETY: reserve-only of an oversized range; failure handled.
        let probe = unsafe { VirtualAlloc(ptr::null(), over, MEM_RESERVE, PAGE_NOACCESS) };
        if probe.is_null() {
            return Err(last_error());
        }
        let aligned = align_up(probe as usize, try_alignment) as *mut c_void;
        // SAFETY: whole probe mapping, never exposed.
        unsafe { VirtualFree(probe, 0, MEM_RELEASE) };
        // SAFETY: explicit base inside a range we just observed free; on race
        // the call fails with null and we retry.
        let p = unsafe { VirtualAlloc(aligned, size, flags, PAGE_READWRITE) };
        if core::ptr::eq(p, aligned) {
            return Ok(Alloc {
                ptr: p.cast(),
                is_large: false,
                is_zero: true,
            });
        }
        if !p.is_null() {
            // SAFETY: mapping we just made at the wrong address, never exposed.
            unsafe { VirtualFree(p, 0, MEM_RELEASE) };
        }
    }
    Err(last_error())
}

pub(super) unsafe fn free(ptr_: *mut u8, _size: usize) -> Result<(), PrimError> {
    // SAFETY: caller passes a whole-mapping base per the prim contract;
    // MEM_RELEASE with size 0 releases the whole reservation.
    let ok = unsafe { VirtualFree(ptr_.cast(), 0, MEM_RELEASE) };
    if ok != 0 { Ok(()) } else { Err(last_error()) }
}

pub(super) unsafe fn commit(ptr_: *mut u8, size: usize) -> Result<bool, PrimError> {
    // SAFETY: caller guarantees the range lies in a live reservation.
    let p = unsafe { VirtualAlloc(ptr_.cast(), size, MEM_COMMIT, PAGE_READWRITE) };
    if p.is_null() {
        return Err(last_error());
    }
    // `VirtualAlloc(NULL, …, MEM_COMMIT)` allocates a *new* region. If the
    // OS did not commit exactly the requested address, release the stray
    // mapping and refuse (OH-rusty_alloc-60).
    if p != ptr_.cast() {
        // SAFETY: `p` is a mapping the call above just created and nothing
        // else has seen; releasing it whole is the only correct disposal.
        let _ = unsafe { VirtualFree(p, 0, MEM_RELEASE) };
        return Err(22);
    }
    // Conservative: the range may include already-committed pages whose
    // contents persist, so we cannot promise zero (fresh pages ARE zero).
    Ok(false)
}

pub(super) unsafe fn decommit(ptr_: *mut u8, size: usize) -> Result<bool, PrimError> {
    // SAFETY: caller guarantees the range lies in a live reservation.
    let ok = unsafe { VirtualFree(ptr_.cast(), size, MEM_DECOMMIT) };
    if ok != 0 { Ok(true) } else { Err(last_error()) }
}

pub(super) unsafe fn reset(ptr_: *mut u8, size: usize) -> Result<(), PrimError> {
    // SAFETY: caller guarantees a live committed range; MEM_RESET marks the
    // contents dead without decommitting.
    let p = unsafe { VirtualAlloc(ptr_.cast(), size, MEM_RESET, PAGE_READWRITE) };
    if p.is_null() {
        Err(last_error())
    } else {
        Ok(())
    }
}

pub(super) unsafe fn protect(ptr_: *mut u8, size: usize, on: bool) -> Result<(), PrimError> {
    let new = if on { PAGE_NOACCESS } else { PAGE_READWRITE };
    let mut old = 0u32;
    // SAFETY: caller guarantees a live committed range; old-protection out-param
    // is a valid local.
    let ok = unsafe { VirtualProtect(ptr_.cast(), size, new, &mut old) };
    if ok != 0 { Ok(()) } else { Err(last_error()) }
}

/// Whether `[ptr, ptr+size)` is reserved or committed (not `MEM_FREE`).
pub(super) fn range_is_reserved(ptr: *const u8, size: usize) -> bool {
    if ptr.is_null() || size == 0 {
        return false;
    }
    let mut addr = ptr as usize;
    let Some(end) = addr.checked_add(size) else {
        return false;
    };
    while addr < end {
        // SAFETY: `MEMORY_BASIC_INFORMATION` is plain data (pointers and
        // integers); all-zero is a valid value of it.
        let mut mbi = unsafe { core::mem::zeroed::<MEMORY_BASIC_INFORMATION>() };
        // SAFETY: read-only query; the out-pointer is a live local of exactly
        // the length passed.
        let n = unsafe {
            VirtualQuery(
                addr as *const c_void,
                &mut mbi,
                core::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if n == 0 || mbi.State == MEM_FREE || mbi.RegionSize == 0 {
            return false;
        }
        let region_end = (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize);
        if region_end <= addr {
            return false;
        }
        addr = region_end;
    }
    true
}

pub(super) fn numa_node_count() -> usize {
    let mut highest = 0u32;
    // SAFETY: out-param is a valid local.
    let ok = unsafe { GetNumaHighestNodeNumber(&mut highest) };
    if ok != 0 { highest as usize + 1 } else { 1 }
}

/// `_mi_prim_getenv`: the value of `name` into `out`, no allocation of ours.
///
/// The ANSI form, as upstream's prim uses: option values are integers and
/// booleans, so there is nothing to lose to the code page. The API converts
/// the name to UTF-16 on the process heap — never through this allocator.
pub(super) fn getenv(name: &[u8], out: &mut [u8]) -> Option<usize> {
    let cap = u32::try_from(out.len()).ok()?;
    // SAFETY: `name` is NUL-terminated (checked by `prim::getenv`); `out` is
    // a live buffer of exactly `cap` bytes, and the call writes at most `cap`
    // bytes into it — when the value does not fit it writes nothing and
    // returns the size it would need.
    let n = unsafe { GetEnvironmentVariableA(name.as_ptr(), out.as_mut_ptr(), cap) };
    // 0 is "unset" or "empty", which the API cannot distinguish without a
    // `GetLastError` round-trip; upstream's prim treats both as unset, and so
    // does this. `n >= cap` means it did not fit: not an option value.
    if n == 0 || n >= cap {
        None
    } else {
        Some(n as usize)
    }
}

#[inline]
pub(super) fn thread_id() -> usize {
    // SAFETY: no preconditions.
    (unsafe { GetCurrentThreadId() }) as usize
}

/// Ticks to nanoseconds. `scale` is `1e9 / freq` when that quotient is exact,
/// and 0 when it is not.
///
/// The `u128` arm is not one instruction, or even a slow one: a 128-bit
/// division on x86-64 is a `__udivti3` **libcall**. When `scale` is exact the
/// whole conversion collapses to a single 64-bit multiply.
#[inline]
fn ticks_to_ns(count: u64, freq: u64, scale: u64) -> u64 {
    if scale != 0 {
        // count * (1e9 / freq). Exact by construction, and a u64 product does
        // not wrap before ~292 years of nanoseconds.
        count.wrapping_mul(scale)
    } else {
        ((u128::from(count) * 1_000_000_000u128) / u128::from(freq.max(1))) as u64
    }
}

pub(super) fn clock_now() -> u64 {
    static FREQ: AtomicU64 = AtomicU64::new(0);
    // `1e9 / freq` when exact, else 0. QueryPerformanceFrequency is documented
    // as fixed for the life of the process, so this is computed once. Every
    // TSC-backed Windows since 8 reports 10 MHz, for which the quotient is
    // exactly 100 — so the common case never divides at all.
    static SCALE: AtomicU64 = AtomicU64::new(0);

    // Acquire pairs with the Release store of FREQ below, so a thread that
    // observes an initialised FREQ also observes the SCALE computed with it.
    let mut freq = FREQ.load(Ordering::Acquire);
    if freq == 0 {
        let mut f = 0i64;
        // SAFETY: out-param is a valid local; QPF cannot fail on XP+.
        unsafe { QueryPerformanceFrequency(&mut f) };
        freq = f.max(1) as u64;
        // ONE division, not two: `is_multiple_of` is itself a modulo, so
        // the obvious spelling divides twice to answer a single question.
        // `q * freq` cannot overflow — `q` is `1e9 / freq`, so the product is
        // at most 1e9 — and a `freq` above 1e9 gives `q == 0`, which fails the
        // test and correctly selects the u128 fallback.
        let q = 1_000_000_000u64 / freq;
        let scale = if q * freq == 1_000_000_000 { q } else { 0 };
        SCALE.store(scale, Ordering::Relaxed);
        FREQ.store(freq, Ordering::Release);
    }
    let mut count = 0i64;
    // SAFETY: out-param is a valid local.
    unsafe { QueryPerformanceCounter(&mut count) };
    ticks_to_ns(count.max(0) as u64, freq, SCALE.load(Ordering::Relaxed))
}

pub(super) struct TlsSlotImpl(u32);

pub(super) fn tls_new(dtor: Option<TlsDtor>) -> Option<TlsSlotImpl> {
    // SAFETY: FlsAlloc accepts an optional callback pointer of exactly this
    // type; the callback fires at thread exit with the slot value when non-null.
    let idx = unsafe { FlsAlloc(dtor) };
    if idx == u32::MAX {
        None
    } else {
        Some(TlsSlotImpl(idx))
    }
}

#[inline]
pub(super) fn tls_get(slot: &TlsSlotImpl) -> *mut c_void {
    // SAFETY: index came from a successful FlsAlloc and is never freed (slots
    // live for the process — mirrors upstream).
    unsafe { FlsGetValue(slot.0) }
}

#[inline]
pub(super) fn tls_set(slot: &TlsSlotImpl, value: *mut c_void) {
    // SAFETY: as tls_get.
    unsafe { FlsSetValue(slot.0, value) };
}

pub(super) fn tls_raw(slot: &TlsSlotImpl) -> usize {
    slot.0 as usize
}

pub(super) fn tls_from_raw(raw: usize) -> TlsSlotImpl {
    TlsSlotImpl(raw as u32)
}

#[cfg(test)]
mod clock_tests {
    use super::ticks_to_ns;

    /// The reference the fast path has to reproduce, bit for bit.
    fn reference(count: u64, freq: u64) -> u64 {
        ((u128::from(count) * 1_000_000_000u128) / u128::from(freq)) as u64
    }

    fn scale_for(freq: u64) -> u64 {
        if 1_000_000_000u64.is_multiple_of(freq) {
            1_000_000_000u64 / freq
        } else {
            0
        }
    }

    #[test]
    fn exact_scale_matches_the_u128_reference() {
        // 10 MHz is what every TSC-backed Windows since 8 reports; the others
        // are exact divisors of 1e9 that a hypervisor could plausibly pick.
        for freq in [10_000_000u64, 1_000_000, 100_000, 1_000, 1] {
            assert_eq!(scale_for(freq), 1_000_000_000 / freq, "freq {freq}");
            for count in [0u64, 1, 7, 12_345, 1 << 20, 1 << 32, 86_400 * freq] {
                assert_eq!(
                    ticks_to_ns(count, freq, scale_for(freq)),
                    reference(count, freq),
                    "freq {freq} count {count}"
                );
            }
        }
    }

    #[test]
    fn inexact_frequency_falls_back_and_stays_correct() {
        // The classic pre-Win8 PIT-derived frequency: 1e9 / 3_579_545 is not
        // an integer, so the fast path must NOT be taken.
        let freq = 3_579_545u64;
        assert_eq!(scale_for(freq), 0, "3.579545 MHz must not get a scale");
        for count in [0u64, 1, 3_579_545, 1 << 32] {
            assert_eq!(ticks_to_ns(count, freq, 0), reference(count, freq));
        }
    }

    #[test]
    fn a_day_of_ticks_does_not_wrap() {
        let freq = 10_000_000u64;
        let day = 86_400 * freq;
        assert_eq!(
            ticks_to_ns(day, freq, scale_for(freq)),
            86_400 * 1_000_000_000,
            "one day must convert exactly"
        );
    }
}
