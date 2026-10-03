//! Unix prim backend (mirrors upstream `src/prim/unix/prim.c`).
//!
//! Salient unix facts encoded here:
//! - `munmap` can free part of a mapping, so alignment is over-allocate + trim
//!   (no race-retry dance needed).
//! - `MADV_DONTNEED` (Linux) leaves the range accessible and zero-on-next-touch
//!   → decommit needs no recommit.
//! - Reserve-only memory is `PROT_NONE` (+`MAP_NORESERVE` on Linux) so commit
//!   charges appear when the allocator says so, not at reservation.

use core::ffi::c_void;
use core::ptr;

use super::{Alloc, MemConfig, PrimError, TlsDtor, align_up};

fn errno() -> PrimError {
    // SAFETY: __errno_location/__error return a valid thread-local pointer.
    #[cfg(target_os = "linux")]
    unsafe {
        *libc::__errno_location() as PrimError
    }
    #[cfg(target_os = "macos")]
    // SAFETY: as above; `__error` is Apple's name for the same accessor.
    unsafe {
        *libc::__error() as PrimError
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        1 // EPERM stand-in; refine per-OS as targets are added
    }
}

pub(super) fn mem_init() -> MemConfig {
    // SAFETY: sysconf with a valid name has no preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let page_size = if page > 0 { page as usize } else { 4096 };
    MemConfig {
        page_size,
        alloc_granularity: page_size,
        large_page_size: 2 * 1024 * 1024, // advisory; real THP/hugetlb wiring in M6
        has_overcommit: cfg!(target_os = "linux"),
        has_partial_free: true,
    }
}

unsafe fn mmap_anon(hint: *mut c_void, size: usize, commit: bool) -> Result<*mut u8, PrimError> {
    let prot = if commit {
        libc::PROT_READ | libc::PROT_WRITE
    } else {
        libc::PROT_NONE
    };
    #[allow(unused_mut)]
    let mut flags = libc::MAP_PRIVATE | libc::MAP_ANON;
    #[cfg(target_os = "linux")]
    if !commit {
        flags |= libc::MAP_NORESERVE;
    }
    // SAFETY: anonymous private mapping; hint may be null; failure yields
    // MAP_FAILED which we translate to errno.
    let p = unsafe { libc::mmap(hint, size, prot, flags, -1, 0) };
    if p == libc::MAP_FAILED {
        Err(errno())
    } else {
        Ok(p.cast())
    }
}

pub(super) unsafe fn alloc(
    size: usize,
    try_alignment: usize,
    commit: bool,
    _allow_large: bool, // explicit huge-page mappings land in M6; THP applies transparently
) -> Result<Alloc, PrimError> {
    let cfg = mem_init();
    if try_alignment <= cfg.page_size {
        // SAFETY: forwarded contract.
        let p = unsafe { mmap_anon(ptr::null_mut(), size, commit)? };
        return Ok(Alloc {
            ptr: p,
            is_large: false,
            is_zero: true,
        });
    }

    // Over-allocate and trim the unaligned head/tail (partial free is allowed).
    // Unrepresentable `size + alignment` is Err, not a debug add overflow
    // (OH-rusty_alloc-29).
    let Some(over) = size.checked_add(try_alignment) else {
        return Err(12);
    };
    // SAFETY: forwarded contract.
    let raw = unsafe { mmap_anon(ptr::null_mut(), over, commit)? };
    let base = raw as usize;
    let aligned = align_up(base, try_alignment);
    let pre = aligned - base;
    let post = over - size - pre;
    if pre > 0 {
        // SAFETY: [raw, raw+pre) is the head of the mapping we just made.
        unsafe { libc::munmap(raw.cast(), pre) };
    }
    if post > 0 {
        // SAFETY: the tail range lies wholly inside the same fresh mapping.
        unsafe { libc::munmap((aligned + size) as *mut c_void, post) };
    }
    Ok(Alloc {
        ptr: aligned as *mut u8,
        is_large: false,
        is_zero: true,
    })
}

pub(super) unsafe fn free(ptr_: *mut u8, size: usize) -> Result<(), PrimError> {
    // SAFETY: caller passes a range from alloc per the prim contract.
    let r = unsafe { libc::munmap(ptr_.cast(), size) };
    if r == 0 { Ok(()) } else { Err(errno()) }
}

pub(super) unsafe fn commit(ptr_: *mut u8, size: usize) -> Result<bool, PrimError> {
    // SAFETY: caller guarantees the range lies in a live mapping.
    let r = unsafe { libc::mprotect(ptr_.cast(), size, libc::PROT_READ | libc::PROT_WRITE) };
    if r != 0 {
        return Err(errno());
    }
    // Pages previously touched then DONTNEED'd read zero; pages never touched
    // read zero; but a commit over still-resident pages keeps contents →
    // conservative false, same as upstream.
    Ok(false)
}

pub(super) unsafe fn decommit(ptr_: *mut u8, size: usize) -> Result<bool, PrimError> {
    // DARWIN IS NOT LINUX HERE. `MADV_DONTNEED` is only advisory for PRIVATE
    // ANONYMOUS memory on macOS/BSD: it neither frees the physical pages nor
    // zeroes them. Using it there breaks decommit in both directions —
    //
    //   * the pages stay RESIDENT, so purge never actually returns memory to
    //     the OS and RSS only ever grows in a long-running process (this is the
    //     abandonment path, which is on by default, so it was live);
    //   * and the range keeps its old CONTENTS, violating the documented
    //     "contents are lost" contract. Nothing trusts that yet — `free_is_zero`
    //     is conservatively cleared on purge — but it is a landmine for anything
    //     that later does.
    //
    // Re-mapping is the portable-on-Darwin decommit: MAP_FIXED over a range we
    // already own atomically drops the old physical pages and installs fresh
    // zero-fill-on-demand ones. The range stays readable/writable and needs no
    // recommit, which is exactly the Linux MADV_DONTNEED contract, so callers
    // see identical semantics on both.
    #[cfg(target_vendor = "apple")]
    {
        // SAFETY: caller guarantees `[ptr_, ptr_+size)` is a page-aligned range
        // inside a live mapping WE own, so replacing it cannot clobber another
        // subsystem's memory. MAP_FIXED is what makes the replacement atomic:
        // the range is never unmapped, so a concurrent reader cannot fault.
        let p = unsafe {
            libc::mmap(
                ptr_.cast(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON | libc::MAP_FIXED,
                -1,
                0,
            )
        };
        if p == libc::MAP_FAILED {
            return Err(errno());
        }
        // The tail on Apple, where the block below is compiled out.
        Ok(false)
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        // SAFETY: caller guarantees the range lies in a live mapping; DONTNEED
        // drops the pages, next touch faults in zeros — stays accessible.
        let r = unsafe { libc::madvise(ptr_.cast(), size, libc::MADV_DONTNEED) };
        if r == 0 { Ok(false) } else { Err(errno()) }
    }
}

pub(super) unsafe fn reset(ptr_: *mut u8, size: usize) -> Result<(), PrimError> {
    // MADV_FREE is lazy (preferred); fall back to DONTNEED where missing.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        // SAFETY: caller guarantees a live committed range.
        let r = unsafe { libc::madvise(ptr_.cast(), size, libc::MADV_FREE) };
        if r == 0 {
            return Ok(());
        }
    }
    // SAFETY: as above.
    let r = unsafe { libc::madvise(ptr_.cast(), size, libc::MADV_DONTNEED) };
    if r == 0 { Ok(()) } else { Err(errno()) }
}

pub(super) unsafe fn protect(ptr_: *mut u8, size: usize, on: bool) -> Result<(), PrimError> {
    let prot = if on {
        libc::PROT_NONE
    } else {
        libc::PROT_READ | libc::PROT_WRITE
    };
    // SAFETY: caller guarantees a live mapping.
    let r = unsafe { libc::mprotect(ptr_.cast(), size, prot) };
    if r == 0 { Ok(()) } else { Err(errno()) }
}

/// Whether `[ptr, ptr+size)` is mapped (`mincore` succeeds on every page).
#[cfg(not(target_vendor = "apple"))]
pub(super) fn range_is_reserved(ptr: *const u8, size: usize) -> bool {
    if ptr.is_null() || size == 0 {
        return false;
    }
    let page = mem_init().page_size.max(1);
    let start = ptr as usize;
    let aligned = start & !(page - 1);
    let Some(span) = size.checked_add(start - aligned) else {
        return false;
    };
    let npages = span.div_ceil(page);
    for i in 0..npages {
        let p = aligned + i * page;
        let mut vec = 0u8;
        // SAFETY: `p` is page-aligned and `page` is one page, so the out
        // vector needs one byte, which `vec` is; `mincore` reads the mapping
        // table and writes only that byte.
        //
        // `.cast()`, not `&mut vec`: the out vector is `*mut c_uchar` on
        // Linux but `*mut c_char` on the BSDs, so a `&mut u8` coerces on one
        // and fails to compile on the other.
        let r = unsafe { libc::mincore(p as *mut libc::c_void, page, (&raw mut vec).cast()) };
        if r != 0 {
            return false;
        }
    }
    true
}

/// `vm_region_basic_info_64` from `<mach/vm_region.h>`, which `libc` does not
/// carry. The header declares it under `#pragma pack(4)`, so `offset` sits at
/// byte 20 rather than 24 and the whole is 36 bytes — the size the kernel is
/// told it may write, through `VM_REGION_BASIC_INFO_COUNT_64` below.
#[cfg(target_vendor = "apple")]
#[repr(C, packed(4))]
#[derive(Default)]
struct VmRegionBasicInfo64 {
    protection: libc::vm_prot_t,
    max_protection: libc::vm_prot_t,
    inheritance: u32,
    shared: i32,
    reserved: i32,
    offset: u64,
    behavior: i32,
    user_wired_count: u16,
}

#[cfg(target_vendor = "apple")]
const _: () = assert!(core::mem::size_of::<VmRegionBasicInfo64>() == 36);

/// Whether `[ptr, ptr+size)` is mapped memory that could ever be accessed.
///
/// Not `mincore`, which is the Linux answer and a wrong one here: XNU's
/// `mincore` succeeds on unmapped ranges, so an unmapped arena was accepted
/// (`oh_f05_manage_os_memory_unmapped_is_err`, the first macOS CI run). The
/// Mach region query is the Apple equivalent of the `VirtualQuery` walk on
/// Windows — and it adds one rule neither other platform needs: every 64-bit
/// Apple process maps `__PAGEZERO` over the low 4 GiB with a MAXIMUM
/// protection of none, so "mapped" alone accepts `0x8` as an arena base
/// (`oh_f05_manage_os_memory_garbage_base_is_err`). A region that can never be
/// made accessible is not memory; a caller's `PROT_NONE` reservation still
/// is, because its maximum protection is not none.
#[cfg(target_vendor = "apple")]
pub(super) fn range_is_reserved(ptr: *const u8, size: usize) -> bool {
    const VM_REGION_BASIC_INFO_64: i32 = 9;
    const VM_REGION_BASIC_INFO_COUNT_64: u32 =
        (core::mem::size_of::<VmRegionBasicInfo64>() / core::mem::size_of::<i32>()) as u32;
    // `mach_task_self()` is a C macro over this global, which libSystem
    // initialises before any Rust code runs. `libc`'s wrapper for it is
    // deprecated in favour of the `mach2` crate; one `static` is not worth a
    // dependency.
    unsafe extern "C" {
        static mach_task_self_: libc::mach_port_t;
        fn mach_vm_region(
            target_task: libc::mach_port_t,
            address: *mut libc::mach_vm_address_t,
            size: *mut libc::mach_vm_size_t,
            flavor: i32,
            info: *mut i32,
            info_cnt: *mut u32,
            object_name: *mut libc::mach_port_t,
        ) -> libc::c_int;
    }
    if ptr.is_null() || size == 0 {
        return false;
    }
    let mut addr = ptr as u64;
    let Some(end) = addr.checked_add(size as u64) else {
        return false;
    };
    while addr < end {
        let mut region = addr;
        let mut len: u64 = 0;
        let mut info = VmRegionBasicInfo64::default();
        let mut count = VM_REGION_BASIC_INFO_COUNT_64;
        let mut object: libc::mach_port_t = 0;
        // SAFETY: a read-only query of our own task, whose port name is a
        // plain integer the runtime wrote once at startup. Every out-pointer is
        // a live local; `info` is exactly `count` 32-bit words, which is all
        // the kernel may write for this flavor, and `object` is MACH_PORT_NULL
        // for the basic-info flavor, so there is no port right to release.
        let kr = unsafe {
            mach_vm_region(
                mach_task_self_,
                &raw mut region,
                &raw mut len,
                VM_REGION_BASIC_INFO_64,
                (&raw mut info).cast(),
                &raw mut count,
                &raw mut object,
            )
        };
        // The call returns the first region AT OR ABOVE `addr`, so a region
        // starting past it means `addr` itself is in a hole.
        if kr != libc::KERN_SUCCESS || region > addr || len == 0 {
            return false;
        }
        if info.max_protection == libc::VM_PROT_NONE {
            return false;
        }
        let Some(region_end) = region.checked_add(len) else {
            return true; // the region runs to the top of the address space
        };
        if region_end <= addr {
            return false;
        }
        addr = region_end;
    }
    true
}

pub(super) fn numa_node_count() -> usize {
    1 // sysfs/getcpu wiring lands with arenas (M6)
}

/// `_mi_prim_getenv`: the value of `name` into `out`, no allocation of ours.
pub(super) fn getenv(name: &[u8], out: &mut [u8]) -> Option<usize> {
    // SAFETY: `name` is NUL-terminated (checked by `prim::getenv`). `getenv`
    // returns null or a pointer into the process environment block, which
    // lives for the process and is read here once, byte by byte, stopping at
    // its NUL or at `out.len()`; nothing is written through it. This is not
    // safe against a concurrent `setenv`, which is the standing caveat on
    // `std::env::set_var` as well, and the same call upstream's prim makes.
    unsafe {
        let v: *const u8 = libc::getenv(name.as_ptr().cast()).cast();
        if v.is_null() {
            return None;
        }
        let mut n = 0;
        while n < out.len() {
            let b = *v.add(n);
            if b == 0 {
                return Some(n);
            }
            out[n] = b;
            n += 1;
        }
    }
    None // longer than the buffer: not an option value
}

/// Every entry of `environ`, in order; see `prim::env_for_each`.
#[cfg(target_os = "linux")]
pub(super) fn env_for_each(mut f: impl FnMut(*const u8)) {
    unsafe extern "C" {
        static environ: *const *const u8;
    }
    // SAFETY: `environ` is the C runtime's NULL-terminated array of
    // NUL-terminated `NAME=VALUE` strings, live for the process. It is only
    // read here, and each entry is handed on as a pointer, not copied. As with
    // `getenv` above, this is not safe against a concurrent `setenv`.
    unsafe {
        let mut p = environ;
        if p.is_null() {
            return;
        }
        while !(*p).is_null() {
            f(*p);
            p = p.add(1);
        }
    }
}

#[inline]
pub(super) fn thread_id() -> usize {
    // SAFETY: no preconditions; pthread_self is async-signal-safe.
    (unsafe { libc::pthread_self() }) as usize
}

pub(super) fn clock_now() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: out-param is a valid local.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

pub(super) struct TlsSlotImpl(libc::pthread_key_t);

pub(super) fn tls_new(dtor: Option<TlsDtor>) -> Option<TlsSlotImpl> {
    let mut key: libc::pthread_key_t = 0;
    // SAFETY: out-param is a valid local; dtor has the exact pthread signature
    // and fires at thread exit for non-null values.
    let r = unsafe { libc::pthread_key_create(&mut key, dtor) };
    if r == 0 { Some(TlsSlotImpl(key)) } else { None }
}

#[inline]
pub(super) fn tls_get(slot: &TlsSlotImpl) -> *mut c_void {
    // SAFETY: key came from a successful pthread_key_create, never deleted.
    unsafe { libc::pthread_getspecific(slot.0) }
}

#[inline]
pub(super) fn tls_set(slot: &TlsSlotImpl, value: *mut c_void) {
    // SAFETY: as tls_get.
    unsafe { libc::pthread_setspecific(slot.0, value) };
}

pub(super) fn tls_raw(slot: &TlsSlotImpl) -> usize {
    slot.0 as usize
}

pub(super) fn tls_from_raw(raw: usize) -> TlsSlotImpl {
    TlsSlotImpl(raw as libc::pthread_key_t)
}
