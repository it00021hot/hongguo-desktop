//! Options table, environment parsing, and the registered hooks (mirrors
//! `options.c`). Option INDICES are ABI: the enum matches the oracle v2.4.5
//! ordering exactly, deprecated slots included.
//!
//! Env: `MIMALLOC_<NAME>` (compat) and `RUSTY_ALLOC_<NAME>` (ours) — e.g.
//! `MIMALLOC_SHOW_STATS=1`, `MIMALLOC_PURGE_DELAY=0`. Parsed once on first
//! option access. Values follow mimalloc: booleans accept 1/0/true/false/
//! yes/no/on/off; sizes are plain integers (`_size` options are KiB).

use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

// The only two 64-bit atomics left in the crate, and the only two whose width
// is a CONTRACT rather than a choice (P3 of `docs/plans/small-metal.md`):
// `VALUES` backs `options::{get,set}`, which are `i64` in an API frozen at
// v2.0.0, and `HEARTBEAT` is handed to a registered `DeferredFreeFun` whose C
// ABI declares it `u64`. Every other 64-bit atomic in the crate was a bitmap
// and was narrowed to `u32` instead. On a target with the real thing this is
// `core`; only 32-bit RISC-V / Xtensa pull the shim, and only there.
#[cfg(target_has_atomic = "64")]
use core::sync::atomic::{AtomicI64, AtomicU64};
// With `std` on a 32-bit target, threads are real and the shim's lock table is
// the honest answer. Without it, the crate already serves `no_std` only on
// single-threaded targets — that is what `lib.rs`'s `SingleThreadCell` rests on
// — so paying `portable_atomic`'s lock table (measured at **4,288 bytes of
// BSS** in a shipped XIAO ESP32-S3 firmware, larger than the heap descriptor
// the allocator starts with) buys atomicity nothing can observe. Two `u32`
// halves cost 8 bytes and no lock. See `split64` below.
#[cfg(all(not(target_has_atomic = "64"), feature = "std"))]
use portable_atomic::{AtomicI64, AtomicU64};
#[cfg(all(not(target_has_atomic = "64"), not(feature = "std")))]
use split64::{AtomicI64, AtomicU64};

/// A 64-bit atomic as two `AtomicU32` halves, for single-threaded `no_std`.
///
/// **Sound only because the crate is single-threaded wherever it is used** —
/// the same standing assumption as `lib.rs`'s `SingleThreadCell`, `prim::fixed`'s
/// constant thread id, and its never-contended spin lock. A `no_std` build on a
/// target that grows threads must revisit all four together. Nothing here is
/// `unsafe`: a struct of `AtomicU32` is `Sync` already, so this adds no unsafe
/// to the crate.
///
/// Only the four operations `options.rs` actually performs are provided; a
/// fifth would need its own thought about which half moves first.
///
/// **Orderings are normalised, not forwarded.** A caller's `Ordering` describes
/// one 64-bit access; this performs two 32-bit ones, so there is nothing
/// faithful to forward it to. Forwarding is also a panic: `AtomicU32::load`
/// rejects `Release`/`AcqRel` and `store` rejects `Acquire`/`AcqRel`, so the
/// `compare_exchange(.., AcqRel, ..)` that `set_default` performs would abort
/// the firmware. Loads use `Acquire` and stores `Release` — valid for every
/// caller, and stronger than a single-threaded target can observe.
// `test` in the cfg so the module COMPILES AND ITS TEST RUNS on the host. Gated
// only on the target that uses it, the test below would never execute anywhere
// CI or a developer runs — and a test that cannot run is worse than no test,
// because it looks like coverage.
#[cfg(any(all(not(target_has_atomic = "64"), not(feature = "std")), test))]
mod split64 {
    use core::sync::atomic::{AtomicU32, Ordering};

    /// Split a `u64` into `(lo, hi)` and back. Free-standing so both wrappers
    /// share one definition of which half is which.
    const fn split(v: u64) -> (u32, u32) {
        (v as u32, (v >> 32) as u32)
    }
    const fn join(lo: u32, hi: u32) -> u64 {
        ((hi as u64) << 32) | lo as u64
    }

    #[derive(Debug)]
    pub struct AtomicU64 {
        lo: AtomicU32,
        hi: AtomicU32,
    }

    impl AtomicU64 {
        pub const fn new(v: u64) -> Self {
            let (lo, hi) = split(v);
            Self {
                lo: AtomicU32::new(lo),
                hi: AtomicU32::new(hi),
            }
        }
        pub fn load(&self, _ord: Ordering) -> u64 {
            join(
                self.lo.load(Ordering::Acquire),
                self.hi.load(Ordering::Acquire),
            )
        }
        pub fn store(&self, v: u64, _ord: Ordering) {
            let (lo, hi) = split(v);
            self.lo.store(lo, Ordering::Release);
            self.hi.store(hi, Ordering::Release);
        }
        pub fn fetch_add(&self, v: u64, ord: Ordering) -> u64 {
            let prev = self.load(ord);
            self.store(prev.wrapping_add(v), ord);
            prev
        }
    }

    /// The signed half of the same thing: options are `i64` in an API frozen at
    /// v2.0.0, and the bit pattern round-trips exactly.
    #[derive(Debug)]
    pub struct AtomicI64(AtomicU64);

    impl AtomicI64 {
        pub const fn new(v: i64) -> Self {
            Self(AtomicU64::new(v as u64))
        }
        pub fn load(&self, ord: Ordering) -> i64 {
            self.0.load(ord) as i64
        }
        pub fn store(&self, v: i64, ord: Ordering) {
            self.0.store(v as u64, ord);
        }
        /// `Ordering` pair mirrors `core`'s signature; single-threaded, so the
        /// read-compare-write cannot be interleaved.
        pub fn compare_exchange(
            &self,
            current: i64,
            new: i64,
            success: Ordering,
            _failure: Ordering,
        ) -> Result<i64, i64> {
            let seen = self.load(success);
            if seen == current {
                self.store(new, success);
                Ok(seen)
            } else {
                Err(seen)
            }
        }
    }

    /// The orderings `options.rs` actually passes, exercised so a future caller
    /// forwarding `AcqRel` cannot reintroduce the panic the module doc names.
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn every_ordering_options_uses_is_accepted() {
            let v = AtomicI64::new(i64::MIN);
            v.store(-1, Ordering::Release);
            assert_eq!(v.load(Ordering::Acquire), -1);
            // `set_default`'s ordering pair — the one that would abort.
            assert_eq!(
                v.compare_exchange(-1, 7, Ordering::AcqRel, Ordering::Acquire),
                Ok(-1)
            );
            assert_eq!(v.load(Ordering::Acquire), 7);
            assert_eq!(
                v.compare_exchange(-1, 9, Ordering::AcqRel, Ordering::Acquire),
                Err(7)
            );

            // Halves join in the right order across the 32-bit boundary.
            let u = AtomicU64::new(u64::from(u32::MAX));
            assert_eq!(u.fetch_add(1, Ordering::Relaxed), u64::from(u32::MAX));
            assert_eq!(u.load(Ordering::Relaxed), 1u64 << 32);
        }
    }
}

/// Number of options (== `_mi_option_last` in v2.4.5).
pub const OPTION_COUNT: usize = 38;

/// Index of `generic_collect` in [`OPTION_NAMES`] — how many trips of the
/// allocator's generic (slow) path go by between automatic collects.
///
/// Named because it is the one option the allocator reads on its own hot-ish
/// path. It was declared with a default of 10,000 and **read by nothing** until
/// P4d of `docs/plans/small-metal.md`, which is why a small-profile heap could
/// starve on its own per-class page cache and never recover.
pub const GENERIC_COLLECT: usize = 36;
const _: () = assert!(GENERIC_COLLECT < OPTION_COUNT);

/// Default trips of the generic path between automatic collects.
///
/// Upstream's 10,000 is tuned for a segment of 512 slices, where one cached
/// page per size class is 14 % of the segment and waiting is free. At the small
/// profile a segment holds 16, so ~24 classes is every slice there is — and the
/// P4d stress battery makes only ~649 generic trips in TOTAL, so a 10,000-trip
/// timer never fires at all before the heap has starved.
///
/// **This is the cheap half of upstream's `retire_expire`.** That mechanism
/// gives each retired page its own countdown, decremented on every generic
/// trip, so a sole empty page ages out after ~16 rather than waiting for a
/// sweep. Implementing it means tracking a retired-bin range on the heap, and
/// `alloc::retire_or_abort` is deliberately written to decide keep-one-warm
/// from the PAGE's own links precisely so it never has to resolve the heap —
/// a measured optimisation. Since `collect` now reclaims a bin's last page,
/// a short sweep period buys the same ageing without touching that path.
/// Upstream's per-page countdown stays unimplemented and is recorded in
/// `docs/plans/small-metal.md` §6.
/// **The period is a TRADE, and on bare metal it is the only lever over it.**
/// A sweep returns an empty page to its segment; the next allocation of that
/// class then carves and extends a fresh one. In a tight alloc/free loop that
/// keeps one block live, the page IS empty at every sweep, so a short period
/// turns into steady page churn — measured on a host at exactly
/// `100_000 / 512 = 195` carve-and-retire cycles per 100,000 allocations on
/// the `direct[]` route (`size <= SMALL_SIZE_MAX`), against **zero** on the
/// bin route just above it. That is the visible half of the 512-byte step the
/// Kairos RTOS reported (`docs/plans/finished/fixed-prim-small-step.md`);
/// raising this option takes the churn to zero.
///
/// Which way to err is workload-dependent and neither answer is free. Short
/// sweeps cost page churn; long ones cost capacity, which is the defect the
/// 512 exists to prevent. **Do not raise it because a benchmark that keeps one
/// block live got faster** — that workload cannot decay.
///
/// Shipped geometry: upstream's 10,000.
#[cfg(not(ra_small_profile))]
pub const GENERIC_COLLECT_DEFAULT: i64 = 10_000;
/// Small profile: 512 by default, `--cfg ra_generic_collect="N"` to move it.
///
/// **The cfg exists because this doc used to tell a firmware to "change the
/// default it is built with" and there was no way to.** `options::set` is a
/// no-op under `crate::ONE_REGION` — options are compile-time constants there,
/// which is what lets the whole option table leave a firmware image — so a
/// bare-metal consumer could neither set it at run time nor override the
/// built-in. Now it can, and the value is still a constant the linker folds.
#[cfg(ra_small_profile)]
pub const GENERIC_COLLECT_DEFAULT: i64 = if cfg!(ra_generic_collect = "64") {
    64
} else if cfg!(ra_generic_collect = "4096") {
    4096
} else if cfg!(ra_generic_collect = "65536") {
    65_536
} else {
    512
};

/// Option names in ABI index order (also the env-var suffixes, uppercased).
pub const OPTION_NAMES: [&str; OPTION_COUNT] = [
    "show_errors",
    "show_stats",
    "verbose",
    "eager_commit",
    "arena_eager_commit",
    "purge_decommits",
    "allow_large_os_pages",
    "reserve_huge_os_pages",
    "reserve_huge_os_pages_at",
    "reserve_os_memory",
    "deprecated_segment_cache",
    "deprecated_page_reset",
    "abandoned_page_purge",
    "deprecated_segment_reset",
    "eager_commit_delay",
    "purge_delay",
    "use_numa_nodes",
    "disallow_os_alloc",
    "os_tag",
    "max_errors",
    "max_warnings",
    "max_segment_reclaim",
    "destroy_on_exit",
    "arena_reserve",
    "arena_purge_mult",
    "purge_extend_delay",
    "abandoned_reclaim_on_free",
    "disallow_arena_alloc",
    "retry_on_oom",
    "visit_abandoned",
    "guarded_min",
    "guarded_max",
    "guarded_precise",
    "guarded_sample_rate",
    "guarded_sample_seed",
    "target_segments_per_thread",
    "generic_collect",
    "allow_thp",
];

const DEFAULTS: [i64; OPTION_COUNT] = [
    0,  // show_errors
    0,  // show_stats
    0,  // verbose
    1,  // eager_commit
    2,  // arena_eager_commit
    1,  // purge_decommits
    0,  // allow_large_os_pages
    0,  // reserve_huge_os_pages
    -1, // reserve_huge_os_pages_at
    0,  // reserve_os_memory (KiB)
    // abandoned_page_purge defaults ON (upstream does the same). An abandoned
    // segment has no owner to reuse its pages, so holding them resident buys
    // nothing and costs 32 MiB a time — the RSS tail measured against mimalloc.
    0,
    0,
    0,
    1,         // deprecated x3 / abandoned_page_purge(1)
    1,         // eager_commit_delay
    -1,        // purge_delay: v1 ships purging OPT-IN (see LEDGER M8 open defect)
    0,         // use_numa_nodes
    0,         // disallow_os_alloc
    100,       // os_tag
    32,        // max_errors
    32,        // max_warnings
    10,        // max_segment_reclaim (%)
    0,         // destroy_on_exit
    1_048_576, // arena_reserve (KiB = 1 GiB)
    10,        // arena_purge_mult
    1,         // purge_extend_delay
    1,         // abandoned_reclaim_on_free
    0,         // disallow_arena_alloc
    400,       // retry_on_oom (ms)
    0,         // visit_abandoned
    0,
    0,
    0,                       // guarded_min/max/precise
    1000,                    // guarded_sample_rate
    0,                       // guarded_sample_seed
    0,                       // target_segments_per_thread
    GENERIC_COLLECT_DEFAULT, // generic_collect
    1,                       // allow_thp
];

static VALUES: [AtomicI64; OPTION_COUNT] = [const { AtomicI64::new(i64::MIN) }; OPTION_COUNT];
static ENV_PARSED: AtomicBool = AtomicBool::new(false);

/// Parse the environment once. The common case — every call after the first —
/// is one plain load and a branch, inlined into the caller.
///
/// This used to be the `swap` below on EVERY call: a locked read-modify-write
/// behind an out-of-line call, on every `options::get`. `span_free` reads
/// `purge_delay` on every span it frees, so a 2 MiB alloc/free pair paid
/// **19 Ir in `ensure_init`** per operation (callgrind, opscan `huge`) to
/// learn that the environment had already been read. The swap stays, in the
/// cold arm, as what decides WHICH thread runs the pass.
#[inline(always)]
fn ensure_init() {
    if !ENV_PARSED.load(Ordering::Acquire) {
        ensure_init_slow();
    }
}

#[cold]
#[inline(never)]
fn ensure_init_slow() {
    if ENV_PARSED.swap(true, Ordering::AcqRel) {
        return;
    }
    // A firmware has no environment, no owned strings and no formatter, so
    // without `std` every option keeps its compiled-in default — the whole of
    // the no_std option story (P3 of `docs/plans/small-metal.md`, §2.5). This
    // is deletion, not a port: there is nothing to read, so the environment
    // pass does not exist rather than existing and returning nothing.
    for i in 0..OPTION_COUNT {
        VALUES[i].store(DEFAULTS[i], Ordering::Release);
    }
    // ...and neither has `wasm32-unknown-unknown`. `std::env::var` there is a
    // stub that always fails, so the pass used to format 76 strings, allocate
    // 76 `String`s and read an environment that cannot exist — on every
    // startup, to find nothing. It also dragged `core::fmt`,
    // `alloc::fmt::format` and `str::to_uppercase` into a module that
    // otherwise needs none of them: `options::get` was the LARGEST function in
    // a wasm build at 3,708 bytes, ahead of anything in the allocator proper.
    // Same deletion as the `no_std` arm above, for the same reason — there is
    // nothing to read. (The pass owns no memory any more, see `env`, but the
    // wasm size ratchet is kept exactly where it was by leaving it out.)
    //
    // `target_os = "unknown"` and not `target_arch` alone: wasm32-wasip1 does
    // have an environment and keeps the pass.
    #[cfg(all(
        feature = "std",
        not(all(target_arch = "wasm32", target_os = "unknown"))
    ))]
    env::pass();
}

/// The environment pass, and why it owns no memory.
///
/// It used to be `OPTION_NAMES[i].to_uppercase()` and two `format!`ed keys
/// handed to `std::env::var`, every one of which returns an owned `String`:
/// 38 options x (one uppercase name, two keys, up to two values) made **251
/// allocations through the global allocator on the first allocation of every
/// process**, measured from a consumer behind a counting `GlobalAlloc`
/// (`docs/plans/youslowbro.md` §4), where mimalloc makes none. Each one landed
/// in the heap that was still being set up — allocator re-entrancy during
/// initialisation — and every short CLI run paid all of them.
///
/// Now the key is built by hand in a stack buffer sized by the table, the
/// value lands in a second stack buffer through `prim::getenv` — the raw
/// `getenv` / `GetEnvironmentVariableA` that upstream's prim calls — and the
/// parse reads the bytes in place. Zero allocations, and
/// `crates/rusty_alloc_api/tests/reentrancy.rs` fails if one comes back.
#[cfg(all(
    feature = "std",
    not(all(target_arch = "wasm32", target_os = "unknown"))
))]
mod env {
    use core::sync::atomic::Ordering;

    use super::{OPTION_COUNT, OPTION_NAMES, VALUES};

    /// Longest option name: the key buffer is sized by the table, not by hand.
    const MAX_NAME_LEN: usize = {
        let mut m = 0;
        let mut i = 0;
        while i < OPTION_COUNT {
            if OPTION_NAMES[i].len() > m {
                m = OPTION_NAMES[i].len();
            }
            i += 1;
        }
        m
    };
    /// `RUSTY_ALLOC_` is the longer prefix; `+ 1` for the NUL a C `getenv` needs.
    const KEY_CAP: usize = "RUSTY_ALLOC_".len() + MAX_NAME_LEN + 1;
    /// Values are integers and booleans; upstream's buffer is 64 bytes as well.
    const VALUE_CAP: usize = 64;

    /// `RUSTY_ALLOC_<NAME>` first, `MIMALLOC_<NAME>` second, as before.
    pub(super) fn pass() {
        #[cfg(all(target_os = "linux", not(miri)))]
        {
            scan_pass();
        }
        #[cfg(not(all(target_os = "linux", not(miri))))]
        {
            keyed_pass();
        }
    }

    /// The same pass as ONE walk of the environment (`prim::env_for_each`)
    /// instead of 76 `getenv` calls, each of which walks all of it: 27,011
    /// instructions on the first allocation of every process, 2 % of a
    /// one-line `sort` (callgrind, inclusive). Keeps [`keyed_pass`]'s
    /// semantics exactly: the FIRST occurrence of a key is the one `getenv`
    /// returns, a value too long for `VALUE_CAP` reads as unset (so it falls
    /// back to `MIMALLOC_`), and a present-but-unparsable `RUSTY_ALLOC_` value
    /// keeps the default without consulting `MIMALLOC_`.
    #[cfg(all(target_os = "linux", not(miri)))]
    fn scan_pass() {
        let mut rusty = [core::ptr::null::<u8>(); OPTION_COUNT];
        let mut mi = [core::ptr::null::<u8>(); OPTION_COUNT];
        crate::prim::env_for_each(|e| {
            // SAFETY: `e` is a NUL-terminated environment entry; `strip`
            // and `match_name` stop at the first mismatch, so no read passes
            // its NUL.
            unsafe {
                let (table, rest) = if let Some(r) = strip(e, b"RUSTY_ALLOC_") {
                    (&mut rusty, r)
                } else if let Some(r) = strip(e, b"MIMALLOC_") {
                    (&mut mi, r)
                } else {
                    return;
                };
                for i in 0..OPTION_COUNT {
                    if let Some(v) = match_name(rest, OPTION_NAMES[i].as_bytes()) {
                        if table[i].is_null() {
                            table[i] = v;
                        }
                        break;
                    }
                }
            }
        });
        let mut val = [0u8; VALUE_CAP];
        for i in 0..OPTION_COUNT {
            // SAFETY: each non-null entry points at a NUL-terminated value.
            let got =
                unsafe { copy_value(rusty[i], &mut val).or_else(|| copy_value(mi[i], &mut val)) };
            if let Some(v) = got.and_then(|len| parse_value(&val[..len])) {
                VALUES[i].store(v, Ordering::Release);
            }
        }
    }

    /// `e` past `prefix`, if it starts with it.
    ///
    /// # Safety
    /// `e` NUL-terminated.
    #[cfg(all(target_os = "linux", not(miri)))]
    unsafe fn strip(e: *const u8, prefix: &[u8]) -> Option<*const u8> {
        for (k, &b) in prefix.iter().enumerate() {
            // SAFETY: a mismatch (the NUL included) returns before reading on.
            if unsafe { *e.add(k) } != b {
                return None;
            }
        }
        // SAFETY: every byte up to here matched a non-NUL prefix byte.
        Some(unsafe { e.add(prefix.len()) })
    }

    /// The value after `NAME=` when `rest` is exactly the uppercased `name`
    /// followed by `=` — `getenv`'s case-sensitive match.
    ///
    /// # Safety
    /// `rest` NUL-terminated.
    #[cfg(all(target_os = "linux", not(miri)))]
    unsafe fn match_name(rest: *const u8, name: &[u8]) -> Option<*const u8> {
        for (k, &b) in name.iter().enumerate() {
            // SAFETY: as in `strip`.
            if unsafe { *rest.add(k) } != b.to_ascii_uppercase() {
                return None;
            }
        }
        // SAFETY: as in `strip`.
        unsafe { (*rest.add(name.len()) == b'=').then(|| rest.add(name.len() + 1)) }
    }

    /// `prim::getenv`'s copy: the value into `out`, `None` when `v` is null or
    /// the value does not fit.
    ///
    /// # Safety
    /// `v` null or NUL-terminated.
    #[cfg(all(target_os = "linux", not(miri)))]
    unsafe fn copy_value(v: *const u8, out: &mut [u8]) -> Option<usize> {
        if v.is_null() {
            return None;
        }
        for (n, slot) in out.iter_mut().enumerate() {
            // SAFETY: stops at the value's NUL.
            let b = unsafe { *v.add(n) };
            if b == 0 {
                return Some(n);
            }
            *slot = b;
        }
        None // longer than the buffer: not an option value
    }

    /// One `getenv` per key: every host without `prim::env_for_each`.
    #[cfg_attr(all(target_os = "linux", not(miri)), allow(dead_code))]
    fn keyed_pass() {
        let mut key = [0u8; KEY_CAP];
        let mut val = [0u8; VALUE_CAP];
        for i in 0..OPTION_COUNT {
            let name = OPTION_NAMES[i].as_bytes();
            let n = build_key(&mut key, b"RUSTY_ALLOC_", name);
            let got = lookup(&key[..n], &mut val).or_else(|| {
                let n = build_key(&mut key, b"MIMALLOC_", name);
                lookup(&key[..n], &mut val)
            });
            if let Some(v) = got.and_then(|len| parse_value(&val[..len])) {
                VALUES[i].store(v, Ordering::Release);
            }
        }
    }

    /// `<prefix><NAME>\0`, `name` ASCII-uppercased, into `key`; returns the
    /// length written, NUL included. `KEY_CAP` bounds every combination.
    fn build_key(key: &mut [u8; KEY_CAP], prefix: &[u8], name: &[u8]) -> usize {
        let mut n = 0;
        for &b in prefix {
            key[n] = b;
            n += 1;
        }
        for &b in name {
            key[n] = b.to_ascii_uppercase();
            n += 1;
        }
        key[n] = 0;
        n + 1
    }

    /// One variable, no allocation: `prim::getenv` on a hosted OS.
    #[cfg(all(any(windows, unix), not(miri)))]
    fn lookup(key: &[u8], out: &mut [u8]) -> Option<usize> {
        crate::prim::getenv(key, out)
    }

    /// The same through `std::env` where there is no raw `getenv` to call:
    /// Miri, which interprets `std::env` itself, and wasm32-wasip1. This arm
    /// allocates, and that is accepted — neither is a shipping host.
    #[cfg(not(all(any(windows, unix), not(miri))))]
    fn lookup(key: &[u8], out: &mut [u8]) -> Option<usize> {
        let name = core::str::from_utf8(key.split_last()?.1).ok()?;
        let v = std::env::var_os(name)?;
        let v = v.to_str()?.as_bytes();
        if v.len() >= out.len() {
            return None;
        }
        out[..v.len()].copy_from_slice(v);
        Some(v.len())
    }

    /// mimalloc's value grammar, on the bytes in place: 1/0/true/false/yes/
    /// no/on/off in any case (an empty value is 1), else a decimal integer.
    fn parse_value(v: &[u8]) -> Option<i64> {
        let t = core::str::from_utf8(v).ok()?.trim();
        if ["", "1", "true", "yes", "on"]
            .iter()
            .any(|w| t.eq_ignore_ascii_case(w))
        {
            return Some(1);
        }
        if ["0", "false", "no", "off"]
            .iter()
            .any(|w| t.eq_ignore_ascii_case(w))
        {
            return Some(0);
        }
        t.parse::<i64>().ok()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn keys_are_the_uppercased_names_with_a_nul_and_always_fit() {
            let mut key = [0u8; KEY_CAP];
            let n = build_key(&mut key, b"RUSTY_ALLOC_", b"purge_delay");
            assert_eq!(&key[..n], b"RUSTY_ALLOC_PURGE_DELAY\0");
            let n = build_key(&mut key, b"MIMALLOC_", b"show_stats");
            assert_eq!(&key[..n], b"MIMALLOC_SHOW_STATS\0");
            // The longest name with the longer prefix is exactly what the
            // buffer was sized for; `build_key` would panic past it.
            for name in OPTION_NAMES {
                let n = build_key(&mut key, b"RUSTY_ALLOC_", name.as_bytes());
                assert!(n <= KEY_CAP, "{name}");
                assert_eq!(key[n - 1], 0);
            }
        }

        #[test]
        fn values_follow_mimalloc_grammar() {
            assert_eq!(parse_value(b""), Some(1));
            assert_eq!(parse_value(b"  On "), Some(1));
            assert_eq!(parse_value(b"TRUE"), Some(1));
            assert_eq!(parse_value(b"off"), Some(0));
            assert_eq!(parse_value(b"No"), Some(0));
            assert_eq!(parse_value(b" -1 "), Some(-1));
            assert_eq!(parse_value(b"1048576"), Some(1_048_576));
            assert_eq!(parse_value(b"maybe"), None);
            assert_eq!(parse_value(&[0xff, 0xfe]), None);
        }

        #[test]
        fn lookup_reads_the_process_environment_in_place() {
            // SAFETY: this test is the only writer of these names, and the
            // one reader of the environment in this process — the one-shot
            // option pass — ran when the harness made its first allocation.
            unsafe {
                std::env::set_var("RUSTY_ALLOC_TEST_PROBE", " 42 ");
                std::env::set_var("RUSTY_ALLOC_TEST_LONG_PROBE", "x".repeat(VALUE_CAP));
            }
            let mut out = [0u8; VALUE_CAP];
            let n = lookup(b"RUSTY_ALLOC_TEST_PROBE\0", &mut out).expect("set");
            assert_eq!(&out[..n], b" 42 ");
            assert_eq!(parse_value(&out[..n]), Some(42));
            assert_eq!(lookup(b"RUSTY_ALLOC_TEST_PROBE_UNSET\0", &mut out), None);
            // A value that does not fit is not an option value.
            assert_eq!(lookup(b"RUSTY_ALLOC_TEST_LONG_PROBE\0", &mut out), None);
        }
    }
}

/// `mi_option_get`.
///
/// On a one-region (bare-metal) build this is the compiled-in default, folded
/// at each call site: there is no environment to read and [`set`] is a no-op
/// there, so the 38-entry table of 64-bit atomics — 304 bytes of `.data`,
/// paid once in flash and once in RAM that the linker takes from the stack —
/// and the split-word atomics that emulate it on a 32-bit chip have no
/// reader left and leave the image (`firmware-what-is-left.md` §3).
pub fn get(option: usize) -> i64 {
    if option >= OPTION_COUNT {
        return 0;
    }
    if crate::ONE_REGION {
        return DEFAULTS[option];
    }
    // NOTE (2026-09-24, REFUTED): a "table complete" flag, so the steady
    // state could skip the `i64::MIN` sentinel test below, measured flat on
    // every opscan op and slightly worse on real programs (allocator Ir lua
    // +527, perl +255, sqlite +162). The sentinel is already free where it is
    // hot: the callers test the value's SIGN, `i64::MIN` is negative, and
    // LLVM folds both questions into one `js` (see `span_free`'s
    // `purge_delay` read).
    ensure_init();
    let v = VALUES[option].load(Ordering::Acquire);
    if v == i64::MIN { DEFAULTS[option] } else { v }
}

/// `mi_option_set`.
///
/// A no-op on a one-region (bare-metal) build, where options are compile-time
/// constants (see [`get`]). A firmware that needs a different value changes
/// the default it is built with; there is no environment to read one from
/// and, before this, no known caller setting one at run time.
pub fn set(option: usize, value: i64) {
    if crate::ONE_REGION {
        return;
    }
    if option < OPTION_COUNT {
        ensure_init();
        VALUES[option].store(value, Ordering::Release);
    }
}

/// `mi_option_set_default`: only if still at the built-in default.
/// A no-op on a one-region build, as [`set`].
pub fn set_default(option: usize, value: i64) {
    if crate::ONE_REGION {
        return;
    }
    if option < OPTION_COUNT {
        ensure_init();
        let _ = VALUES[option].compare_exchange(
            DEFAULTS[option],
            value,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

/// `mi_option_is_enabled`.
pub fn is_enabled(option: usize) -> bool {
    get(option) != 0
}

/// `mi_option_get_clamp`.
///
/// Inverted bounds (`min > max`) are swapped, not a panic from `i64::clamp`
/// (OH-rusty_alloc-18).
pub fn get_clamp(option: usize, min: i64, max: i64) -> i64 {
    get(option).clamp(min.min(max), min.max(max))
}

/// `mi_option_get_size`: `_size` options are stored in KiB.
pub fn get_size(option: usize) -> usize {
    let v = get(option).max(0) as usize;
    match option {
        // Overflow is "unrepresentable", not a wrapped small reserve
        // (OH-rusty_alloc-19).
        9 | 23 => v.saturating_mul(1024),
        _ => v,
    }
}

/// `mi_options_print` via the output hook.
///
/// std-only: building the line needs an owned string. A `no_std` consumer that
/// wants this can format into a stack buffer and call [`out_fmt`], which is
/// the seam that survives.
#[cfg(feature = "std")]
pub fn print() {
    ensure_init();
    for (i, name) in OPTION_NAMES.iter().enumerate() {
        out_fmt(&format!("option '{name}': {}\n", get(i)));
    }
}

// ---------------------------------------------------------------------------
// Registered hooks (mi_register_output / _error / _deferred_free)
// ---------------------------------------------------------------------------

/// C output hook signature.
pub type OutputFun = unsafe extern "C" fn(msg: *const core::ffi::c_char, arg: *mut c_void);
/// C error hook signature.
pub type ErrorFun = unsafe extern "C" fn(err: i32, arg: *mut c_void);
/// C deferred-free hook signature.
pub type DeferredFreeFun = unsafe extern "C" fn(force: bool, heartbeat: u64, arg: *mut c_void);

static OUTPUT_FUN: AtomicUsize2 = AtomicUsize2::new();
static ERROR_FUN: AtomicUsize2 = AtomicUsize2::new();
static DEFERRED_FUN: AtomicUsize2 = AtomicUsize2::new();
static HEARTBEAT: AtomicU64 = AtomicU64::new(0);

ra_thread_local! {
    static IN_DEFERRED: Cell<bool> = const { Cell::new(false) };
    static IN_ERROR: Cell<bool> = const { Cell::new(false) };
    static IN_OUTPUT: Cell<bool> = const { Cell::new(false) };
}

/// Re-arms one of the in-hook flags above on scope exit, so a hook that
/// returns early — or unwinds, in a debug build — still clears its guard.
struct HookExit(fn());

impl Drop for HookExit {
    fn drop(&mut self) {
        (self.0)();
    }
}

/// (fn ptr, arg) pair stored as two atomics (registration is set-once-ish;
/// tearing between the two reads yields a stale-but-valid pair).
struct AtomicUsize2 {
    f: AtomicPtr<c_void>,
    a: AtomicPtr<c_void>,
}

impl AtomicUsize2 {
    /// The FUNCTION pointer alone. `load` reads both halves; a caller that
    /// only needs to know whether a hook is registered at all should not pay
    /// for the argument it is not going to use.
    #[inline]
    fn load_fun(&self) -> *mut c_void {
        self.f.load(Ordering::Acquire)
    }

    const fn new() -> Self {
        AtomicUsize2 {
            f: AtomicPtr::new(core::ptr::null_mut()),
            a: AtomicPtr::new(core::ptr::null_mut()),
        }
    }
    fn set(&self, f: *mut c_void, a: *mut c_void) {
        self.a.store(a, Ordering::Release);
        self.f.store(f, Ordering::Release);
    }
    fn load(&self) -> (*mut c_void, *mut c_void) {
        (
            self.f.load(Ordering::Acquire),
            self.a.load(Ordering::Acquire),
        )
    }
}

/// `mi_register_output`.
pub fn register_output(f: Option<OutputFun>, arg: *mut c_void) {
    OUTPUT_FUN.set(f.map_or(core::ptr::null_mut(), |f| f as *mut c_void), arg);
}

/// `mi_register_error`.
pub fn register_error(f: Option<ErrorFun>, arg: *mut c_void) {
    ERROR_FUN.set(f.map_or(core::ptr::null_mut(), |f| f as *mut c_void), arg);
}

/// `mi_register_deferred_free`.
pub fn register_deferred_free(f: Option<DeferredFreeFun>, arg: *mut c_void) {
    DEFERRED_FUN.set(f.map_or(core::ptr::null_mut(), |f| f as *mut c_void), arg);
}

/// Route a message to the registered output hook, else stderr.
///
/// Takes `&str` and needs no allocation, so this SEAM survives `no_std` — a
/// firmware that registers an output hook still gets the allocator's messages
/// over its serial log. Only the *stderr fallback* and the `format!`-based
/// CALLERS are std-only (P3 of `docs/plans/small-metal.md`, §2.5).
pub fn out_fmt(msg: &str) {
    let (f, a) = OUTPUT_FUN.load();
    if f.is_null() {
        // `write_all`, not `eprint!`. The macro formats, and formatting is not
        // free: `core::fmt`, `Display for str` and `Display for u64` are ~2.5 KiB
        // of wasm that this one interpolation of an ALREADY-`&str` argument
        // pulled into every build. Bytes to a writer need none of it.
        #[cfg(feature = "std")]
        {
            use std::io::Write;
            let _ = std::io::stderr().write_all(msg.as_bytes());
        }
        return;
    }
    // A hook that calls `out_fmt` again recurses until the stack dies
    // (OH-rusty_alloc-24).
    if IN_OUTPUT.with(|c| c.get()) {
        return;
    }
    IN_OUTPUT.with(|c| c.set(true));
    let _clear = HookExit(|| IN_OUTPUT.with(|c| c.set(false)));
    // NUL-terminate on the stack for the C hook (bounded copy).
    let bytes = msg.as_bytes();
    let mut buf = [0u8; 512];
    let n = bytes.len().min(511);
    buf[..n].copy_from_slice(&bytes[..n]);
    // SAFETY: f was registered with the documented signature; buf is a valid
    // NUL-terminated C string for the duration of the call.
    unsafe {
        let fun: OutputFun = core::mem::transmute::<*mut c_void, OutputFun>(f);
        fun(buf.as_ptr().cast(), a);
    }
}

/// `"rusty_alloc: error <n>\n"` into `buf`, without a formatter.
///
/// Hand-rendered because `format!` on a single integer is what dragged
/// `core::fmt` into every build; see [`error`]. The buffer is sized for the
/// prefix plus the longest `i32` (`-2147483648`) plus the newline.
fn render_error(buf: &mut [u8; 32], err: i32) -> &str {
    const PREFIX: &[u8] = b"rusty_alloc: error ";
    buf[..PREFIX.len()].copy_from_slice(PREFIX);
    let mut n = PREFIX.len();
    if err < 0 {
        buf[n] = b'-';
        n += 1;
    }
    // `unsigned_abs`: negating `i32::MIN` overflows, and this path must not
    // panic -- it is what runs when something has already gone wrong.
    let mut v = err.unsigned_abs();
    let mut digits = [0u8; 10];
    let mut d = 0;
    loop {
        digits[d] = b'0' + (v % 10) as u8;
        d += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    while d > 0 {
        d -= 1;
        buf[n] = digits[d];
        n += 1;
    }
    buf[n] = b'\n';
    n += 1;
    // `from_utf8`, not `from_utf8_unchecked`: every byte above is ASCII by
    // construction, so this cannot fail -- but validating 30 bytes on a path
    // that only runs when something has already gone wrong is cheaper than
    // adding an `unsafe` to the census for it.
    core::str::from_utf8(&buf[..n]).unwrap_or(
        "rusty_alloc: error
",
    )
}

/// Fire the deferred-free hook (called from the allocation heartbeat).
pub fn deferred_free(force: bool) {
    // Peek at the FUNCTION pointer alone first. `mi_register_deferred_free` is
    // unregistered in nearly every process, and this runs on every slow-path
    // allocation — loading the argument pointer too, only to discard it when
    // there is no hook, is an atomic load spent on nothing.
    if DEFERRED_FUN.load_fun().is_null() {
        return;
    }
    fire_deferred(force);
}

/// Actually call the registered hook.
///
/// Out of line because it is an INDIRECT call, and `deferred_free` inlines
/// into `Heap::malloc_generic`: an indirect call with values live across it
/// forces the whole heartbeat's caller to preserve callee-saved registers, on
/// every slow-path allocation, for a hook that is unregistered in nearly every
/// process. The peek above is all the common path executes.
///
/// The re-entry guard lives HERE, not in `deferred_free`, and that is the
/// point of it. In a cdylib `IN_DEFERRED` is a general-dynamic thread-local,
/// so reading it is a call to `__tls_get_addr` — and LLVM treats that address
/// computation as free to hoist, so with the guard inline it ran ABOVE the
/// null test of the hook pointer: **12 Ir in `__tls_get_addr` on every
/// slow-path allocation**, for a hook no process had registered (callgrind,
/// opscan `huge`/`big`; the disassembly loads `DEFERRED_FUN` and calls
/// `__tls_get_addr` before testing it). Out here it runs only when a hook
/// exists.
#[cold]
#[inline(never)]
fn fire_deferred(force: bool) {
    // A hook that allocates re-enters `malloc_generic` → `deferred_free`.
    // Without a per-thread guard that recurses until the stack dies
    // (OH-rusty_alloc-17).
    if IN_DEFERRED.with(|c| c.get()) {
        return;
    }
    IN_DEFERRED.with(|c| c.set(true));
    let _clear = HookExit(|| IN_DEFERRED.with(|c| c.set(false)));
    let (f, a) = DEFERRED_FUN.load();
    if !f.is_null() {
        let hb = HEARTBEAT.fetch_add(1, Ordering::Relaxed);
        // SAFETY: registered with the documented signature.
        unsafe {
            let fun: DeferredFreeFun = core::mem::transmute::<*mut c_void, DeferredFreeFun>(f);
            fun(force, hb, a);
        }
    }
}

/// Report an error code through the hook (else stderr when show_errors).
pub fn error(err: i32) {
    let (f, a) = ERROR_FUN.load();
    if !f.is_null() {
        // A hook that calls `error` again recurses until the stack dies
        // (OH-rusty_alloc-23).
        if IN_ERROR.with(|c| c.get()) {
            return;
        }
        IN_ERROR.with(|c| c.set(true));
        let _clear = HookExit(|| IN_ERROR.with(|c| c.set(false)));
        // SAFETY: registered with the documented signature.
        unsafe {
            let fun: ErrorFun = core::mem::transmute::<*mut c_void, ErrorFun>(f);
            fun(err, a);
        }
    } else if is_enabled(0) {
        // Rendered into a stack buffer rather than `format!`. The error code is
        // one integer; paying `core::fmt` plus an allocation for it linked the
        // whole formatting machinery into a wasm module that never reports an
        // error. It also means this fallback no longer needs `std`, so a
        // firmware gets back the message P3 had to delete.
        let mut buf = [0u8; 32];
        out_fmt(render_error(&mut buf, err));
    }
}

#[cfg(test)]
mod render_error_tests {
    use super::render_error;

    /// Including the value that makes a naive `-err` overflow.
    #[test]
    fn renders_every_shape_without_a_formatter() {
        let mut b = [0u8; 32];
        assert_eq!(render_error(&mut b, 0), "rusty_alloc: error 0\n");
        assert_eq!(render_error(&mut b, 7), "rusty_alloc: error 7\n");
        assert_eq!(render_error(&mut b, 12345), "rusty_alloc: error 12345\n");
        assert_eq!(render_error(&mut b, -1), "rusty_alloc: error -1\n");
        assert_eq!(
            render_error(&mut b, i32::MAX),
            "rusty_alloc: error 2147483647\n"
        );
        assert_eq!(
            render_error(&mut b, i32::MIN),
            "rusty_alloc: error -2147483648\n"
        );
    }
}
