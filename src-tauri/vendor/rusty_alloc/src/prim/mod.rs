//! OS primitive layer (mirrors upstream `src/prim/*`, plan §6). One backend per
//! platform, selected at compile time; under miri a registry-backed [`mock`]
//! stands in so the layers above stay testable where FFI cannot run (gate G4).
//!
//! This module is one of the few allowed `unsafe` (plan §6 policy): every block
//! carries its SAFETY invariant, and everything above talks to the safe-ish
//! surface in [`crate::os`].

#[cfg(all(windows, not(miri)))]
mod windows;
#[cfg(all(windows, not(miri)))]
use windows as sys;

#[cfg(all(unix, not(miri)))]
mod unix;
#[cfg(all(unix, not(miri)))]
use unix as sys;

// Wasm is neither `windows` nor `unix`, so it needs its own arm: one linear
// memory that only grows, no page protection, no clock, one thread.
#[cfg(all(target_arch = "wasm32", not(miri)))]
mod wasm;
#[cfg(all(target_arch = "wasm32", not(miri)))]
use wasm as sys;

#[cfg(miri)]
pub mod mock;
#[cfg(miri)]
use mock as sys;

// A target with no OS at all — a microcontroller, where memory is a region the
// linker reserved (P1 of `docs/plans/small-metal.md`). This is the fifth arm
// P0 found missing: `riscv32imac-unknown-none-elf` is none of the four above,
// so before this existed no `sys` was named at all and every call through the
// seam failed together.
//
// ALWAYS COMPILED, so it is type-checked and unit-tested on the host;
// SELECTED only where no arm above matches, so no host build changes.
#[cfg_attr(
    any(windows, unix, all(target_arch = "wasm32", not(miri)), miri),
    allow(dead_code)
)]
pub mod fixed;
#[cfg(all(not(miri), not(windows), not(unix), not(target_arch = "wasm32")))]
use fixed as sys;

use core::ffi::c_void;

/// OS error code (`GetLastError` on Windows, `errno` on unix, synthetic in mock).
pub type PrimError = u32;

/// Static memory-subsystem facts, queried once at startup (`_mi_prim_mem_init`).
#[derive(Debug, Clone, Copy)]
pub struct MemConfig {
    /// OS page size (4 KiB on x86-64, 16 KiB on Apple Silicon).
    pub page_size: usize,
    /// Granularity of address reservations (64 KiB on Windows, = page elsewhere).
    pub alloc_granularity: usize,
    /// Large/huge page size, 0 if unavailable to this process.
    pub large_page_size: usize,
    /// OS overcommits (Linux) — commits may lazily succeed and fault later.
    pub has_overcommit: bool,
    /// Part of a mapping can be freed (`munmap` yes; `VirtualFree` no).
    pub has_partial_free: bool,
}

/// Whether [`free`] actually returns memory to the host.
///
/// True everywhere except wasm: `munmap` and `VirtualFree(MEM_RELEASE)` both
/// release the whole mapping (only PARTIAL free differs between them — that is
/// [`MemConfig::has_partial_free`], a different property). On wasm linear
/// memory can only grow, so [`free`] is a no-op and a mapping handed to it
/// would otherwise be unreachable forever. [`crate::os::free`] keys on this to
/// hand such blocks to [`crate::arena::adopt_os_block`] instead, which is the
/// allocator's ONLY recycling layer on such a platform — the segment cache
/// this role is sometimes attributed to does not exist (upstream mimalloc v2
/// deleted it when arenas replaced it, and this remake followed).
///
/// A `const`, not a `MemConfig` field, so the adoption path folds away
/// entirely on every platform whose free works.
pub const FREE_RETURNS_MEMORY: bool = !cfg!(all(target_arch = "wasm32", not(miri)));

/// Result of a successful [`alloc`].
#[derive(Debug, Clone, Copy)]
pub struct Alloc {
    /// Base of the mapping — also what [`free`] must receive.
    pub ptr: *mut u8,
    /// Backed by large/huge pages (cannot be partially decommitted).
    pub is_large: bool,
    /// Memory is guaranteed zero (fresh OS pages).
    pub is_zero: bool,
}

/// Query static memory configuration.
pub fn mem_init() -> MemConfig {
    sys::mem_init()
}

/// Reserve (and optionally commit) `size` bytes aligned to `try_alignment`.
///
/// `size` must be page-multiple and > 0; `try_alignment` a power of two.
/// `allow_large` permits (but never requires) large-page backing.
///
/// # Safety
/// Returned memory is unmanaged: caller owns the range and must eventually
/// [`free`] it with the same base and size.
pub unsafe fn alloc(
    size: usize,
    try_alignment: usize,
    commit: bool,
    allow_large: bool,
) -> Result<Alloc, PrimError> {
    // A non-power-of-two (or zero) alignment is Err, not `align_up`'s
    // `debug_assert!` (OH-rusty_alloc-38). `os::alloc_aligned` already
    // refuses; this public lower seam did not.
    if !try_alignment.is_power_of_two() {
        return Err(22);
    }
    // SAFETY: forwarded contract; alignment is a power of two.
    unsafe { sys::alloc(size, try_alignment, commit, allow_large) }
}

/// Release a mapping obtained from [`alloc`].
///
/// # Safety
/// `ptr`/`size` must denote exactly one prior [`alloc`] result (whole mapping —
/// partial free only where `MemConfig::has_partial_free`), not yet freed.
pub unsafe fn free(ptr: *mut u8, size: usize) -> Result<(), PrimError> {
    // SAFETY: forwarded contract.
    unsafe { sys::free(ptr, size) }
}

/// Commit a page-aligned range inside a reservation. Returns whether the
/// committed range is known-zero.
///
/// # Safety
/// Range must lie inside a live [`alloc`] mapping, page-aligned.
pub unsafe fn commit(ptr: *mut u8, size: usize) -> Result<bool, PrimError> {
    // A null range is Err, not a fresh OS allocation (OH-rusty_alloc-60).
    // Windows `VirtualAlloc(NULL, size, MEM_COMMIT)` treats a null address as
    // "pick anywhere" and returns Ok with an untracked mapping.
    if ptr.is_null() {
        return Err(22);
    }
    // SAFETY: forwarded contract; base is non-null.
    unsafe { sys::commit(ptr, size) }
}

/// Decommit a page-aligned range. Returns whether a later [`commit`] is
/// required before touching it again (Windows: yes; unix `MADV_DONTNEED`: no).
///
/// # Safety
/// Range must lie inside a live [`alloc`] mapping, page-aligned; contents lost.
pub unsafe fn decommit(ptr: *mut u8, size: usize) -> Result<bool, PrimError> {
    if ptr.is_null() {
        return Err(22);
    }
    // SAFETY: forwarded contract; base is non-null.
    unsafe { sys::decommit(ptr, size) }
}

/// Tell the OS the range's contents are dead but keep it committed
/// (`MEM_RESET` / `MADV_FREE`). Contents afterwards are undefined.
///
/// # Safety
/// Range must lie inside a live, committed mapping, page-aligned.
pub unsafe fn reset(ptr: *mut u8, size: usize) -> Result<(), PrimError> {
    if ptr.is_null() {
        return Err(22);
    }
    // SAFETY: forwarded contract; base is non-null.
    unsafe { sys::reset(ptr, size) }
}

/// Toggle no-access protection on a page-aligned range (guard pages, M8).
///
/// # Safety
/// Range must lie inside a live, committed mapping; caller must not touch a
/// protected range until unprotected.
pub unsafe fn protect(ptr: *mut u8, size: usize, protect: bool) -> Result<(), PrimError> {
    if ptr.is_null() {
        return Err(22);
    }
    // SAFETY: forwarded contract; base is non-null.
    unsafe { sys::protect(ptr, size, protect) }
}

/// Whether `[ptr, ptr+size)` is reserved or committed by the OS.
///
/// Used by `manage_os_memory_ex` so an unmapped lie cannot become an arena
/// (OH-rusty_alloc-201). Best-effort: a racing unmap can flip the answer.
pub fn range_is_reserved(ptr: *const u8, size: usize) -> bool {
    if ptr.is_null() || size == 0 {
        return false;
    }
    sys::range_is_reserved(ptr, size)
}

/// Number of NUMA nodes (≥ 1). M1 returns the real count on Windows, 1 on
/// unix (sysfs parsing lands with arenas in M6).
pub fn numa_node_count() -> usize {
    sys::numa_node_count().max(1)
}

/// Copy the value of the environment variable `name` — NUL-terminated ASCII —
/// into `out`, **without allocating** (`_mi_prim_getenv`). `Some(len)` with the
/// value's byte length when the variable is set and fits; `None` when it is
/// unset, empty on Windows (the API reports both as 0 and upstream's prim
/// treats both as unset), or longer than `out`.
///
/// This exists because `options.rs` read the environment through
/// `std::env::var`, whose key and value are both owned `String`s: 38 options
/// x 2 prefixes made **251 allocations through the global allocator on the
/// first allocation of every process** — inside the heap that was being set
/// up (`docs/plans/youslowbro.md` §4). Hosted platforms only: a firmware has
/// no environment, wasm has no raw `getenv`, and Miri interprets `std::env`
/// itself, so `options.rs` keeps a `std::env` fallback for those.
#[cfg(all(any(windows, unix), not(miri)))]
pub fn getenv(name: &[u8], out: &mut [u8]) -> Option<usize> {
    debug_assert_eq!(name.last(), Some(&0), "getenv: name must be NUL-terminated");
    if name.last() != Some(&0) || out.is_empty() {
        return None;
    }
    sys::getenv(name, out)
}

/// Call `f` with a pointer to every `NAME=VALUE` entry of the process
/// environment, in order, each NUL-terminated — one walk of `environ`, without
/// allocating. Linux only (glibc and musl export `environ`; a macOS dylib has
/// to go through `_NSGetEnviron`, and Windows has no such block of C strings).
///
/// For `options.rs`, which needs 38 options under two prefixes: as 76
/// [`getenv`] calls that is 76 walks of the whole environment, 17,372
/// instructions inside libc and 27,011 inclusive on the first allocation of
/// every process — 2 % of a one-line `sort` (callgrind). One walk that looks
/// only at entries starting with either prefix does the same job.
#[cfg(all(target_os = "linux", not(miri)))]
pub fn env_for_each(f: impl FnMut(*const u8)) {
    sys::env_for_each(f);
}

/// Cheap unique id of the calling thread (the heap-ownership key from M4).
#[inline]
pub fn thread_id() -> usize {
    sys::thread_id()
}

/// Monotonic clock in nanoseconds (purge delays, stats).
pub fn clock_now() -> u64 {
    sys::clock_now()
}

/// Platform-native TLS destructor signature. On x86-64/aarch64 the `"system"`
/// and `"C"` ABIs coincide; the alias exists so each backend hands its OS the
/// exact type it expects.
#[cfg(all(windows, not(miri)))]
pub type TlsDtor = unsafe extern "system" fn(*const c_void);
/// Platform-native TLS destructor signature (pthread form).
#[cfg(any(not(windows), miri))]
pub type TlsDtor = unsafe extern "C" fn(*mut c_void);

/// A dynamically-created TLS slot whose destructor runs at thread exit with
/// the slot's value, if non-null (`FlsAlloc` / `pthread_key_create`). This is
/// the hook `mi_thread_done` hangs off in M4.
pub struct TlsSlot(sys::TlsSlotImpl);

impl TlsSlot {
    /// Create a slot. `None` if the OS is out of TLS indices.
    pub fn new(dtor: Option<TlsDtor>) -> Option<TlsSlot> {
        sys::tls_new(dtor).map(TlsSlot)
    }

    /// Read the calling thread's value (null if never set).
    #[inline]
    pub fn get(&self) -> *mut c_void {
        sys::tls_get(&self.0)
    }

    /// Set the calling thread's value.
    #[inline]
    pub fn set(&self, value: *mut c_void) {
        sys::tls_set(&self.0, value)
    }

    /// The slot's raw OS handle (for storing in an atomic; slots live for the
    /// process, so the handle never dangles).
    pub fn into_raw(self) -> usize {
        sys::tls_raw(&self.0)
    }

    /// Rebuild a slot from [`into_raw`](Self::into_raw)'s value.
    ///
    /// # Safety
    /// `raw` must come from `into_raw` of a slot created in this process.
    pub unsafe fn from_raw(raw: usize) -> TlsSlot {
        TlsSlot(sys::tls_from_raw(raw))
    }
}

// SAFETY: a TLS slot handle is an index into per-thread storage; sharing the
// handle across threads is the entire point (each thread sees its own value).
unsafe impl Send for TlsSlot {}
// SAFETY: as above — get/set only touch calling-thread state.
unsafe impl Sync for TlsSlot {}

/// Round `n` up to a multiple of power-of-two `align`.
#[inline]
pub(crate) const fn align_up(n: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (n + align - 1) & !(align - 1)
}

/// Checked sibling of [`align_up`]. `None` when `n + align - 1` overflows
/// (OH-rusty_alloc-29). Only the Windows backend's large-page path rounds a
/// caller size up by an OS-reported unit; the others add before they align.
#[cfg(all(windows, not(miri)))]
#[inline]
pub(crate) fn align_up_checked(n: usize, align: usize) -> Option<usize> {
    debug_assert!(align.is_power_of_two());
    n.checked_add(align - 1).map(|s| s & !(align - 1))
}
