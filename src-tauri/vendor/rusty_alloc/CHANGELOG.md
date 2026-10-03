# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.2.2](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-v2.2.1...rusty_alloc-v2.2.2) - 2026-10-01

### Fixed

- *(prim)* macOS range_is_reserved via mach_vm_region, not mincore
- *(blockmap)* clear a reused slot's map pointer -- stress_mt aborted on 4 CPUs
- *(prim)* build on macOS -- mincore's out-vector type differs on Apple

### Other

- *(unsafe)* record the two macOS range_is_reserved sites; ratchet 963 -> 965

## [2.2.1](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-v2.2.0...rusty_alloc-v2.2.1) - 2026-09-25

### Fixed

- **Growing a huge block with `realloc` copied its whole reservation.** A
  huge segment's page reported its usable size as the chunk-rounded
  reservation minus the header — 64 MiB for a 33 MB request, 96 MiB for a
  64 MB one — and `realloc` copies `usable_size` bytes when it moves a block,
  so growing a 33 MB block copied, and first-touched on both sides, twice what
  the caller had written; `zalloc` on a recycled chunk zeroed the same extent.
  A consumer measured the two `realloc` steps that cross the segment size at
  **1.41x and 1.23x mimalloc, 0/6 pairs**, with every step below it faster
  than mimalloc, and a `Vec` pushed to 64 MB at 1.23x
  (`docs/plans/youslowbro.md` §3). The page now reports the request rounded
  up to a slice — upstream's `psize`, and what `mi_usable_size` returns there
  — while the reservation itself is unchanged. On the consumer's own
  harness, pinned and ABBA-paired against the tree before the fix: see the
  LEDGER entry for the numbers. `tests/alloc_core.rs::huge_usable_size_is_the_request_not_the_reservation`
  pins the reported size and the bytes a move preserves.
- **The options pass made 251 allocations through the global allocator on
  the first allocation of every process.** `options::ensure_init` built its
  38 x 2 environment keys with `to_uppercase` and `format!` and read them with
  `std::env::var`, every one an owned `String` — inside the heap that was
  still being set up, and on every short CLI run. mimalloc makes none. The
  keys are now built in a stack buffer sized by the table and read through
  `prim::getenv` (a raw `getenv` / `GetEnvironmentVariableA`, the calls
  upstream's prim makes) into a second stack buffer, and the value grammar is
  parsed in place. On the consumer's counting probe the process's start-up
  went from **263 allocations to 12, which is exactly what mimalloc reads
  there**. `tests/options_env.rs` proves the variables still arrive (both
  prefixes, precedence, the boolean grammar, KiB scaling) from a child
  process, and `rusty_alloc_api/tests/reentrancy.rs` fails the build if the
  allocator ever allocates through the global allocator again while serving
  a request (`docs/plans/youslowbro.md` §4).
- **A cross-thread double free could hang the next collect instead of
  aborting.** The README says a double free aborts "on both the local and the
  cross-thread path". The cross-thread arm of that check lived in
  `page_collect` and compared the drained chain's length against `used` —
  AFTER walking the chain. A block freed twice from another thread is linked
  onto `xthread_free` twice, which makes the chain CYCLIC, so the walk never
  reached the compare: the owner's next collect, or on an abandoned page the
  next reclaim, spun forever, and nothing aborted. Now `remote_free` refuses a
  block that is already the chain head — one compare on the cross-thread arm,
  and the local path is byte-identical, checked on the x86-64 release
  assembly for Windows and Linux — and the collect walk is bounded by `used`,
  so the interleaved case (A, B, A) aborts on the first collect instead of
  hanging. Four child-process regressions in `tests/double_free.rs` (a block
  abandoned by a dying thread; `set_default_heap` then exit; a first-class
  heap then exit; interleaved A-B-A), green under `secure`, `blockmap` and
  both. Found by the Openheimer campaign as OH-rusty_alloc-11, whose log
  claimed a fix that had never been written.
- **Thread exit abandoned only the heap in `done_slot`.** A first-class heap
  never `heap_delete`d before its thread exited, or one installed with
  `set_default_heap`, stayed DELAYED under a dead owner: its blocks could be
  freed any number of times onto a delayed list no thread would ever drain,
  and a thread that only ever called `create_heap` had no exit hook at all.
  `create_heap` now bootstraps the thread first and exit abandons every heap
  the thread still owns; upstream deletes non-backing heaps at thread exit
  for the same reason (OH-rusty_alloc-13).
- **Integer wraps a caller could reach with a size, alignment, offset or
  option value, all on cold paths:** `malloc(usize::MAX)` overflowed
  `header + size` in `huge_alloc`; a huge `offset` to `malloc_aligned_at` or
  `realloc_aligned_at` wrapped `p + offset` (debug panic, release wild
  pointer); guarded `malloc(MAX)` wrapped `payload + page`; `os::page_align_up`
  and `os::alloc_aligned` on an unrepresentable size or a garbage alignment;
  `bin_size` of a garbage index; `page_under_utilized` with a huge percentage;
  `options::get_size` KiB scaling; `get_clamp` with inverted bounds;
  `reserve_huge_os_pages` `pages * 1 GiB`; `prim::fixed::dedicated_segments`
  and `region_for` at `usize::MAX`; `segment_map::register_range` within one
  window of the top of the address space (the window walk is bounded);
  `subproc_new` exhaustion and `subproc_add_current_thread` out of range,
  which were `assert!`s on public entries; and `is_aligned_to(x, 0)`, which
  was true for every `x`. Each returns null, `Err`, `false` or a refused id
  instead (OH-rusty_alloc-5, 6, 8, 9, 10, 14, 18–21, 28–34, 42–45).
- **`manage_os_memory` adopted anything it was handed.** Null, a range that
  wrapped, an unmapped address, or a window this allocator already owns all
  became an arena, and the next default `malloc` wrote a `Segment` header
  there. The range must now be non-null, non-wrapping, mapped (`mincore` /
  `VirtualQuery` behind `prim::range_is_reserved`) and not already a
  registered window; a real OS reservation still adopts. `arena_register`
  claims its slot with a CAS and frees the descriptor and the mapping when
  refused; `chunk_alloc_n(0)` is `None`, and `chunk_free_n` / `chunk_free`
  refuse a count past the bitmap or an interior pointer instead of releasing
  a live chunk (OH-rusty_alloc-12, 15, 22, 25, 27, 201, 202).
- **Hook re-entry.** A deferred-free, error or output hook that allocated,
  printed or errored re-entered itself until the stack was gone; each hook
  now runs under a thread-local in-hook flag (OH-rusty_alloc-17, 23, 24).
- **`--release --features debug_checks` did not check.** The foreign-pointer
  guard was a `debug_assert!` inside the `debug_checks` cfg, compiled out of
  the one build a consumer enables the feature for. It is an `assert!`
  (OH-rusty_alloc-7). `malloc_small` / `zalloc_small` forward an oversize
  request instead of `debug_assert!`ing on it (OH-62).
- **OS wrappers on a lie.** `prim::alloc` with a garbage alignment and
  `prim::commit` of null are `Err`; the latter used to hand Windows an
  untracked mapping (OH-rusty_alloc-38, 60).
- **C ABI.** `mi_dupenv_s` / `mi_wdupenv_s` clear the caller's out-pointers
  on EINVAL as the CRT contract says; `mi_realpath` never writes past
  `PATH_MAX`; every out-parameter write refuses a null or misaligned pointer;
  a null, misaligned or immortal-empty heap handle is "no heap" for every
  `mi_heap_*` entry rather than a dereference, and `mi_heap_set_default`
  refuses such a handle instead of installing it (OH-rusty_alloc-16, 26, 54,
  58, 65, 66, 97–102).

### Performance

- **Process start-up: the options pass walks the environment once** (Linux)
  instead of calling `getenv` 76 times, each of which walked all of it —
  about 25,000 fewer instructions on the first allocation of every process
  (a one-line `sort` −1.6 % whole-process), more with a larger environment.
  Same semantics, pinned by `tests/options_env.rs`.
- **Four more instruction-count reductions**, CURIOSITY ROUND FOUR in
  `docs/LEDGER.md`: `posix_memalign` and Rust's over-aligned `alloc` carry the
  aligned fast path inline (opscan `aligned` −9.2 %); a medium allocation pops
  its page before collecting it (`big`/`large` −3.7 %, `mixed` −3.0 %); C++17
  aligned `new` takes the power-of-two fast path; and Rust `realloc` above
  two words of alignment keeps a block in place when it fits instead of
  always moving it (−27.3 % against 2.2.0 on an over-aligned buffer workload).
- **Rust programs: the `GlobalAlloc` entry points were paying for a shim
  and a detour on every call** (`rusty_alloc-api`). The trait methods are now
  `#[inline]`, as the `mimalloc` crate's are, so `__rust_alloc` and friends
  carry the fast paths instead of jumping through the GOT to them; `dealloc`
  carries the free body with its null test folded away; and layouts aligned
  up to two words (16 bytes on x86-64, what every hashbrown table asks for)
  are served from the ordinary size classes, which are already aligned that
  far, instead of the aligned path, and can now `realloc` in place instead of
  always allocating, copying and freeing. Two deterministic Rust workloads
  (`bench/rust-globalalloc.sh`), measured against 2.2.0: **−21.1 %** and **−5.8 %** whole-program
  instructions, for about 2–3 KB of text. `tests/natural_align.rs` pins the
  alignment for every size class through alloc, alloc_zeroed and realloc.
- **Two more on the allocator core**, with the three above making up
  CURIOSITY ROUND THREE in `docs/LEDGER.md`: the page-extend floor now matches upstream's
  `MI_MIN_EXTEND` of four blocks (perl −454,165 whole-program, opscan
  `big`/`large` −4.5 %), and span marking no longer rewrites interior slices
  that already point to their span (opscan `huge` −53 %).
- **Ten deterministic instruction-count reductions** on the allocation and
  free paths, each measured alone under callgrind on opscan and on real
  programs; `docs/LEDGER.md` (CURIOSITY) has every number and the seven
  refutations. The largest: `realloc(NULL, n)` no longer pays the moving
  path's five-register frame — Lua routes every allocation through
  `realloc`, and its allocator instruction count fell **39.9 %** — and
  `malloc_slow` is a tail call again, which with a leaf `calloc` fast path, a
  single-branch keep-one-warm test, a load-first `options::get` and a
  thread-local that LLVM had hoisted above its guard takes opscan
  `big`/`large` −16.5 %, `mixed` −12.9 %, `calloc` −11.9 % and `huge` −7.4 %.
  No op regressed. `stats.delayed_frees` is now counted in debug builds
  only, like `allocs` and `frees`.
- **Ten more, measured the same way plus Python, jq, gawk and sort**
  (`docs/LEDGER.md`, CURIOSITY, ROUND TWO). Cross-thread frees are **31 %
  cheaper** on opscan `xthread`: a remote free to a parked page now releases
  the page to NORMAL after its one delayed-list push, as upstream mimalloc
  does, so later remote frees are a single CAS onto the page's own list
  instead of three CASes and a per-block drain by the owner (single-block
  pages keep the delayed route; the loom model gained a case for it).
  Also: `realloc` decides a move in one compare, a medium allocation whose
  front page is dry grows it without re-running the heartbeat, the generic
  path tail-calls page growth again, `free_general` needs no stack frame,
  and `posix_memalign` / `GlobalAlloc` skip a power-of-two re-test through
  the new `alloc::malloc_aligned_pow2`. perl −456,939 and Python −389,886
  whole-program instructions; opscan `big`/`large`/`mixed` read +0.7…+1.0
  on the medium-grow change against round one's intermediate state and are
  still −1 % over the round.

### Changed

- **`usable_size` (`mi_usable_size`) of a huge block is the request rounded
  up to a slice, no longer the reservation.** Two behaviours follow from that
  and both match upstream: a `realloc` that grows a huge block into what used
  to be reported as slack now moves it instead of returning it in place, and
  `expand` refuses such a growth. A shrink to at least half the request stays
  in place, where before it could MOVE (a 33 MB block shrunk to 20 MB was
  below half of the 64 MiB it reported). The address space reserved for a
  huge block is exactly what it was.
- **What was NOT taken from the Openheimer campaign, and why.** The
  campaign's log carries 202 findings; 139 of them are one probe — an
  internal `unsafe fn` handed null, `0x1`, or an address just past a
  constant "floor" — chased through 25 helpers and up a ladder of floors
  (`0x1000`, `0x10000`, `SEGMENT_SIZE`, then 2×, 3×, 4× that) and finally
  answered with a segment-map membership lookup on every internal handle:
  `page_of`, `page_index`, `page_area`, `free_local`, `retire_emptied`,
  `box_of_xheap` (a registry walk under a global lock, on the free path) and
  twenty more. Those guards validated contracts the caller inside this crate
  already upholds, duplicated the residual the threat model accepts for `free`
  (R-001: release `free` trusts its pointer's window), and taxed the hot path
  — `free` +27, `realloc` +164, `page_extend` +68 instructions when the
  campaign stopped, which it never measured. None of it landed. What did:
  every check on a value a caller can actually choose — sizes, counts,
  alignments, offsets, option values, out-pointers, C strings, heap handles at
  the C ABI, `manage_os_memory` ranges — on paths that are cold or, for
  `free`, cost the local path nothing. x86-64 release assembly against 2.2.0,
  Windows / Linux: `free` +4 / +4 (both on the cross-thread arm; no frame),
  `malloc` 0 / 0, `malloc_aligned_at` 0 / 0, `realloc` +7 / +6,
  `realloc_aligned_at` +1 / +1, `page_extend` 0 / 0, `usable_size` 0 / 0.
  The disposition, finding by finding, is `docs/plans/openheimer-run.md`.
- `bins::is_aligned_to` spells its power-of-two test as
  `align & (align - 1) == 0` rather than `is_power_of_two()`: without `popcnt`
  in the target features the latter was emitted as a 20-instruction SWAR bit
  count inside `realloc_aligned_at` (+43 on that function, now +1).

## [2.2.0](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-v2.1.0...rusty_alloc-v2.2.0) - 2026-09-10

### Fixed

- **The small profile extended every page ONE BLOCK AT A TIME, so 100 % of
  allocations took the slow path.** `page_extend` bounds its batch at 4 KiB of
  payload and computed that bound with a hardcoded shift whose constant term is
  really `SEGMENT_SLICE_SIZE / 4096`. At the shipped 64 KiB slice the literal
  was right; under `--cfg ra_small_profile` the slice is 4 KiB, so the bound
  was **256 bytes instead of 4 KiB** — sixteen times too small. For a 512-byte
  class the batch computed to 0 and was clamped to 1, leaving every page with
  `capacity == 1` and no second block for the fast path to find. Counted over
  100,000 alloc+free pairs at the small profile, entries into `malloc_generic`
  per op: **512 B 1.0000 -> 0.1250, 513 B 1.0000 -> 0.1667, 1 KiB 1.0000 ->
  0.2500**, with page carve-and-retire churn falling from 195 per 100,000 to
  24/32/49. On a 32-bit host the 512-vs-513 step inverts from +8.6 % (slower)
  to −36 % (faster), about **1.9× faster at 512**. **Measured on silicon** too
  — a XIAO ESP32-S3, `main` against the fix on one board with identical
  checksums and floor: **13.0 % faster** on 32 B ping-pong, **15.7 %** on a
  64-block mixed batch, **15.3 %** on 8-512 B churn, and 1.1 % at 2,048 B,
  which is on the bin route this does not touch. Found by the Kairos RTOS
  report (`docs/plans/finished/fixed-prim-small-step.md` §8.7).
  **The default geometry is unchanged** — the derived constant equals the old
  literal there, and the all-features x86-64 assembly diff moves no executable
  function.

### Added

- **`--cfg ra_generic_collect="64" | "4096" | "65536"`**, so a bare-metal
  firmware can move the periodic-collect heartbeat. It is the only lever over a
  real trade — a sweep returns an empty page, and the next allocation of that
  class carves and extends a fresh one, so a short period costs page churn
  while a long one costs capacity — and it was unreachable: `options::set` is a
  no-op under `ONE_REGION`, and the option's own doc told firmwares to "change
  the default it is built with" when no cfg existed. The default does not move.
- **`prim::fixed::shape_of(size) -> Shape`** — page bytes, dedicated segments
  and `direct_route`, `const` and derived from the active geometry. Answers
  "which page kind, and how many region bytes, does this size cost", which
  `region_stats()` cannot because it reports over region extents.
  **`Shape::direct_route` names a boundary that moves with POINTER WIDTH**:
  `SMALL_SIZE_MAX` is `128 * size_of::<usize>()`, so it is 1,024 on a 64-bit
  host and 512 on a 32-bit chip. The Kairos RTOS measured a 16 % step there on
  a device and could not reproduce it on a workstation for exactly that reason
  (`docs/plans/finished/fixed-prim-small-step.md`).

## [2.1.0](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-v2.0.5...rusty_alloc-v2.1.0) - 2026-09-10

### Added

- **`--cfg ra_segment_size="256k"`, for a firmware whose allocation unit is
  tens of kilobytes.** A segment's slice 0 is its header, so the largest object
  that can share a segment is `SEGMENT_SIZE - SEGMENT_SLICE_SIZE` — 61,440
  bytes at the default small profile — and one byte over that takes a dedicated
  run of segments. Since no allocation of `SEGMENT_SIZE` can share a segment
  with its own metadata, a 64 KiB request costs **two** segments and a 128 KiB
  request three. A `rusty_zstd` firmware measured the consequence on an
  ESP32-S3: one 64 KiB block served from a 256 KiB region, the second refused
  with 192 KiB unused. The flag moves the small profile to an 8 KiB slice x 32,
  raising `LARGEST_SHARED_ALLOC` to 253,952 so a 64 KiB request becomes a span
  three of which pack into one segment — **3 blocks instead of 1 in the same
  256 KiB region, measured on the board**. Opt-in, because it doubles the page
  floor `(classes touched) x slice` that a small-object workload pays.
- **`prim::fixed::LARGEST_SHARED_ALLOC`, `dedicated_segments(size)` and
  `region_for_allocs(size, count)`** — the sizing API that predicts the above at
  compile time, instead of leaving a firmware to discover it on silicon.
  `region_for_allocs` also counts the segment the first small allocation claims,
  which is what took the reporting firmware from two blocks to one.
- **`prim::fixed::PrimError`**, re-exported so the whole fixed-region recipe is
  reachable from one path. The type has always been public as
  `prim::PrimError`, but only from the parent module, so a seam re-exporting
  this API in a single `pub use` could name `Region`, `good_region_size`,
  `init_region` and the `FERR_*` values but not the type they fail with. The
  Kairos RTOS allocator seam hit exactly that and carried an "arrives with the
  next release" comment for it. Same type, one more path.
- **`prim::fixed::region_capacity() -> (free_segments, largest_servable)`.**
  `region_stats` reports free BYTES, and free bytes hide this failure: the
  refused allocation above had 126,976 bytes free and read
  `free_segments=1, largest_servable=61440`.

### Fixed

- **The README's footprint model did not cover large allocations and implied
  the opposite of the truth for them.** It described the floor as
  `(classes touched) x (page size)`, "independent of bytes requested" and
  amortising as the working set grows. That holds for small objects; for
  segment-sized ones the cost is a granularity tax that scales with how many
  are live. The section now says so, with the measured numbers and the flag.

## [2.0.5](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-v2.0.4...rusty_alloc-v2.0.5) - 2026-09-09

### Semver note, read this one

**`prim::fixed::Region<N>`'s alignment is reduced**, from `SEGMENT_SIZE`
(32 MiB at the default geometry, 64 KiB under `--cfg ra_small_profile`) to
16 bytes. `cargo-semver-checks` classes a `repr(align)` change as breaking
and would have made this 3.0.0. It ships as a patch deliberately: `Region`
was introduced one release ago in 2.0.4, at the default geometry its old
alignment was 32 MiB and its own docs called that unhonourable by any
chip-sized `.bss`, and the documented use — `static HEAP: Region<N>` then
`HEAP.give()` — cannot observe the change. `size_of::<Region<N>>()`,
`Region::USABLE` and every other signature are untouched.

**If you depend on the old alignment for a reason of your own, set
`--cfg ra_aligned_region`**, which restores the 2.0.4 layout exactly: the
address mask on `free`, `Region` segment-aligned, and the linker gap in
front of it. Nothing else in the public API changed except the added
`REGION_ALIGN` const.

### Changed

- **A firmware's region no longer needs to be segment-aligned, and the
  linker's gap in front of it is gone.** On a fixed region segments are
  carved at `SEGMENT_SIZE` strides from the region's base, and `segment_of`
  masks the offset from that base instead of the address (`REGION_STRIDES`;
  wasm made the same trade with a slice table in 2.0.0). `prim::fixed::Region`
  is 16-byte aligned (`REGION_ALIGN`, new), `usable_bytes` / `MIN_REGION` /
  `good_region_size` are exact from any 16-byte-aligned base, and the
  firmware that reported the gap (`docs/plans/finished/region-alignment-dissolve.md`)
  gets **24,144 bytes of stack back** with `.bss` unchanged — the 24,148 that
  `size -A` could never show. The price is three instructions on every
  `free` on the ESP32-S3 (39 against 36), 9–17 ns per alloc/free pair on the
  board. **`--cfg ra_aligned_region`** (new) keeps the address mask and the
  segment alignment — the 2.0.4 layout, gap included — for a firmware that
  would rather have those; every sizing rule, the backend's placement and
  `Region`'s alignment follow the flag. Hosted builds are unchanged to the
  instruction. `FERR_MISALIGNED` still exists and still fires: a base off
  the 16-byte grid with an exact length loses its last segment to the
  run-up, exactly as before — `Region` cannot produce one. Strictly, a
  type's alignment moving is observable; nothing a firmware does with
  `Region` depends on it.
- The arena layer folds on `FIXED_REGION` (any fixed-region target) rather
  than on `ONE_REGION`: chunks carved on absolute segment boundaries are not
  a strided region's segments.

### Fixed

- **Every `free` on a single-threaded target paid an acquire load and a
  memory barrier for a comparison that had already folded to `true`.** The
  segment's owner thread id was read with `Acquire` before `ONE_THREAD`
  short-circuited the compare, and LLVM keeps an unused acquire; on the
  ESP32-S3 that was an `l32i` and a `memw` on the hottest path in the
  crate. The load is inside the predicate now: −8 ns per alloc/free pair on
  the board, hosted builds unchanged. Found reading the free path's
  disassembly for the change above.

## [2.0.4](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-v2.0.3...rusty_alloc-v2.0.4) - 2026-09-09

### Fixed

- **A region sized by `good_region_size` at an unaligned base silently served
  a segment fewer than its size said** (reported by the Janus firmware:
  `good_region_size(220 * 1024)` at the base the linker chose served two
  segments, not three, and panicked in `handle_alloc_error` 484 bytes short of
  the round number that had happened to work). `init_region` now refuses a
  base that costs a whole segment against what the length promises, with
  `FERR_MISALIGNED`; a round region that strands as much aligned as it loses
  misaligned still passes.
- **The documented fix was worse than the bug, and every sizing rule was the
  reason.** The 2.0.3 rule `k * SEGMENT_SIZE + FIXED_PAGE` reserved a page of
  the region for the first heap's descriptor, so an exact region was never a
  whole number of segments, so a `#[repr(align(65536))]` container of it was
  rounded up to the next segment: 200,704 bytes became 262,144 in `.bss`, and
  the firmware that took the advice lost 60,952 bytes of stack. The first
  heap's descriptor on a one-region target is now a static of the fixed
  backend's (1,752 bytes), and the region is whole segments.

### Added

- `prim::fixed::Region<N>`: the region container a firmware should use.
  `SEGMENT_SIZE`-aligned by construction, `N` checked at compile time to be a
  whole number of segments, `size_of::<Region<N>>() == N` (asserted in the
  crate), `give(&'static self) -> Result<usize, PrimError>` hands it over once
  and returns the usable bytes; `Region::USABLE` for `const` assertions. On the
  rig it replaced the consumer's aligned container for **+63,780 bytes of
  stack** (both aligned, so the linker's gap cancels), and the footprint
  sketch runs on `Region<{ 64 * 1024 }>`: one segment, `65536 usable of
  65536`. Against a round *unaligned* region the honest gain is +2,828 bytes
  of stack for the same usable heap: the alignment gap the linker leaves
  before an aligned static is charged to no section, so a `.bss` delta
  overstates it — the `Region` docs say so.
- `FERR_MISALIGNED` (0xF141), `FIXED_PAGE` (now `pub`),
  `take_first_heap_box` / `is_first_heap_box`.

### Changed

- **The sizing rules lose their `+ FIXED_PAGE`**, which changes what these
  return: `MIN_REGION` is `SEGMENT_SIZE`; `usable_bytes(0, len)` is
  `⌊len / SEGMENT_SIZE⌋ · SEGMENT_SIZE`; `good_region_size(220 * 1024)` is
  196,608 (was 200,704); `region_for(192 * 1024)` is 196,608. A consumer
  `const`-asserting the old literal will fail to build, which is the point:
  the old shape is the padded one. The strict semver reading of a changed
  `const fn` result is a minor bump; it is presented as a fix because the old
  results were the defect.
- The README's recipe declares the region through `Region`, and its floor row
  reads 64 KiB plus the descriptor static (was 68 KiB).

## [2.0.3](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-v2.0.2...rusty_alloc-v2.0.3) - 2026-09-09

### Added

- `prim::fixed::good_region_size(budget)` and `region_for(usable)`, `const fn`s
  that size a firmware's region so the 64 KiB granule strands nothing: the
  largest zero-waste region no bigger than a budget, and the smallest region
  serving at least `usable` bytes. `good_region_size(220 * 1024)` is 200,704
  (three segments plus the page) where a literal 220 KiB stranded 24,576.
- `prim::fixed::region_contains(addr)`, the one-region answer to "is this
  pointer ours".
- `--cfg ra_max_extents="8"` / `"16"` / `"64"` resizes the fixed backend's
  free-extent table (default 32 unchanged). Its doc states the bound: never
  more slots than live blocks plus one.

### Changed

- **Firmware code size, second pass: everything that exists to manage many OS
  ranges folds to what one linker-handed region needs.** A new internal
  predicate (`ONE_REGION`, the bare-metal arm of `ONE_THREAD`) folds arenas,
  the segment map, the runtime option table and the RAM-resident heap sentinel
  on a bare-metal target: `chunk_alloc` is `None`, `segment_map::contains` is
  two compares against the region bounds, `options::get` is the compiled-in
  default at each call site, and the heap sentinel and empty page live in
  `.rodata` (flash) with `create_heap` copying its template from there. On the
  ESP32-S3 firmware measured in `docs/plans/finished/firmware-what-is-left.md`
  the allocator's flash cost went **+7,860 → +3,208 B** and its static RAM
  **+3,052 → +284 B** (attributed code 8,262 → 4,313 B); board throughput is
  unchanged to within 5 ns per operation. Hosted builds are byte-for-byte
  unchanged; the wasm ratchet is flat.
- What a bare-metal build gives up by it, stated: `arena::reserve_os_memory_ex`
  and `manage_os_memory_ex` return `Err` there, and `options::set` /
  `set_default` are no-ops. Neither had a working meaning on a chip before.
- README embedded figures refreshed from a same-day board run of both arms:
  2.80x / 2.17x / 3.98x / 1.45x (the 2.0.0-era rows were 7-9 % stale — 2.0.2's
  free fold had already moved them), the cost table above, the region-sizing
  advice, and the placement caution for kernel comparisons across allocators.

## [2.0.2](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-v2.0.1...rusty_alloc-v2.0.2) - 2026-09-09

### Changed

- **Firmware code size halved: `--cfg ra_single_threaded` now prunes what it
  promises.** The cfg used to change only correctness assumptions; the code
  that a single context can never reach — abandoning a segment when a thread
  ends, adopting one back, the delayed list a cross-thread free lands on, the
  thread-exit hook — was still linked. On an ESP32-S3 firmware built both ways
  from one source, the allocator's flash cost fell from **+16,584 B to
  +7,860 B** and its attributable code from 16,256 B to 8,262 B, measured with
  `size -A` and `nm --size-sort` on the linked ELF, one brick at a time. One
  internal predicate (`ONE_THREAD`: the fixed backend under
  `ra_single_threaded`, or `wasm32-unknown-unknown` without atomics) folds the
  branches so the linker can see it; hosted builds compare thread ids exactly
  as before and are byte-for-byte unchanged.
- The same predicate takes **10.7 % off the gzipped wasm bundle** (22,574 →
  20,169 bytes); the ratchet baseline is updated in the same commit.
- The README's embedded section now carries the flash and static-RAM cost
  beside the heap floor, and the hazard the decomposition exposed: static RAM
  comes straight out of `.stack`, to the byte, so a firmware near its stack
  limit adopts this and gets an overflow rather than a bigger binary.

### Fixed

- **Guarded-object sampling ran on targets that cannot protect a page.**
  `prim::fixed` and `prim::wasm` return `Err` from `protect`; the sampler used
  to accept a rate anyway and hand out a dedicated segment with an unprotected
  trailing page — the full cost of a guarded object and none of the protection
  — while `try_guarded` and the ChaCha block function it samples with stayed in
  a `secure`-off image. Guarded sampling is now compiled out where no guard
  page can exist (`guarded_set_sample_rate` leaves it off there), and a heap's
  CSPRNG is seeded only where something draws from it. On the ESP32-S3 that was
  2,366 bytes of unreachable code plus 1,621 bytes of seeding for a generator
  nothing read.
- **`--features linkcheck` without `secure` did not compile.** The non-secure
  link check still passed the page extent that the dropped narrowing used to
  take. CI built only the default and `--all-features`, so the combination
  rotted unseen; it now builds, and CI clippies each optional feature alone.
- A `prim::fixed` unit test asserted that a `SEGMENT_SIZE`-aligned page cannot
  come out of a region smaller than a segment, which is only true when the
  region does not straddle a boundary — the loader decides that, and on
  2026-09-08 it put CI's 512 KiB window across a 32 MiB line. The allocator was
  right; the test now decides from the address.

## [2.0.1](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-v2.0.0...rusty_alloc-v2.0.1) - 2026-09-08

### Added

- `prim::fixed::MIN_REGION` and `prim::fixed::usable_bytes(base, len)` — a
  firmware can now size its heap region at COMPILE time
  (`const _: () = assert!(N >= MIN_REGION)`) instead of discovering the answer
  on silicon, and can log how much of a region can actually back segments. The
  `k * SEGMENT_SIZE + FIXED_PAGE` rule previously existed only in a design
  document; a 220 KiB region strands 24,576 bytes and nothing said so.
- Distinct `prim::fixed` error codes: `FERR_TOO_SMALL`, `FERR_GEOMETRY`,
  `FERR_REGISTERED`. One sentinel covered three conditions with three different
  fixes.

### Fixed

- **`init_region` accepted a region that could never yield a segment.** Setting
  `--cfg ra_single_threaded` (which the crate demands loudly) without
  `--cfg ra_small_profile` (which nothing demanded) left `SEGMENT_SIZE` at
  32 MiB, so a kilobyte-scale region returned `Ok(())`, linked clean, and then
  failed every allocation on the board with a backtrace pointing at whatever
  allocated first. It now returns `FERR_GEOMETRY`, checked against the real base
  address rather than the length alone — an unaligned base needs up to
  `SEGMENT_SIZE - 1` more than a length test would demand.
- **An allocating interrupt handler hung the firmware silently.**
  `prim::fixed`'s lock is not reentrant, and on a single-context target a lock
  observed held can only mean reentrancy. That is now a panic naming the ISR
  instead of an unbounded spin that surfaces as a watchdog reset. Confirmed with
  a load, because `compare_exchange_weak` may fail spuriously and a bare CAS
  failure would misfire.
- The README documents `--cfg ra_small_profile`, which it never mentioned, and
  attaches it to the 68 KiB figure that is only true under it.

  All four reported by the first outside firmware to adopt 2.0.0
  (`docs/plans/embedded-adoption.md`).

### Changed

- `portable-atomic` is `optional` and enabled by `std`, so it is pulled only
  where it is reachable — a target without 64-bit atomics that also has `std`.
  A `no_std` firmware was fetching and compiling a crate it then discarded at
  link time. Costs nothing on the device either way; it buys an accurate
  dependency graph in an SBOM or a `cargo audit`.

## [2.0.0](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-v1.1.6...rusty_alloc-v2.0.0) - 2026-09-07

### Breaking

Two changes require a major bump. Neither affects a consumer using default
features, which is the overwhelming majority.

- **`default-features = false` now selects `no_std`.** Before this release the
  crate had `default = []`, so `default-features = false` was identical to the
  default and got the full crate. It now selects the single-heap `no_std`
  profile, which additionally refuses to compile without
  `--cfg ra_single_threaded`. If you set `default-features = false` and want
  what you had, set `features = ["std"]`. **Verified against the downstream
  corpus** (`tools/corpus/`): `spacedb-sdk` (plain and `secure`),
  `rusty_alloc_default` and `rusty_zstd` compile unchanged; `rusty_maplibre` is
  broken by this and is fixed by adding `features = ["std"]` to both of its
  `rusty_alloc` dependencies -- tested by applying it, not assumed.
- **`heap::Heap` gained a field** (`generic_countdown`). Every field on it is
  `pub` and it is not `#[non_exhaustive]`, so a struct literal naming all
  fields no longer compiles. Nothing constructs a `Heap` that way in practice —
  it needs raw pointers and a `PageQueue` array — but semver is semver.

### Fixed

- *(heap)* **`collect` could not reclaim a size class's last empty page.** The
  keep-one-page-per-bin reuse cache is `mi_page_retire`'s policy on the free
  path; `collect` had borrowed it, so no collect at any level could return a
  class's page to a different class. Upstream's `mi_heap_page_collect` frees an
  all-free page unconditionally ("this will free retired pages as well").
  Harmless at the default 32 MiB geometry, where 512 slices per segment absorb
  a cached page per class; severe where slices are scarce.
- *(heap)* **nothing ever collected automatically.** `generic_collect` was
  declared as an option with a default of 10,000 and read nowhere. It is now a
  per-heap countdown on the generic path, as upstream.
- *(heap)* **the generic path reported OOM while holding reclaimable memory.**
  It now reclaims once and retries before returning null. Free on the happy
  path — it runs only when the allocation was about to fail.
- *(segment)* an exclusive arena's huge-path allocations could escape to the OS
  instead of failing, diverging from `mi_segment_huge_page_alloc`.
- *(arena)* the chunk bitmap scan stopped at the first word: an arena of more
  than 32 chunks could not allocate past chunk 31.
- *(heap)* `stats.segments` was not incremented on the huge path, so it did not
  balance `segments_freed`.
- *(prim)* the fixed-region backend placed every allocation bottom-up, so one
  page-sized block below a segment boundary cost a whole segment of reach.

### Added

- `no_std` support behind a default-on `std` feature: the core allocator builds
  and runs on bare metal (`riscv32imac`/`imafc` gated in CI, ESP32-S3 measured
  on hardware).
- `ra_small_profile`: a second geometry (4 KiB slices, 64 KiB segments) for
  parts with kilobytes rather than gigabytes. Non-additive, so it is a `--cfg`
  rather than a feature.
- `prim::fixed`, a fixed-region backend for targets with no OS —
  `init_region`, `region_stats`.
- `options::GENERIC_COLLECT`, the index of the `generic_collect` option.
- `segment_map::range_table_overflowed`, an observable latch for the
  small-profile segment map.

### Changed

- 64-bit atomics on targets without them: `no_std` builds use two `AtomicU32`
  halves instead of `portable-atomic`'s lock-based fallback (sound under the
  single-threaded assumption `no_std` already carries), removing the dependency
  from that path.

### Performance

- Medium allocations (above `SMALL_SIZE_MAX`, up to `MEDIUM_OBJ_SIZE_MAX`)
  collect-and-retry the bin queue front before the slow-path heartbeat. Through
  `GlobalAlloc` those sizes reach `malloc_generic` on **1.000** of their calls,
  so the saving lands on every one: **+14-16 % on a 2 KiB tight alloc/free
  loop**, and it survives multithreading (+12-26 % with two threads each freeing
  their own blocks). The retry **turns itself off per heap** once that heap is
  seen receiving cross-thread frees: `free`, `local_free` and `xthread_free` are
  adjacent in a `#[repr(C)]` `Page`, so peeking a page another core is freeing
  into costs 20-30 %, and no variant of the peek avoids it. Measured on all
  three targets that can run it: **+14-16 % native x86-64, +1-16 % wasm32 in
  V8, +15 % on an ESP32-S3** (`no_std`, small profile), with every other
  workload unchanged. Host INSTRUCTION counts under callgrind still pending.
- **On wasm, 4.9-7.3x faster than the Rust default allocator on a churn workload**
  (64 live blocks, random 8-512 B), measured in node with a subtracted harness
  floor and checksums proving work parity. A tight 2 KiB same-size loop is
  ~0.7x, for a reason `alloc.rs` already documents as measured-and-refuted.
- **wasm modules are 8,551 bytes smaller (3,931 gzipped)** — the allocator's
  gzipped overhead on a minimal consumer falls from +7,760 to +3,829 bytes,
  roughly halving what it adds to a bundle. Two causes, both measured
  (`docs/plans/wasm-size.md`), plus a `tools/wasm-size.sh` CI ratchet so it
  cannot regress unnoticed.
- `ra_thread_local!` takes the single-`static` arm on `wasm32-unknown-unknown`
  without the atomics proposal, which `prim/wasm.rs` has assumed single-threaded
  since it was written. A `std::thread_local!` there linked lazy init,
  destructor registration and an "accessed during or after destruction" panic
  that can never run.
- **wasm modules are 8,180 bytes smaller (3,700 gzipped).** The option
  environment pass ran on `wasm32-unknown-unknown`, where `std::env::var` is a
  stub that always fails: 38 iterations formatting 76 strings and allocating 76
  `String`s at startup, to read an environment that cannot exist. It also made
  `options::get` the largest function in a wasm build at 3,708 bytes and dragged
  `str::to_uppercase`, `alloc::fmt::format` and the `OPTION_NAMES` table in with
  it. Measured against the Rust default allocator on a minimal consumer, the
  allocator's gzipped overhead falls from +7,760 to +4,060 bytes.
- `options::error` renders its code into a stack buffer instead of `format!`,
  and `out_fmt` writes bytes instead of `eprint!`. Worth ~0 on wasm (measured),
  but it removes an allocation from an error path and gives `no_std` back the
  message it used to lose.
- **On an ESP32-S3, 2.06-3.73x faster than `esp-alloc`** across four
  allocate/free workloads, measured with a subtracted harness floor and
  checksums proving work parity (`docs/plans/small-metal.md` §2.15).
- The reclamation fixes above cost 2.2-7.9 % of allocation throughput on that
  board. **The host instruction counts in the README predate them** and are
  pending re-measurement; a scheduled `icount` CI job now regenerates them.
- `generic_collect`'s default is geometry-aware: upstream's 10,000 at the
  shipped geometry, 512 under `ra_small_profile`, chosen from a measured sweep
  rather than picked.

## [1.1.6](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-v1.1.5...rusty_alloc-v1.1.6) - 2026-08-28

### Other

- *(wasm)* dissolve the segment tax — slice-aligned segments and a free-slice pool (F2)
- *(heap)* route every span-sized allocation in-segment — the segment tax, F1+F5

## [1.1.5](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-v1.1.4...rusty_alloc-v1.1.5) - 2026-08-28

### Fixed

- *(arena)* keep the public Arena shape, harden the adoption tests, own the census
- *(wasm)* recycle freed segments — adopt-on-free arenas replace the no-op leak
