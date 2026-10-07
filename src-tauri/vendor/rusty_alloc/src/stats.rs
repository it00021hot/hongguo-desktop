//! Stats reporting (mirrors `stats.c`): per-heap counters live in
//! [`crate::heap::Stats`]; this module aggregates them across the global heap
//! registry, formats reports through the output hook, and queries process
//! metrics (`mi_process_info`).

use crate::heap::Stats;
#[cfg(feature = "std")]
use crate::options::out_fmt;

/// Sum the counters of every registered heap (`mi_stats_merge` semantics —
/// our counters are always per-heap, so the merged view is computed on read).
pub fn merged() -> Stats {
    let mut total = Stats::new();
    crate::init::for_each_heap(&mut |h| {
        let s = h.stats;
        total.allocs += s.allocs;
        total.frees += s.frees;
        total.generic += s.generic;
        total.pages_fresh += s.pages_fresh;
        total.segments += s.segments;
        total.huge_allocs += s.huge_allocs;
        total.extends += s.extends;
        total.large_allocs += s.large_allocs;
        total.pages_retired += s.pages_retired;
        total.segments_freed += s.segments_freed;
        total.realloc_in_place += s.realloc_in_place;
        total.realloc_moved += s.realloc_moved;
        total.delayed_frees += s.delayed_frees;
        total.reclaims += s.reclaims;
    });
    total
}

/// std-only: the line is built as an owned string. See `options::out_fmt` for
/// the seam a `no_std` consumer uses instead.
#[cfg(feature = "std")]
fn print_one(label: &str, s: &Stats) {
    out_fmt(&std::format!(
        "{label}: allocs {} frees {} (generic {}), pages fresh {} retired {}, \
         segments {} freed {}, large {} huge {}, realloc {}/{} (in-place/moved), \
         delayed {} reclaims {}\n",
        s.allocs,
        s.frees,
        s.generic,
        s.pages_fresh,
        s.pages_retired,
        s.segments,
        s.segments_freed,
        s.large_allocs,
        s.huge_allocs,
        s.realloc_in_place,
        s.realloc_moved,
        s.delayed_frees,
        s.reclaims,
    ));
}

/// `mi_stats_print` / `mi_stats_print_out`: process-wide (merged) stats.
#[cfg(feature = "std")]
pub fn print_process() {
    let m = merged();
    print_one("heap stats (process)", &m);
    let (elapsed, user, sys, rss, peak_rss, commit, peak_commit, faults) = process_info();
    out_fmt(&std::format!(
        "process: elapsed {elapsed} ms, user {user} ms, sys {sys} ms, rss {} KiB (peak {}), \
         commit {} KiB (peak {}), faults {faults}\n",
        rss / 1024,
        peak_rss / 1024,
        commit / 1024,
        peak_commit / 1024,
    ));
}

/// `mi_thread_stats_print_out`: the calling thread's heap only.
#[cfg(feature = "std")]
pub fn print_thread() {
    let s = crate::alloc::stats();
    print_one("heap stats (thread)", &s);
}

/// `mi_stats_reset`: zero the CALLING thread's counters (per-heap model).
pub fn reset() {
    // SAFETY: own heap.
    unsafe {
        let hb = crate::init::heap_box();
        (*(*hb).heap.get()).stats = Stats::new();
    }
}

/// `mi_process_info`: (elapsed_ms, user_ms, system_ms, current_rss, peak_rss,
/// current_commit, peak_commit, page_faults). Best-effort per platform;
/// unknown fields read 0.
pub fn process_info() -> (usize, usize, usize, usize, usize, usize, usize, usize) {
    #[cfg(all(windows, not(miri)))]
    {
        win_process_info()
    }
    // `feature = "std"`: `unix_process_info` reads `/proc/self/statm` through
    // `std::fs`, so a `no_std` build on a unix host cannot take this arm.
    #[cfg(all(unix, feature = "std", not(miri)))]
    {
        unix_process_info()
    }
    // Miri, wasm, and bare metal: no process accounting to report. Wasm has no
    // RSS concept distinct from the size of linear memory, and no host time; a
    // firmware has no process at all — there is no `/proc`, no `GetProcessMemoryInfo`
    // and nothing the numbers would mean. The doc above already promises
    // "unknown fields read 0", so this is the contract, not a stub.
    //
    // The bare-metal arm is the fifth this four-way selection was missing, the
    // same shape P0 found in `prim/mod.rs` and P3 found in `random.rs`.
    #[cfg(any(
        miri,
        all(target_arch = "wasm32", not(miri)),
        // Bare metal AND unix-without-`std`: no `/proc` reachable either way.
        all(
            not(miri),
            not(windows),
            not(target_arch = "wasm32"),
            not(all(unix, feature = "std"))
        )
    ))]
    {
        (0, 0, 0, 0, 0, 0, 0, 0)
    }
}

#[cfg(all(windows, not(miri)))]
fn win_process_info() -> (usize, usize, usize, usize, usize, usize, usize, usize) {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    // SAFETY: out-params are valid locals; pseudo-handle needs no rights.
    unsafe {
        let proc = GetCurrentProcess();
        let (mut c, mut e, mut k, mut u) = ([0u32; 2], [0u32; 2], [0u32; 2], [0u32; 2]);
        GetProcessTimes(
            proc,
            c.as_mut_ptr().cast(),
            e.as_mut_ptr().cast(),
            k.as_mut_ptr().cast(),
            u.as_mut_ptr().cast(),
        );
        let ft = |t: [u32; 2]| (u64::from(t[1]) << 32 | u64::from(t[0])) / 10_000; // 100ns → ms
        let mut pmc: PROCESS_MEMORY_COUNTERS = core::mem::zeroed();
        pmc.cb = core::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        K32GetProcessMemoryInfo(proc, &mut pmc, pmc.cb);
        let now = crate::prim::clock_now() / 1_000_000;
        (
            now as usize,
            ft(u) as usize,
            ft(k) as usize,
            pmc.WorkingSetSize,
            pmc.PeakWorkingSetSize,
            pmc.PagefileUsage,
            pmc.PeakPagefileUsage,
            pmc.PageFaultCount as usize,
        )
    }
}

#[cfg(all(unix, feature = "std", not(miri)))]
fn unix_process_info() -> (usize, usize, usize, usize, usize, usize, usize, usize) {
    // SAFETY: out-param is a valid local.
    unsafe {
        let mut ru: libc::rusage = core::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut ru);
        let ms = |tv: libc::timeval| (tv.tv_sec as usize) * 1000 + (tv.tv_usec as usize) / 1000;

        // RSS is the one field with no portable Unix source, and BOTH of the
        // obvious readings are wrong on Darwin:
        //
        //   * `/proc/self/statm` does not exist (no procfs) — current_rss read
        //     back as 0, which is what `process_info rss` caught;
        //   * `ru_maxrss` is in BYTES on macOS/BSD but KiB on Linux, so the
        //     ×1024 below silently over-reports peak RSS by 1024× there. That
        //     one is the more dangerous of the two: a plausible-looking number
        //     rather than an obviously-absent one, in the exact field the
        //     README lists as still unmeasured.
        //
        // Darwin gets the mach task info instead, which reports both figures in
        // bytes from one call.
        #[cfg(target_vendor = "apple")]
        // `libc::mach_task_self_` is deprecated in favour of the `mach2` crate.
        // Taking that dependency to read one port constant would be a poor
        // trade for an allocator whose selling point is a dependency-free tree;
        // the symbol is a stable part of libSystem and is not going anywhere.
        #[allow(deprecated)]
        let (current_rss, peak_rss) = {
            let mut info: libc::mach_task_basic_info = core::mem::zeroed();
            let mut count = libc::MACH_TASK_BASIC_INFO_COUNT;
            // SAFETY: `info` is a valid, correctly-sized out-buffer for the
            // MACH_TASK_BASIC_INFO flavor and `count` states its length in
            // natural_t units; mach_task_self_ is the task port for this
            // process. Fields are read by copy (the struct is repr(packed(4)),
            // so no reference into it is ever formed).
            let kr = libc::task_info(
                libc::mach_task_self_,
                libc::MACH_TASK_BASIC_INFO as libc::task_flavor_t,
                (&raw mut info).cast(),
                &mut count,
            );
            if kr == libc::KERN_SUCCESS {
                (info.resident_size as usize, info.resident_size_max as usize)
            } else {
                (0, 0) // report "unknown" rather than a fabricated number
            }
        };
        #[cfg(not(target_vendor = "apple"))]
        let (current_rss, peak_rss) = {
            // Linux: ru_maxrss is KiB; current RSS is field 2 of statm, in pages.
            let peak = (ru.ru_maxrss as usize) * 1024;
            let current = std::fs::read_to_string("/proc/self/statm")
                .ok()
                .and_then(|s| {
                    s.split_whitespace()
                        .nth(1)
                        .and_then(|v| v.parse::<usize>().ok())
                })
                .map_or(0, |pages| pages * crate::os::page_size());
            (current, peak)
        };

        let now = crate::prim::clock_now() / 1_000_000;
        (
            now as usize,
            ms(ru.ru_utime),
            ms(ru.ru_stime),
            current_rss,
            peak_rss,
            0,
            0,
            (ru.ru_majflt + ru.ru_minflt) as usize,
        )
    }
}
