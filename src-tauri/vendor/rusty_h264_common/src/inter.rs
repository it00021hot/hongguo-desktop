//! Inter prediction primitives: motion compensation and motion-vector
//! prediction. Shared by the encoder (reconstruction) and decoder so the two
//! agree bit-for-bit. Motion vectors are in quarter-pel units; phase 4b uses
//! the integer part only (full-pel), with the 6-tap/bilinear sub-pel filters
//! arriving in 4c.

#[allow(unused_imports)]
use alloc::vec;
use alloc::vec::Vec;

/// Median of three values (`a + b + c − min − max`).
#[inline]
pub fn median3(a: i32, b: i32, c: i32) -> i32 {
    a + b + c - a.min(b).min(c) - a.max(b).max(c)
}

/// A motion-vector predictor neighbor: whether the neighbor block is available
/// (inside the picture/decoded), its motion vector, and its reference index
/// (`-1` for intra/unavailable neighbors, which contribute a zero MV with a
/// non-matching reference).
#[derive(Clone, Copy)]
pub struct MvNeighbor {
    pub available: bool,
    pub mv: (i32, i32),
    pub ref_idx: i32,
}

impl MvNeighbor {
    pub const NONE: MvNeighbor = MvNeighbor {
        available: false,
        mv: (0, 0),
        ref_idx: -1,
    };
}

/// Median motion-vector prediction for a partition (spec §8.4.1.3.1), against
/// the current partition's reference `cur_ref`. `a`/`b`/`c` are the left, above,
/// and above-right neighbors. If exactly one neighbor shares `cur_ref`, its MV is
/// the predictor; otherwise the component-wise median is used.
pub fn predict_mv(a: MvNeighbor, b: MvNeighbor, c: MvNeighbor, cur_ref: i32) -> (i32, i32) {
    // Per-neighbor (mv, refIdx): a usable inter neighbor keeps its mv+ref, else
    // a zero MV with ref −1 (never matches a valid `cur_ref` ≥ 0).
    let resolve = |n: MvNeighbor| -> ((i32, i32), i32) {
        if n.available && n.ref_idx >= 0 {
            (n.mv, n.ref_idx)
        } else {
            ((0, 0), -1)
        }
    };
    let (mva, ra) = resolve(a);
    let (mut mvb, mut rb) = resolve(b);
    let (mut mvc, mut rc) = resolve(c);

    // If both B and C are unavailable but A is available, B and C take A.
    if !b.available && !c.available && a.available {
        mvb = mva;
        rb = ra;
        mvc = mva;
        rc = ra;
    }

    let matches = (ra == cur_ref) as i32 + (rb == cur_ref) as i32 + (rc == cur_ref) as i32;
    if matches == 1 {
        if ra == cur_ref {
            mva
        } else if rb == cur_ref {
            mvb
        } else {
            mvc
        }
    } else {
        (median3(mva.0, mvb.0, mvc.0), median3(mva.1, mvb.1, mvc.1))
    }
}

/// Directional MV prediction for a sub-partition (spec §8.4.1.3.2) against the
/// partition's reference `cur_ref`. `mode` is the inter `mb_type` (0 = 16×16,
/// 1 = 16×8, 2 = 8×16). 16×8/8×16 use a specific neighbor directly when it shares
/// `cur_ref`; otherwise (and always for 16×16) the median.
pub fn predict_partition_mv(
    mode: u8,
    part: usize,
    a: MvNeighbor,
    b: MvNeighbor,
    c: MvNeighbor,
    cur_ref: i32,
) -> (i32, i32) {
    // 16x16 (mode 0) has no directional shortcut and is the dominant shape —
    // leave before building the closure or entering the table.
    if mode == 0 {
        return predict_mv(a, b, c, cur_ref);
    }
    let m = |n: MvNeighbor| n.available && n.ref_idx == cur_ref;
    match (mode, part) {
        (1, 0) if m(b) => b.mv, // 16×8 top → above
        (1, 1) if m(a) => a.mv, // 16×8 bottom → left
        (2, 0) if m(a) => a.mv, // 8×16 left → left
        (2, 1) if m(c) => c.mv, // 8×16 right → above-right
        _ => predict_mv(a, b, c, cur_ref),
    }
}

/// Inter `mb_type` → luma partition regions `(x, y, w, h)` in samples.
pub fn inter_partitions(mode: u8) -> &'static [(usize, usize, usize, usize)] {
    match mode {
        1 => &[(0, 0, 16, 8), (0, 8, 16, 8)], // P_16x8
        2 => &[(0, 0, 8, 16), (8, 0, 8, 16)], // P_8x16
        // P_8x8: four 8×8 sub-macroblocks in raster (decode_p8x8) order, each its
        // own MV. (Sub-8×8 shapes 8×4/4×8/4×4 within an 8×8 are a further split.)
        3 => &[(0, 0, 8, 8), (8, 0, 8, 8), (0, 8, 8, 8), (8, 8, 8, 8)],
        _ => &[(0, 0, 16, 16)], // P_L0_16x16
    }
}

/// Reference sample with edge clamping.
///
/// Bounds-SAFE: a decoder driven by a malformed stream can reach here with `(cw, ch)`
/// that do not match the plane it was handed, and every caller's "interior" test is
/// written against `cw`/`ch` rather than the slice length. In bounds this is identical
/// to a direct index; out of bounds it yields 0 instead of panicking. (Found by the
/// fuzzer the moment CABAC became the default — the CABAC parse produces wilder
/// vectors on mutated input than CAVLC did, so this path had never been reached.)
#[inline]
fn at(reference: &[u8], cw: usize, ch: usize, x: isize, y: isize) -> i32 {
    let xx = x.clamp(0, cw as isize - 1) as usize;
    let yy = y.clamp(0, ch as isize - 1) as usize;
    reference.get(yy * cw + xx).copied().unwrap_or(0) as i32
}

#[inline]
fn clip_u8(v: i32) -> i32 {
    v.clamp(0, 255)
}

/// Per-pixel quarter-pel luma sample (spec §8.4.2.2.1) — the readable reference
/// kept as the bit-exactness oracle for the block-kernel MC below.
#[cfg(test)]
fn luma_sample(
    reference: &[u8],
    cw: usize,
    ch: usize,
    ix: isize,
    iy: isize,
    fx: i32,
    fy: i32,
) -> i32 {
    let g = |dx: isize, dy: isize| at(reference, cw, ch, ix + dx, iy + dy);
    if fx == 0 && fy == 0 {
        return g(0, 0);
    }
    let hor6 = |dy: isize| {
        g(-2, dy) - 5 * g(-1, dy) + 20 * g(0, dy) + 20 * g(1, dy) - 5 * g(2, dy) + g(3, dy)
    };
    let ver6 = |dx: isize| {
        g(dx, -2) - 5 * g(dx, -1) + 20 * g(dx, 0) + 20 * g(dx, 1) - 5 * g(dx, 2) + g(dx, 3)
    };
    let b = || clip_u8((hor6(0) + 16) >> 5);
    let h = || clip_u8((ver6(0) + 16) >> 5);
    let m = || clip_u8((ver6(1) + 16) >> 5);
    let s = || clip_u8((hor6(1) + 16) >> 5);
    let j = || {
        let j1 = hor6(-2) - 5 * hor6(-1) + 20 * hor6(0) + 20 * hor6(1) - 5 * hor6(2) + hor6(3);
        clip_u8((j1 + 512) >> 10)
    };
    match (fx, fy) {
        (1, 0) => (g(0, 0) + b() + 1) >> 1,
        (2, 0) => b(),
        (3, 0) => (g(1, 0) + b() + 1) >> 1,
        (0, 1) => (g(0, 0) + h() + 1) >> 1,
        (1, 1) => (b() + h() + 1) >> 1,
        (2, 1) => (b() + j() + 1) >> 1,
        (3, 1) => (b() + m() + 1) >> 1,
        (0, 2) => h(),
        (1, 2) => (h() + j() + 1) >> 1,
        (2, 2) => j(),
        (3, 2) => (j() + m() + 1) >> 1,
        (0, 3) => (g(0, 1) + h() + 1) >> 1,
        (1, 3) => (h() + s() + 1) >> 1,
        (2, 3) => (j() + s() + 1) >> 1,
        _ => (m() + s() + 1) >> 1,
    }
}

// ---- Block-level luma MC, mirroring openh264 `mc.cpp` (`McHorVerNN_c`) ----
//
// openh264 computes each half-pel plane once per block (not per pixel) and
// averages clipped planes for the quarter positions. Our `luma_sample` recomputed
// the 6-tap for every pixel; these kernels compute each plane once. Bit-identical:
// the 6-tap is separable with exact (un-rounded) integer intermediates, so
// horizontal-then-vertical equals vertical-then-horizontal, and the clamped tile
// reproduces `at()` exactly.

/// Max luma interpolation tile: a 16×16 block plus the 6-tap halo (2 left/up,
/// 3 right/down) → 21×21.
const LUMA_TILE: usize = 21;

/// Per-thread scratch for the sub-pel MC path.
///
/// These three buffers used to be created — and therefore **zero-initialised** —
/// inside every `mc_luma` call, and the tile was additionally returned *by value*.
/// That is ~1.4 KB of `memset` plus a 441-byte move per call, paid identically
/// whether the block is 16×16 (256 useful samples) or 4×4 (16). Profiling put
/// `inter-mc` at 166 s of a 266 s quality-preset encode over 991 M calls, and a
/// block-size sweep showed a 4×4 sub-pel call costing *more* than a 16×16 one
/// (248 vs 172 ns) — i.e. the cost was the per-call buffers, not the filter.
///
/// Hoisting them to thread-local scratch makes that setup once per thread instead
/// of once per candidate. Byte-identical: both `luma_tile_into` paths write the
/// whole `(bw+5)×(bh+5)` region before it is read, and `a`/`b` are written over
/// `bw*bh` samples by `luma_h`/`luma_v`/`luma_centre` before `pixel_avg`/`avg_full`
/// read the same range — so the zero-fill was dead in every case.
pub struct McScratch {
    tile: [u8; LUMA_TILE * LUMA_TILE],
    a: [u8; 256],
    b: [u8; 256],
}

/// Runs `f` with the thread's MC scratch borrowed ONCE. Callers that issue many
/// MC calls in a row (the bi-pred pair, the P-path rect ladder) hoist the
/// TLS lookup + RefCell borrow out of the per-call path by taking this at
/// region/MB scope and calling `mc_luma_padded_pre` inside — that fixed
/// per-call cost is paid twice per bi-pred region, ~1M sub-pel calls per clip.
#[inline]
pub fn with_mc_scratch<R>(f: impl FnOnce(&mut McScratch) -> R) -> R {
    MC_SCRATCH.with(|s| f(&mut s.borrow_mut()))
}

#[cfg(feature = "std")]
thread_local! {
    static MC_SCRATCH: core::cell::RefCell<McScratch> = const {
        core::cell::RefCell::new(McScratch {
            tile: [0; LUMA_TILE * LUMA_TILE],
            a: [0; 256],
            b: [0; 256],
        })
    };
}

/// Without `std` the MC scratch (under 1 KB) lives on the stack of each
/// call instead of in a thread-local; the `with` shape is kept so the call
/// sites do not change.
#[cfg(not(feature = "std"))]
struct McScratchCell;
#[cfg(not(feature = "std"))]
impl McScratchCell {
    fn with<R>(&self, f: impl FnOnce(&core::cell::RefCell<McScratch>) -> R) -> R {
        let cell = core::cell::RefCell::new(McScratch {
            tile: [0; LUMA_TILE * LUMA_TILE],
            a: [0; 256],
            b: [0; 256],
        });
        f(&cell)
    }
}
#[cfg(not(feature = "std"))]
static MC_SCRATCH: McScratchCell = McScratchCell;

/// Dev-only census of `mc_luma` calls by block size × sub-pel phase.
///
/// Exists to size the half-pel-plane-cache lever: with a cached plane set, a
/// half-pel call becomes a strided COPY and a quarter-pel call a 2-tap AVERAGE,
/// so the prize is (how many calls are sub-pel) × (filter cost − read cost). The
/// mix is deterministic, so this is valid on a loaded machine.
#[cfg(feature = "profile")]
pub mod mcstats {
    use crate::atomic::AtomicU64;
    use core::sync::atomic::Ordering;

    /// Descent E depth-6: WHO calls `mc_luma`? `prof inter-mc` counts ~17 calls per
    /// macroblock while reconstruction needs only 1-4, so the stage is dominated by
    /// something other than recon and any prize computed against "recon MC" is priced
    /// on the wrong population. Callers tag themselves; 0 = untagged.
    pub const SITES: [&str; 5] = [
        "untagged",
        "recon",
        "search-fallback",
        "skip-check",
        "bdirect",
    ];
    pub static SITE_COUNTS: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
    pub static SITE_CYCLES: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
    thread_local! {
        pub static SITE: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
    }
    /// Scoped tag: set on construction, restored on drop (call sites nest).
    pub struct SiteTag(usize);
    impl SiteTag {
        pub fn new(s: usize) -> Self {
            let prev = SITE.with(|c| c.replace(s));
            SiteTag(prev)
        }
    }
    impl Drop for SiteTag {
        fn drop(&mut self) {
            SITE.with(|c| c.set(self.0));
        }
    }
    #[inline]
    pub(super) fn site_add(c: u64) {
        let s = SITE.with(|c| c.get());
        SITE_COUNTS[s].fetch_add(1, Ordering::Relaxed);
        SITE_CYCLES[s].fetch_add(c, Ordering::Relaxed);
    }
    pub fn site_snapshot() -> Vec<(&'static str, u64, u64)> {
        (0..5)
            .map(|i| {
                (
                    SITES[i],
                    SITE_COUNTS[i].load(Ordering::Relaxed),
                    SITE_CYCLES[i].load(Ordering::Relaxed),
                )
            })
            .collect()
    }

    /// Phase classes: 0 = full-pel (0,0), 1 = half H/V, 2 = half centre (2,2),
    /// 3 = quarter (one component odd).
    pub const PHASES: [&str; 4] = ["fullpel", "half-HV", "half-ctr", "quarter"];
    /// Size classes: 16×16, 16×8/8×16, 8×8, 8×4/4×8, 4×4, other.
    pub const SIZES: [&str; 6] = ["16x16", "16x8/8x16", "8x8", "8x4/4x8", "4x4", "other"];

    pub static COUNTS: [AtomicU64; 24] = [const { AtomicU64::new(0) }; 24];
    /// Descent E depth-6: CYCLES per bucket. A call-count census is the WRONG
    /// denominator for a prune — a full-pel 16x16 is a row copy, a quarter-pel is a
    /// per-pixel 6-tap, and they differ ~10x. Weight by time, not by calls.
    pub static CYCLES: [AtomicU64; 24] = [const { AtomicU64::new(0) }; 24];

    #[inline]
    pub(super) fn bucket(bw: usize, bh: usize, fx: i32, fy: i32) -> usize {
        let size = match (bw, bh) {
            (16, 16) => 0,
            (16, 8) | (8, 16) => 1,
            (8, 8) => 2,
            (8, 4) | (4, 8) => 3,
            (4, 4) => 4,
            _ => 5,
        };
        let phase = match (fx, fy) {
            (0, 0) => 0,
            (2, 2) => 2,
            (fx, fy) if fx % 2 == 0 && fy % 2 == 0 => 1,
            _ => 3,
        };
        size * 4 + phase
    }

    #[inline]
    pub(super) fn add_cycles(b: usize, c: u64) {
        CYCLES[b].fetch_add(c, Ordering::Relaxed);
    }

    #[inline]
    pub(super) fn record(bw: usize, bh: usize, fx: i32, fy: i32) {
        let size = match (bw, bh) {
            (16, 16) => 0,
            (16, 8) | (8, 16) => 1,
            (8, 8) => 2,
            (8, 4) | (4, 8) => 3,
            (4, 4) => 4,
            _ => 5,
        };
        let phase = match (fx, fy) {
            (0, 0) => 0,
            (2, 2) => 2,
            (fx, fy) if fx % 2 == 0 && fy % 2 == 0 => 1,
            _ => 3,
        };
        COUNTS[size * 4 + phase].fetch_add(1, Ordering::Relaxed);
    }

    pub fn reset() {
        for c in COUNTS.iter() {
            c.store(0, Ordering::Relaxed);
        }
        for c in CYCLES.iter() {
            c.store(0, Ordering::Relaxed);
        }
        for c in SITE_COUNTS.iter() {
            c.store(0, Ordering::Relaxed);
        }
        for c in SITE_CYCLES.iter() {
            c.store(0, Ordering::Relaxed);
        }
    }

    /// `(size, phase, count, cycles)` for every non-empty bucket.
    pub fn snapshot_cycles() -> Vec<(&'static str, &'static str, u64, u64)> {
        let mut v = Vec::new();
        for i in 0..24 {
            let n = COUNTS[i].load(Ordering::Relaxed);
            if n != 0 {
                v.push((
                    SIZES[i / 4],
                    PHASES[i % 4],
                    n,
                    CYCLES[i].load(Ordering::Relaxed),
                ));
            }
        }
        v
    }

    /// `(size_label, phase_label, count)` for every non-empty bucket.
    pub fn snapshot() -> Vec<(&'static str, &'static str, u64)> {
        let mut v = Vec::new();
        for (i, c) in COUNTS.iter().enumerate() {
            let n = c.load(Ordering::Relaxed);
            if n > 0 {
                v.push((SIZES[i / 4], PHASES[i % 4], n));
            }
        }
        v
    }
}

/// Extracts the `(bw+5)×(bh+5)` reference neighbourhood around the full-pel origin
/// `(ix0,iy0)` into `t`, clamping at the frame border — the edge-extended input
/// openh264's kernels assume. The block's top-left sample lands at tile `(2,2)`.
/// Returns the tile stride. Every sample in `[0, (bh+5)*ts)` is written.
fn luma_tile_into(
    t: &mut [u8],
    reference: &[u8],
    cw: usize,
    ch: usize,
    ix0: isize,
    iy0: isize,
    bw: usize,
    bh: usize,
) -> usize {
    let ts = bw + 5;
    // Interior fast path: the whole `(bw+5)×(bh+5)` halo is inside the frame, so no
    // edge clamp is needed. Extract by contiguous row copies (a vectorized memcpy)
    // — the unconditional per-pixel `clamp` on the slow path defeats
    // autovectorization even when (as here) it would always be a no-op.
    if ix0 - 2 >= 0
        && iy0 - 2 >= 0
        && ix0 - 2 + ts as isize <= cw as isize
        && iy0 - 2 + (bh + 5) as isize <= ch as isize
        && reference.len() >= cw * ch
    {
        let (rx0, ry0) = ((ix0 - 2) as usize, (iy0 - 2) as usize);
        for ty in 0..bh + 5 {
            let src = (ry0 + ty) * cw + rx0;
            t[ty * ts..ty * ts + ts].copy_from_slice(&reference[src..src + ts]);
        }
        return ts;
    }
    for ty in 0..bh + 5 {
        let ry = (iy0 - 2 + ty as isize).clamp(0, ch as isize - 1) as usize * cw;
        let trow = &mut t[ty * ts..][..ts];
        for (tx, o) in trow.iter_mut().enumerate() {
            let rx = (ix0 - 2 + tx as isize).clamp(0, cw as isize - 1) as usize;
            *o = reference.get(ry + rx).copied().unwrap_or(0);
        }
    }
    ts
}

/// Horizontal half-pel plane (`McHorVer20`): `clip((6tapₕ + 16) >> 5)`, block
/// shifted by `(dr, dc)` tile rows/cols.
fn luma_h(t: &[u8], ts: usize, bw: usize, bh: usize, dr: usize, dc: usize, dst: &mut [u8]) {
    #[cfg(accel)]
    if bw == 16 || bw == 8 {
        rusty_h264_accel::mc_hor20(t, (2 + dr) * ts + 2 + dc, ts, dst, bw, bh);
        return;
    }
    #[cfg(accel)]
    if bw == 4 && bh <= 16 {
        let off = (2 + dr) * ts + 2 + dc;
        if off >= 2 && t.len() >= off + (bh - 1) * ts + 8 + 3 {
            w4_via_w8(dst, bh, |s| {
                rusty_h264_accel::mc_hor20(t, off, ts, s, 8, bh)
            });
            return;
        }
    }
    // TWO SLICES PER ROW, and the geometry is why this works here and NOT in
    // `luma_centre`. These six taps are HORIZONTAL — `t[p-2]` through `t[p+3]`
    // lie in one contiguous run — so a single `bw + 5` slice covers every read
    // the whole row makes. `luma_centre`'s taps stride by `ts`, six separate
    // runs, which is what made the identical-looking edit cost +9.3% there.
    for r in 0..bh {
        let base = (2 + r + dr) * ts + 2 + dc;
        let src = &t[base - 2..][..bw + 5];
        let drow = &mut dst[r * bw..][..bw];
        // `windows(6)` yields a slice of EXACTLY six, so all six taps fold —
        // `src[c] ..= src[c + 5]` still needed `c < bw` related to `bw + 5`.
        for (o, w) in drow.iter_mut().zip(src.windows(6)) {
            let f = w[0] as i32 - 5 * w[1] as i32 + 20 * w[2] as i32 + 20 * w[3] as i32
                - 5 * w[4] as i32
                + w[5] as i32;
            *o = clip_u8((f + 16) >> 5) as u8;
        }
    }
}

/// 4-WIDE LUMA BLOCKS THROUGH THE 8-WIDE KERNELS (SIMD census 2026-09-05,
/// findings #3-5). The accel luma kernels serve `w` in {8, 16}; sub-8x8
/// partitions (4x4 / 4x8) fell to the scalar loops below -- `luma_v` paying
/// SIX `.get().unwrap_or(0)` checked loads per sample. Compose instead: run the
/// 8-wide kernel into a scratch (the four discarded columns are real samples of
/// the padded plane, kept in bounds by the caller's length check, which
/// mirrors the kernel's own assert) and copy the left four of each row.
/// Byte-identical: every kept sample is the same taps at the same position.
/// The quarter-pel dispatchers' compose fallbacks (`luma_h` + `avg_full`,
/// `luma_h` + `luma_v` + `pixel_avg`, ...) then run on kernels at w == 4 too.
#[cfg(accel)]
#[inline]
fn w4_via_w8(dst: &mut [u8], bh: usize, run: impl FnOnce(&mut [u8])) {
    let mut s = [0u8; 8 * 16];
    run(&mut s[..8 * bh]);
    for (d, srow) in dst.chunks_exact_mut(4).zip(s.chunks_exact(8)).take(bh) {
        d.copy_from_slice(&srow[..4]);
    }
}

/// Vertical half-pel plane (`McHorVer02`): `clip((6tapᵥ + 16) >> 5)`.
fn luma_v(t: &[u8], ts: usize, bw: usize, bh: usize, dr: usize, dc: usize, dst: &mut [u8]) {
    #[cfg(accel)]
    if bw == 16 || bw == 8 {
        rusty_h264_accel::mc_ver02(t, (2 + dr) * ts + 2 + dc, ts, dst, bw, bh);
        return;
    }
    #[cfg(accel)]
    if bw == 4 && bh <= 16 {
        let off = (2 + dr) * ts + 2 + dc;
        if off >= 2 * ts && t.len() >= off + (bh + 2) * ts + 8 {
            w4_via_w8(dst, bh, |s| {
                rusty_h264_accel::mc_ver02(t, off, ts, s, 8, bh)
            });
            return;
        }
    }
    // DESTINATION row sliced only. The taps here are vertical — six strided
    // runs — and slicing THOSE is the refuted shape (see `luma_centre`); the
    // write is contiguous and costs one slice per row instead of `bw` checks.
    for r in 0..bh {
        let base = (2 + r + dr) * ts + 2 + dc;
        let drow = &mut dst[r * bw..][..bw];
        for c in 0..bw {
            let p = base + c;
            let g = |q: usize| t.get(q).copied().unwrap_or(0) as i32;
            let f = g(p - 2 * ts) - 5 * g(p - ts) + 20 * g(p) + 20 * g(p + ts) - 5 * g(p + 2 * ts)
                + g(p + 3 * ts);
            drow[c] = clip_u8((f + 16) >> 5) as u8;
        }
    }
}

/// Centre half-pel plane (`McHorVer22`): vertical 6-tap to 16-bit intermediates,
/// then horizontal 6-tap — `clip((·+ 512) >> 10)`.
fn luma_centre(t: &[u8], ts: usize, bw: usize, bh: usize, dst: &mut [u8]) {
    #[cfg(accel)]
    if bw == 16 || bw == 8 {
        rusty_h264_accel::mc_centre(t, ts, dst, bw, bh);
        return;
    }
    #[cfg(accel)]
    if bw == 4 && bh <= 16 && t.len() >= (bh + 4) * ts + 8 + 5 {
        w4_via_w8(dst, bh, |s| rusty_h264_accel::mc_centre(t, ts, s, 8, bh));
        return;
    }
    // REFUTED, do not retry: row-slicing the six vertical taps (and `dst`)
    // removed ONE panic path of eight and cost +9.3% instructions (323 -> 353).
    // `bw`/`bh` are runtime values, so each slice length is a runtime bound the
    // compiler cannot fold — six of them per row outweigh the per-sample checks
    // they replace. Same shape as `weight_partition`'s inner rows (+28%).
    let mut itmp = [0i32; LUMA_TILE];
    for r in 0..bh {
        let base = (2 + r) * ts;
        for (j, slot) in itmp[..bw + 5].iter_mut().enumerate() {
            let p = base + j;
            let g = |q: usize| t.get(q).copied().unwrap_or(0) as i32;
            *slot = g(p - 2 * ts) - 5 * g(p - ts) + 20 * g(p) + 20 * g(p + ts) - 5 * g(p + 2 * ts)
                + g(p + 3 * ts);
        }
        // The earlier refutation bundled SEVEN changes; this is the one of them
        // that stands alone. The vertical-tap slices were the cost — the
        // destination row is contiguous and independent of them.
        let drow = &mut dst[r * bw..][..bw];
        for (o, w) in drow.iter_mut().zip(itmp[..bw + 5].windows(6)) {
            let f = w[0] - 5 * w[1] + 20 * w[2] + 20 * w[3] - 5 * w[4] + w[5];
            *o = clip_u8((f + 512) >> 10) as u8;
        }
    }
}

/// Bi-average two STRIDED luma windows into a contiguous `w * h` block.
///
/// The B full-pel skip path had this as a hand-rolled scalar byte loop -- 256
/// `(p + q + 1) >> 1` per macroblock -- three modules from the `pixel_avg`
/// kernel that computes exactly that with `pavgb`. The kernel already takes
/// both source strides, which is what the two reference windows need; only a
/// public door was missing.
pub fn avg_row_into(a: &[u8], b: &[u8], w: usize, out: &mut [u8]) {
    #[cfg(accel)]
    if out.len() >= w && a.len() >= w && b.len() >= w {
        // The kernel is defined at widths 16/8/4, so walk the row in those
        // pieces. Every caller here is a whole number of macroblocks wide, so
        // the 16-wide loop does all the work and the 8/4 tails never run on
        // luma; chroma lands on the 8-wide step.
        let mut c = 0;
        while c + 16 <= w {
            rusty_h264_accel::pixel_avg(&mut out[c..c + 16], &a[c..], 16, &b[c..], 16, 16, 1);
            c += 16;
        }
        while c + 8 <= w {
            rusty_h264_accel::pixel_avg(&mut out[c..c + 8], &a[c..], 8, &b[c..], 8, 8, 1);
            c += 8;
        }
        while c + 4 <= w {
            rusty_h264_accel::pixel_avg(&mut out[c..c + 4], &a[c..], 4, &b[c..], 4, 4, 1);
            c += 4;
        }
        if c == w {
            return;
        }
    }
    // Scalar oracle, and the path on a non-accel build or a width the kernel
    // cannot cover exactly.
    for (d, (&x, &y)) in out[..w].iter_mut().zip(a[..w].iter().zip(&b[..w])) {
        *d = ((x as u16 + y as u16 + 1) >> 1) as u8;
    }
}

/// `PixelAvg_c`: `(a + b + 1) >> 1` of two clipped planes.
fn pixel_avg(a: &[u8], b: &[u8], bw: usize, bh: usize, dst: &mut [u8]) {
    let n = bw * bh;
    // Same leak `avg_full` fixed, for the TWO-FILTER quarter positions
    // ((1,1)/(3,1)/(1,3)/(3,3) and the centre-adjacent four): the average on
    // top of the asm half-pel kernels was a scalar runtime-width loop. The MC
    // census prices quarter-pel at ~68% of decoder MC cycles, and the eight
    // positions served here all end in this loop. `pavgb` computes
    // `(a + b + 1) >> 1` exactly — byte-identical. The scalar loop stays as
    // the non-accel path and the oracle.
    //
    // The TRUE block geometry is passed, never reconstructed from `n`: the
    // openh264 row loops are unrolled by more than one row, so a synthetic
    // (16, n/16) shape with h < 4 over-runs the block — that was a real
    // SEGFAULT on every sub-8x8 stream (first cut of this dispatch, caught by
    // the corpus gate). The kernels are exercised at every real (bw, bh ≥ 4)
    // shape by `avg_full` already.
    #[cfg(accel)]
    if (bw == 16 || bw == 8 || bw == 4) && a.len() >= n && b.len() >= n && dst.len() >= n {
        rusty_h264_accel::pixel_avg(&mut dst[..n], &a[..n], bw, &b[..n], bw, bw, bh);
        return;
    }
    // ZIP, not three indexed reads per pixel. `dst[i]`, `a[i]` and `b[i]` are
    // three INDEPENDENT slices, so every pixel carried three bounds checks; the
    // zip proves all three once for the whole block. (Contrast `luma_centre`,
    // where the same idea lost: there it would be six slices PER ROW against a
    // runtime width — here it is three for the entire block.)
    for (d, (&x, &y)) in dst[..n].iter_mut().zip(a[..n].iter().zip(&b[..n])) {
        *d = ((x as i32 + y as i32 + 1) >> 1) as u8;
    }
}

/// `PixelAvg_c` of a half-pel plane with a full-pel block shifted by `(dr, dc)`.
fn avg_full(
    t: &[u8],
    ts: usize,
    bw: usize,
    bh: usize,
    dr: usize,
    dc: usize,
    half: &[u8],
    dst: &mut [u8],
) {
    // The QUARTER-PEL step. `luma_h`/`luma_v`/`luma_centre` have been on asm for a
    // long time; this average layered on top of them stayed a scalar per-pixel loop
    // with a runtime width. On real x264 streams ~85% of decoder MC cycles are
    // quarter-pel, so this loop was handing the kernel's win back on the large
    // majority of MC. `pavgb` computes `(a + b + 1) >> 1` exactly — byte-identical.
    #[cfg(accel)]
    if bw == 16 || bw == 8 || bw == 4 {
        let base = (2 + dr) * ts + 2 + dc;
        rusty_h264_accel::pixel_avg(dst, &t[base..], ts, half, bw, bw, bh);
        return;
    }
    // Row-at-a-time zip: the destination walks in `bw` chunks, and the two
    // sources become one slice each per row instead of one check per sample.
    for (r, drow) in dst[..bw * bh].chunks_mut(bw).enumerate() {
        let base = (2 + r + dr) * ts + 2 + dc;
        let trow = &t[base..][..bw];
        let hrow = &half[r * bw..][..bw];
        for ((d, &x), &y) in drow.iter_mut().zip(trow).zip(hrow) {
            *d = ((x as i32 + y as i32 + 1) >> 1) as u8;
        }
    }
}

/// A/B: restore half→scratch→avg compose for one-filter qpel (`RS_H264_QPEL_COMPOSE=1`).
#[inline]
#[cfg_attr(not(accel), allow(dead_code))] // callers are accel-gated
fn qpel_compose() -> bool {
    // ROUTED AT BUILD TIME (routing round 2026-09-05): the shipped arm is the
    // constant below; the env arm exists only under `--features knobs`.
    #[cfg(not(feature = "knobs"))]
    {
        return false;
    }
    #[cfg(feature = "knobs")]
    {
        use crate::once::OnceLock;
        static V: OnceLock<bool> = OnceLock::new();
        *V.get_or_init(|| crate::knob("RS_H264_QPEL_COMPOSE").is_some_and(|v| v == "1"))
    }
}

/// One-filter qpel (`(1,0)/(3,0)/(0,1)/(0,3)`): fused half+avg when accel + w∈{8,16}.
/// `horiz`: horizontal 6-tap; else vertical. `fshift`: full-pel offset (0 or 1) in the
/// filter axis (`fdc` for horiz, `fdr` for vert) — matches `avg_full`'s (0,1)/(1,0).
fn qpel_one_filter(
    t: &[u8],
    ts: usize,
    bw: usize,
    bh: usize,
    horiz: bool,
    fshift: usize,
    out: &mut [u8],
) {
    #[cfg(accel)]
    if !qpel_compose() && (bw == 16 || bw == 8) {
        let off = 2 * ts + 2;
        if horiz {
            rusty_h264_accel::mc_hor_qpel(t, off, ts, out, bw, bh, fshift);
        } else {
            rusty_h264_accel::mc_ver_qpel(t, off, ts, out, bw, bh, fshift);
        }
        return;
    }
    // Compose oracle / narrow widths / non-accel: half into a stack scratch, then avg.
    let mut half = [0u8; 256];
    let n = bw * bh;
    debug_assert!(n <= 256);
    if horiz {
        luma_h(t, ts, bw, bh, 0, 0, &mut half[..n]);
        avg_full(t, ts, bw, bh, 0, fshift, &half[..n], out);
    } else {
        luma_v(t, ts, bw, bh, 0, 0, &mut half[..n]);
        avg_full(t, ts, bw, bh, fshift, 0, &half[..n], out);
    }
}

/// Two-filter HV qpel: horizontal half into `a`, then vertical half+avg into `out`
/// (kills the second scratch store). Compose oracle when `qpel_compose()`.
fn qpel_hv(
    t: &[u8],
    ts: usize,
    bw: usize,
    bh: usize,
    hdr: usize,
    hdc: usize,
    vdr: usize,
    vdc: usize,
    a: &mut [u8],
    out: &mut [u8],
) {
    #[cfg(accel)]
    if !qpel_compose() && (bw == 16 || bw == 8) {
        // FULL fusion: both halves + avg in one kernel loop — no `a` staging,
        // no second kernel call (the previous two-step form, a ver02+avg kernel
        // reading back a staged half plane, was ripped once this landed).
        rusty_h264_accel::mc_hv_qpel(t, ts, out, bw, bh, hdr, hdc, vdr, vdc);
        return;
    }
    luma_h(t, ts, bw, bh, hdr, hdc, a);
    let n = bw * bh;
    let mut b = [0u8; 256];
    debug_assert!(n <= 256);
    luma_v(t, ts, bw, bh, vdr, vdc, &mut b[..n]);
    pixel_avg(a, &b[..n], bw, bh, out);
}

/// Centre-adjacent qpel ((2,1)/(2,3)/(1,2)/(3,2)): fused centre + half + avg.
/// The half-pel operand is DERIVED from the centre's own full-precision pass-1
/// buffer (its rounded form), so the 3-kernel 2-staging compose collapses to
/// one pass-1 + one fused pass-2/avg. `RS_H264_QPEL_COMPOSE=1` restores the
/// compose path as the A/B oracle, same knob as the other fused positions.
#[allow(clippy::too_many_arguments)]
fn qpel_centre_adj(
    t: &[u8],
    ts: usize,
    bw: usize,
    bh: usize,
    horiz: bool,
    fshift: usize,
    a: &mut [u8],
    b: &mut [u8],
    out: &mut [u8],
) {
    #[cfg(accel)]
    if !qpel_compose() && (bw == 16 || bw == 8) {
        if horiz {
            rusty_h264_accel::mc_centre_hq(t, ts, out, bw, bh, fshift);
        } else {
            rusty_h264_accel::mc_centre_vq(t, ts, out, bw, bh, fshift);
        }
        return;
    }
    // Compose oracle / narrow widths / non-accel.
    if horiz {
        luma_h(t, ts, bw, bh, fshift, 0, a);
    } else {
        luma_v(t, ts, bw, bh, 0, fshift, a);
    }
    luma_centre(t, ts, bw, bh, b);
    pixel_avg(a, b, bw, bh, out);
}

/// The `McLuma_c` `[mvx&3][mvy&3]` dispatch over the clamped tile (sub-pel only;
/// `(0,0)` is handled by the full-pel copy path in [`mc_luma`]).
#[allow(clippy::too_many_arguments)]
fn mc_luma_subpel(
    t: &[u8],
    ts: usize,
    bw: usize,
    bh: usize,
    fx: i32,
    fy: i32,
    a: &mut [u8],
    b: &mut [u8],
    out: &mut [u8],
) {
    // `a`/`b` are caller-owned scratch (see `McScratch`): every arm below fully
    // writes the `n` samples it later reads, so their prior contents are dead.
    // One-filter qpel no longer needs them when fused (see `qpel_one_filter`).
    match (fx, fy) {
        (2, 0) => luma_h(t, ts, bw, bh, 0, 0, out),
        (0, 2) => luma_v(t, ts, bw, bh, 0, 0, out),
        (2, 2) => luma_centre(t, ts, bw, bh, out),
        // One-filter qpel: fuse half-pel + pavgb (openh264 McHorVer10/30/01/03).
        // Compose path (`RS_H264_QPEL_COMPOSE=1`) keeps the scratch store as A/B oracle.
        (1, 0) => qpel_one_filter(t, ts, bw, bh, true, 0, out),
        (3, 0) => qpel_one_filter(t, ts, bw, bh, true, 1, out),
        (0, 1) => qpel_one_filter(t, ts, bw, bh, false, 0, out),
        (0, 3) => qpel_one_filter(t, ts, bw, bh, false, 1, out),
        (1, 1) => qpel_hv(t, ts, bw, bh, 0, 0, 0, 0, a, out),
        (3, 1) => qpel_hv(t, ts, bw, bh, 0, 0, 0, 1, a, out),
        (1, 3) => qpel_hv(t, ts, bw, bh, 1, 0, 0, 0, a, out),
        (3, 3) => qpel_hv(t, ts, bw, bh, 1, 0, 0, 1, a, out),
        (2, 1) => qpel_centre_adj(t, ts, bw, bh, true, 0, a, b, out),
        (2, 3) => qpel_centre_adj(t, ts, bw, bh, true, 1, a, b, out),
        (1, 2) => qpel_centre_adj(t, ts, bw, bh, false, 0, a, b, out),
        (3, 2) => qpel_centre_adj(t, ts, bw, bh, false, 1, a, b, out),
        _ => unreachable!("(0,0) is the full-pel path"),
    }
}

/// The three half-pel luma planes of one reference picture, at full frame size.
///
/// x264 filters these ONCE per reference frame and then every sub-pel motion-search
/// candidate is a strided copy from one plane (half positions) or a 2-tap average of
/// two (quarter positions). We previously re-ran the 6-tap per candidate: measured at
/// 166 s of a 266 s quality-preset encode over 991 M calls, against x264's 188 ms of
/// `hpel-filter` for the entire corpus.
///
/// Naming follows the spec's sample grid at full-pel `G`:
///   `h` = `b` (half-pel horizontal) · `v` = `h` (half-pel vertical) · `c` = `j` (centre)
#[derive(Clone, Debug)]
pub struct HpelPlanes {
    /// Edge-replicated FULL-pel plane. Needed as an operand for the quarter
    /// positions that average against `G`, and padded so out-of-frame candidates
    /// resolve without a bounds decline.
    pub f: Vec<u8>,
    pub h: Vec<u8>,
    pub v: Vec<u8>,
    pub c: Vec<u8>,
    /// Row stride of every plane (`cw + 2*HPEL_PAD`).
    pub stride: usize,
    pub pad: usize,
    pub pw: usize,
    pub ph: usize,
    pub cw: usize,
    pub ch: usize,
}

/// Border added to every cached plane, in samples.
///
/// Sized so a ±24 motion search around any macroblock still lands inside: a 16-wide
/// block at the right edge reaches `cw + 24`, and reading its `+1` neighbour needs
/// `cw + 41` ≤ `cw + PAD`. Without a border the plane cache DECLINED every candidate
/// whose vector left the picture — 62.5% of the remaining `mc_luma` fallbacks, at
/// ~195 ns against 33 ns for a plane read.
///
/// SWEPT (foreman / bus / park_joy, `RFF_HPEL_PAD`), because a bigger border is a
/// TRADE — more candidates covered, but a larger working set (4 planes over
/// `(cw+2P)(ch+2P)`) and a costlier per-frame build:
///
/// | pad | foreman fallbacks | bus | park_joy | build (foreman) |
/// |---|---|---|---|---|
/// | 0  | 207,537 | 838,581 | 6,354,258 | 5.7 ms |
/// | 8  | 127,407 | 774,592 | 6,172,777 | 4.8 ms |
/// | **16** | **127,274** | **765,625** | **6,122,122** | **6.1 ms** |
/// | 32 | 127,262 | 764,999 | 6,093,358 | 9.0 ms |
///
/// Padding replicated around each cached half-pel plane.
///
/// 32, raised from 16 after the edge full-pel path landed. The earlier sweep found the
/// pad made NO difference and was correctly recorded as refuted -- but that measurement
/// ran against a population dominated by FULL-PEL declines, which `hpel_ref` now serves.
/// The refutation expired when its baseline moved. Re-swept, pad 32 removes essentially
/// every remaining search fallback (football 17,662 -> 175, foreman 3,238 -> 0).
///
/// Priced at the PIPELINE level, not by component arithmetic -- the component estimate
/// was wrong in both directions (it mixed an rdtsc cycle census with profiler
/// milliseconds via an assumed clock, and read a 23-sample build cost off single runs).
/// Paired ABBA, median of paired ratios: bus 1.113x (14/14), blue_sky 1.054x (6/6),
/// park_joy 1.050x (11/12), football 1.026x (11/14), foreman 1.015x (ns),
/// crowd_run 1.005x (ns). No clip regresses; the gain tracks edge-overhang density, so
/// fast pans benefit most and large frames least.
///
/// Byte-identical at 16/32/64 -- a wider pad grows the planes but never changes a value
/// read. `RFF_HPEL_PAD` overrides.
pub const HPEL_PAD_DEFAULT: usize = 32;
pub fn hpel_pad() -> usize {
    crate::cached_knob!(usize, HPEL_PAD_DEFAULT, {
        crate::knob("RFF_HPEL_PAD")
            .and_then(|s| s.parse().ok())
            .unwrap_or(HPEL_PAD_DEFAULT)
            .clamp(0, 128)
    })
}

/// Builds the three half-pel planes for `reference` (`cw`×`ch`, MB-aligned).
///
/// Bit-exact with [`mc_luma`] by CONSTRUCTION: it walks the frame in the same 16×16
/// blocks, extracts the same clamped halo via [`luma_tile_into`], and runs the same
/// `luma_h`/`luma_v`/`luma_centre` kernels. Each output sample depends only on the
/// six clamped input samples around it, so the value is independent of which block
/// computed it.
/// Edge-replicates a `w`×`h` plane into a `(w+2·pad)`×`(h+2·pad)` padded copy —
/// openh264's `ExpandPicture`. `mc_luma` clamps each tap independently via `at()`,
/// and clamping IS edge replication, so filtering the padded plane is bit-identical
/// to the clamped-read original (the argument `build_hpel_planes` rests on; the
/// decoder's per-reference padding reuses it so per-MC-call tile extraction dies).
pub fn pad_plane(src: &[u8], w: usize, h: usize, pad: usize) -> Vec<u8> {
    pad_plane_into(Vec::new(), src, w, h, pad)
}

/// `pad_plane` reusing a recycled buffer. Every byte of the padded plane is
/// written by the row loop below (left fill + interior copy + right fill, all
/// `ph` rows), so a RIGHT-SIZED recycled buffer needs no clearing at all — and,
/// unlike a fresh `vec![0; n]` (whose zero pages are free but whose first
/// touches page-fault), its pages are already mapped and warm. This is what the
/// earlier pad_plane memset-elimination attempt (refuted, WHYS ledger) was
/// missing: on a FRESH alloc the memset is free; the win only exists when the
/// allocation itself is recycled.
pub fn pad_plane_into(mut f: Vec<u8>, src: &[u8], w: usize, h: usize, pad: usize) -> Vec<u8> {
    let (pw, ph) = (w + 2 * pad, h + 2 * pad);
    if f.len() != pw * ph {
        f.clear();
        f.resize(pw * ph, 0);
    }
    for y in 0..ph {
        let sy = (y as isize - pad as isize).clamp(0, h as isize - 1) as usize;
        let row = &src[sy * w..sy * w + w];
        let d = &mut f[y * pw..y * pw + pw];
        let (Some(&l), Some(&r)) = (row.first(), row.last()) else {
            continue;
        };
        d[..pad].fill(l);
        d[pad..pad + w].copy_from_slice(row);
        d[pad + w..].fill(r);
    }
    f
}

pub fn build_hpel_planes(reference: &[u8], cw: usize, ch: usize) -> HpelPlanes {
    let _g = crate::prof::scope(crate::prof::Stage::MeHpelBuild);
    let pad = hpel_pad();
    let (pw, ph) = (cw + 2 * pad, ch + 2 * pad);
    // 1. Edge-replicated source (see `pad_plane` for why this is bit-identical).
    let f = pad_plane(reference, cw, ch, pad);
    // 2. Filter the three half-pel planes over the padded area.
    let mut h = vec![0u8; pw * ph];
    let mut v = vec![0u8; pw * ph];
    let mut c = vec![0u8; pw * ph];
    // Dispatch ladder: the AVX2 FUSED single pass (side-by-side descent: the tile
    // walk measured 8× x264's hpel-filter, whose fused shape this kernel mirrors)
    // → env-forced scalar fused (RFF_HPEL_FUSED=1, the oracle base) → tile walk.
    // All three are byte-identical (`fused_hpel_builder_matches_tiles`,
    // `avx2_fused_matches_scalar_fused`); the choice is speed only.
    #[cfg(accel)]
    let done = !hpel_fused_forced_off()
        && rusty_h264_accel::hpel_fused(&f, pw, ph, &mut h, &mut v, &mut c);
    #[cfg(not(accel))]
    let done = false;
    if !done {
        if hpel_fused_enabled() {
            build_hpel_fused(&f, pw, ph, &mut h, &mut v, &mut c);
        } else {
            build_hpel_tiles(&f, pw, ph, &mut h, &mut v, &mut c);
        }
    }
    HpelPlanes {
        f,
        h,
        v,
        c,
        stride: pw,
        pad,
        pw,
        ph,
        cw,
        ch,
    }
}

/// Campaign-3 knob. DEFAULT OFF (the tile walk): the scalar fused pass measured
/// SLOWER than the tile walk (950 vs 617 µs/frame CIF) because the tiles run the
/// SSE2/AVX2 `mc_hor20/ver02/centre` asm kernels — their throughput beats the
/// fused pass's redundancy savings. The fused builder + its byte-identity oracle
/// stay in-tree as the base for a future AVX2 fused kernel (`RFF_HPEL_FUSED=1`),
/// which is the only shape that can beat the tiles.
fn hpel_fused_enabled() -> bool {
    crate::cached_knob!(
        bool,
        false,
        crate::knob("RFF_HPEL_FUSED")
            .map(|s| s == "1")
            .unwrap_or(false)
    )
}

/// `RFF_HPEL_AVX2=0` pins the pre-kernel path (tile walk / scalar fused) for A/B —
/// the AVX2 fused pass is byte-identical, so this is a speed-experiment knob only.
#[cfg(accel)]
fn hpel_fused_forced_off() -> bool {
    crate::cached_knob!(
        bool,
        false,
        crate::knob("RFF_HPEL_AVX2")
            .map(|s| s == "0")
            .unwrap_or(false)
    )
}

/// The ORIGINAL builder — walks 16×16 tiles through `luma_tile_into` + the MC
/// kernels. Kept as the fused builder's oracle and the `RFF_HPEL_FUSED=0` path.
fn build_hpel_tiles(f: &[u8], pw: usize, ph: usize, h: &mut [u8], v: &mut [u8], c: &mut [u8]) {
    let mut tile = [0u8; LUMA_TILE * LUMA_TILE];
    let mut blk = [0u8; 256];
    let mut by = 0;
    while by < ph {
        let mut bx = 0;
        while bx < pw {
            let bh = 16.min(ph - by);
            let bwid = 16.min(pw - bx);
            let ts = luma_tile_into(&mut tile, f, pw, ph, bx as isize, by as isize, bwid, bh);
            for kind in 0u8..3 {
                let plane: &mut [u8] = match kind {
                    0 => &mut *h,
                    1 => &mut *v,
                    _ => &mut *c,
                };
                match kind {
                    0 => luma_h(&tile, ts, bwid, bh, 0, 0, &mut blk),
                    1 => luma_v(&tile, ts, bwid, bh, 0, 0, &mut blk),
                    _ => luma_centre(&tile, ts, bwid, bh, &mut blk),
                }
                for r in 0..bh {
                    plane[(by + r) * pw + bx..][..bwid].copy_from_slice(&blk[r * bwid..][..bwid]);
                }
            }
            bx += 16;
        }
        by += 16;
    }
}

/// Campaign 3 (lets-win-optimize.md): the FUSED single-pass builder — x264's
/// `hpel_filter` shape. The tile walk re-extracts a `(16+5)²` halo per 16×16 block
/// (1.7× redundant reads), re-runs per-tile dispatch, and copies each block out
/// three times; this pass reads each source row band once and writes H, V, C
/// directly. BYTE-IDENTICAL to the tile builder: the same 6-tap integer formulas
/// over the same clamped `f` samples (interior rows/cols never clamp; the ±2/3
/// borders of the padded plane clamp exactly like `luma_tile_into`'s halo does),
/// and the centre's i32 vertical intermediates reproduce `luma_centre`'s order.
/// Pinned by `fused_hpel_builder_matches_tiles`.
fn build_hpel_fused(f: &[u8], pw: usize, ph: usize, h: &mut [u8], v: &mut [u8], c: &mut [u8]) {
    let cl = |i: isize, hi: usize| i.clamp(0, hi as isize - 1) as usize;
    // Extended per-row buffers: index j covers column j-2 (clamped into the plane).
    let mut vt_buf = vec![0i32; pw + 5]; // vertical 6-tap intermediates (unrounded)
    let mut hb_buf = vec![0u8; pw + 5]; // this row's source with clamped column halo
                                        // STATE THE LENGTH ONCE. Read back as `vt[x] ..= vt[x + 5]` for `x < pw`,
                                        // which is in range by construction — but against a `Vec`'s runtime length
                                        // LLVM cannot see it, so all six taps checked on every column of every row.
                                        // Reborrowing at the literal `pw + 5` makes `x + 5 < pw + 5` provable.
    let vt = &mut vt_buf[..pw + 5];
    let hb = &mut hb_buf[..pw + 5];
    for y in 0..ph {
        let (ym2, ym1, y0, yp1, yp2, yp3) = (
            cl(y as isize - 2, ph) * pw,
            cl(y as isize - 1, ph) * pw,
            y * pw,
            cl(y as isize + 1, ph) * pw,
            cl(y as isize + 2, ph) * pw,
            cl(y as isize + 3, ph) * pw,
        );
        // Column-clamped borders (≤5 samples each side); the interior runs with
        // DIRECT indices — a per-element clamp in this loop defeats
        // autovectorization and measured the whole builder 2.2× slower.
        // SIX SOURCE ROWS, bound once. The taps stride by `pw`, which is the
        // geometry whose row-slicing was REFUTED in `luma_centre` (+9.3%) — but
        // that runs at bw = 4, where six slices replace twenty-four checks. Here
        // `pw` is the frame width, so six slices replace ~6 * pw of them. Width
        // is the variable that refutation did not vary.
        let rows: [&[u8]; 6] = [
            &f[ym2..][..pw],
            &f[ym1..][..pw],
            &f[y0..][..pw],
            &f[yp1..][..pw],
            &f[yp2..][..pw],
            &f[yp3..][..pw],
        ];
        // Branchless taps: a `.get` per row is a conditional move, and it beats
        // both the indexed form and a per-row re-slice (the shape refuted on
        // `luma_centre`). Same finding as `luma_v`.
        let tap = |x: usize| {
            let g = |k: usize| rows[k & 7].get(x).copied().unwrap_or(0) as i32;
            g(0) - 5 * g(1) + 20 * g(2) + 20 * g(3) - 5 * g(4) + g(5)
        };
        // Walk `vt`/`hb` in three ZIPPED segments rather than indexing them: the
        // buffers are `pw + 5` long and every `j` is inside, but relating the two
        // is what LLVM would not do. Three slice pairs cover all `pw + 5` writes.
        let mid = rows[2];
        let (lo, edge) = (
            mid.first().copied().unwrap_or(0),
            mid.last().copied().unwrap_or(0),
        );
        let tap_edge = tap(pw.saturating_sub(1));
        for (v, h) in vt[..2].iter_mut().zip(hb[..2].iter_mut()) {
            (*v, *h) = (tap(0), lo);
        }
        for (x, (v, h)) in vt[2..pw + 2]
            .iter_mut()
            .zip(hb[2..pw + 2].iter_mut())
            .enumerate()
        {
            (*v, *h) = (tap(x), mid.get(x).copied().unwrap_or(0));
        }
        for (v, h) in vt[pw + 2..].iter_mut().zip(hb[pw + 2..].iter_mut()) {
            (*v, *h) = (tap_edge, edge);
        }
        let hrow = &mut h[y0..y0 + pw];
        let vrow = &mut v[y0..y0 + pw];
        let crow = &mut c[y0..y0 + pw];
        // `windows(6)` hands back a slice of EXACTLY six, so `w[0]..w[5]` are
        // provable outright — the `vt[x] ..= vt[x + 5]` form needed LLVM to
        // relate `x < pw` to the buffer's length and it would not. `zip` bounds
        // the output row the same way.
        for (o, &t) in vrow.iter_mut().zip(&vt[2..]) {
            *o = clip_u8((t + 16) >> 5) as u8;
        }
        for (o, w) in crow.iter_mut().zip(vt.windows(6)) {
            let s = w[0] - 5 * w[1] + 20 * w[2] + 20 * w[3] - 5 * w[4] + w[5];
            *o = clip_u8((s + 512) >> 10) as u8;
        }
        for (o, w) in hrow.iter_mut().zip(hb.windows(6)) {
            let s = w[0] as i32 - 5 * w[1] as i32 + 20 * w[2] as i32 + 20 * w[3] as i32
                - 5 * w[4] as i32
                + w[5] as i32;
            *o = clip_u8((s + 16) >> 5) as u8;
        }
    }
}

/// Fills `out` with the `bw`×`bh` sub-pel prediction read from cached planes.
///
/// Returns `false` when the access (which may reach one sample past the block on
/// either axis, for the `m`/`s` neighbours) is not fully interior — the caller then
/// falls back to [`mc_luma`]. Bit-identical to `mc_luma` wherever it returns `true`:
/// each arm below is the same pair of operands `mc_luma_subpel` averages.
#[allow(clippy::too_many_arguments)]
/// Descent C: half-pel (single-plane, copy-free-able) vs quarter-pel (two-plane average).
#[cfg(feature = "profile")]
pub mod hpelphase {
    use crate::atomic::AtomicU64;
    use core::sync::atomic::Ordering;
    pub static C: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
    #[inline]
    pub fn bump(i: usize) {
        C[i].fetch_add(1, Ordering::Relaxed);
    }
    pub fn reset() {
        for c in C.iter() {
            c.store(0, Ordering::Relaxed);
        }
    }
    pub fn snapshot() -> Vec<u64> {
        C.iter().map(|c| c.load(Ordering::Relaxed)).collect()
    }
}

/// Descent C: the SINGLE-PLANE (half-pel) phases — (2,0)->h, (0,2)->v, (2,2)->c —
/// are already contiguous at plane stride, exactly like the interior full-pel case.
/// Hand the consumer `(plane, base, stride)` so it can read them IN PLACE rather than
/// copy 256 bytes into a temp first. Byte-identical to `hpel_block` by construction:
/// same plane, same base, same samples — only the copy is elided.
/// Returns `None` for quarter-pel (a two-plane average, which must be materialized),
/// for out-of-range coordinates, and whenever `hpel_block` would decline.
#[inline]
pub fn hpel_ref<'a>(
    p: &'a HpelPlanes,
    x0: usize,
    y0: usize,
    bw: usize,
    bh: usize,
    mvx: i32,
    mvy: i32,
) -> Option<(&'a [u8], usize, usize)> {
    let (fx, fy) = (mvx & 3, mvy & 3);
    let plane = match (fx, fy) {
        (2, 0) => &p.h,
        (0, 2) => &p.v,
        (2, 2) => &p.c,
        // Descent E: FULL-PEL off the frame edge. `hpel_block` declines these before its
        // bounds check ("the full-pel copy path is already optimal"), so the caller fell
        // back to a per-pixel clamped `mc_luma` -- measured at 76-79% of all mc_luma
        // TIME, the single largest consumer of that stage. But `f` IS the padded,
        // edge-replicated reference, so it reproduces mc_luma's clamp exactly and can be
        // read in place like any other phase. (Widening the pad does NOT fix this: the
        // decline count is identical at pad 16 and pad 64 -- it was never a bounds issue.)
        (0, 0) => &p.f,
        _ => return None,
    };
    let (ix0, iy0) = (
        x0 as isize + (mvx >> 2) as isize,
        y0 as isize + (mvy >> 2) as isize,
    );
    let (px, py) = (ix0 + p.pad as isize, iy0 + p.pad as isize);
    // Identical guard to `hpel_block`, including its `+1` slack, so the two paths
    // accept exactly the same candidate set (no bitstream drift from a wider path).
    if px < 0
        || py < 0
        || px + bw as isize + 1 > p.pw as isize
        || py + bh as isize + 1 > p.ph as isize
    {
        return None;
    }
    let base = py as usize * p.stride + px as usize;
    if base + (bh - 1) * p.stride + bw > plane.len() {
        return None;
    }
    Some((plane, base, p.stride))
}

/// Challenge-1 A3: the QUARTER-pel companion of [`hpel_ref`] — resolves the two
/// plane operands whose `(a+b+1)>>1` average IS the prediction, so the ME cost can
/// be one fused avg+SATD kernel pass instead of materialize-256-bytes-then-SATD.
/// Same bounds guard as [`hpel_block`] (identical accepted-candidate set); returns
/// `(plane_a, base_a, plane_b, base_b, stride)`. `None` for non-quarter phases
/// (those are [`hpel_ref`]'s) and whenever `hpel_block` would decline.
#[inline]
pub fn hpel_qpel_refs<'a>(
    p: &'a HpelPlanes,
    x0: usize,
    y0: usize,
    bw: usize,
    bh: usize,
    mvx: i32,
    mvy: i32,
) -> Option<(&'a [u8], usize, &'a [u8], usize, usize)> {
    let (fx, fy) = (mvx & 3, mvy & 3);
    // Quarter phases only: at least one fractional part is odd.
    if fx & 1 == 0 && fy & 1 == 0 {
        return None;
    }
    let (ix0, iy0) = (
        x0 as isize + (mvx >> 2) as isize,
        y0 as isize + (mvy >> 2) as isize,
    );
    let (px, py) = (ix0 + p.pad as isize, iy0 + p.pad as isize);
    if px < 0
        || py < 0
        || px + bw as isize + 1 > p.pw as isize
        || py + bh as isize + 1 > p.ph as isize
    {
        return None;
    }
    let stride = p.stride;
    let base = py as usize * stride + px as usize;
    let g: &[u8] = &p.f;
    // The SAME operand table as `hpel_block`'s quarter arm (the spec's `m`/`s`
    // neighbour shifts baked into the base offsets).
    let (pa, oa, pb, ob): (&[u8], usize, &[u8], usize) = match (fx, fy) {
        (1, 0) => (g, 0, &p.h, 0),
        (3, 0) => (g, 1, &p.h, 0),
        (0, 1) => (g, 0, &p.v, 0),
        (0, 3) => (g, stride, &p.v, 0),
        (1, 1) => (&p.h, 0, &p.v, 0),
        (3, 1) => (&p.h, 0, &p.v, 1),
        (1, 3) => (&p.h, stride, &p.v, 0),
        (3, 3) => (&p.h, stride, &p.v, 1),
        (2, 1) => (&p.h, 0, &p.c, 0),
        (2, 3) => (&p.h, stride, &p.c, 0),
        (1, 2) => (&p.v, 0, &p.c, 0),
        _ => (&p.v, 1, &p.c, 0), // (3, 2)
    };
    // Slice-length check so a short plane can never OOB (the `+1` slack in the
    // bounds test above covers the shifted operand's reach geometrically; this
    // makes it a hard guarantee at the slice level too).
    if base + oa + (bh - 1) * stride + bw > pa.len()
        || base + ob + (bh - 1) * stride + bw > pb.len()
    {
        return None;
    }
    Some((pa, base + oa, pb, base + ob, stride))
}

pub fn hpel_block(
    p: &HpelPlanes,
    x0: usize,
    y0: usize,
    bw: usize,
    bh: usize,
    mvx: i32,
    mvy: i32,
    out: &mut [u8],
) -> bool {
    let _g = crate::prof::scope(crate::prof::Stage::MeHpel);
    let (ix0, iy0) = (
        x0 as isize + (mvx >> 2) as isize,
        y0 as isize + (mvy >> 2) as isize,
    );
    let (fx, fy) = (mvx & 3, mvy & 3);
    if fx == 0 && fy == 0 {
        return false; // the full-pel copy path is already optimal
    }
    // Padded coordinates. The `+1` slack covers the `m`/`s` neighbours.
    let (px, py) = (ix0 + p.pad as isize, iy0 + p.pad as isize);
    if px < 0
        || py < 0
        || px + bw as isize + 1 > p.pw as isize
        || py + bh as isize + 1 > p.ph as isize
    {
        return false;
    }
    let stride = p.stride;
    let base = py as usize * stride + px as usize;

    let single = match (fx, fy) {
        (2, 0) => Some(&p.h),
        (0, 2) => Some(&p.v),
        (2, 2) => Some(&p.c),
        _ => None,
    };
    // Descent C: single-plane (half-pel) reads are CONTIGUOUS AT PLANE STRIDE — they
    // could be SATD'd in place like the interior full-pel path instead of copied into
    // a temp. Count the split before building that path.
    #[cfg(feature = "profile")]
    crate::inter::hpelphase::bump(if single.is_some() { 0 } else { 1 });
    if let Some(src) = single {
        for r in 0..bh {
            out[r * bw..r * bw + bw].copy_from_slice(&src[base + r * stride..][..bw]);
        }
        return true;
    }

    // Quarter-pel: (a + b + 1) >> 1 of two planes. The operand table (the spec's
    // `m`/`s` neighbour shifts) lives in ONE place — `hpel_qpel_refs` — so the
    // materializing path here and the fused avg+SATD path can never drift apart.
    let Some((pa, ba, pb, bb, qstride)) = hpel_qpel_refs(p, x0, y0, bw, bh, mvx, mvy) else {
        return false;
    };
    // The asm read (inline-execution.md §11.6c) refuted "LLVM already gets this":
    // all three `avg_rows` widths compile fully SCALAR — 0 `pavgb`, 0 xmm — while
    // `pixel_avg` is the SAME `(a+b+1)>>1` with SSE2/NEON arms and a differential
    // test. Compose, don't write: the kernel takes the accel build; `avg_rows`
    // stays below as the reachable scalar twin (non-accel builds and the oracle).
    #[cfg(accel)]
    if bw == 16 || bw == 8 || bw == 4 {
        rusty_h264_accel::pixel_avg(out, &pa[ba..], qstride, &pb[bb..], qstride, bw, bh);
        return true;
    }
    match bw {
        16 => avg_rows::<16>(pa, ba, pb, bb, qstride, bh, out),
        8 => avg_rows::<8>(pa, ba, pb, bb, qstride, bh, out),
        4 => avg_rows::<4>(pa, ba, pb, bb, qstride, bh, out),
        _ => return false,
    }
    true
}

#[inline]
fn avg_rows<const BW: usize>(
    pa: &[u8],
    oa: usize,
    pb: &[u8],
    ob: usize,
    cw: usize,
    bh: usize,
    out: &mut [u8],
) {
    for r in 0..bh {
        let sa = &pa[oa + r * cw..][..BW];
        let sb = &pb[ob + r * cw..][..BW];
        let o = &mut out[r * BW..r * BW + BW];
        for i in 0..BW {
            o[i] = ((sa[i] as u16 + sb[i] as u16 + 1) >> 1) as u8;
        }
    }
}

/// Quarter-pel motion compensation of a `bw`×`bh` luma block (`McLuma_c`).
#[allow(clippy::too_many_arguments)]
/// Descent E: accumulates one `mc_luma` call's cycles into its (size, phase) bucket.
#[cfg(feature = "profile")]
struct McCycleGuard {
    b: usize,
    t: u64,
}
#[cfg(feature = "profile")]
impl Drop for McCycleGuard {
    fn drop(&mut self) {
        let c = crate::prof::tick().wrapping_sub(self.t);
        mcstats::add_cycles(self.b, c);
        mcstats::site_add(c);
    }
}

pub fn mc_luma(
    reference: &[u8],
    cw: usize,
    ch: usize,
    x0: usize,
    y0: usize,
    bw: usize,
    bh: usize,
    mvx: i32,
    mvy: i32,
    out: &mut [u8],
) {
    let _g = crate::prof::scope(crate::prof::Stage::InterMc);
    if abl_mc() {
        out.fill(128);
        return;
    }
    let (ix0, iy0) = (
        x0 as isize + (mvx >> 2) as isize,
        y0 as isize + (mvy >> 2) as isize,
    );
    let (fx, fy) = (mvx & 3, mvy & 3);
    #[cfg(feature = "profile")]
    mcstats::record(bw, bh, fx, fy);
    // Descent E: time this call into its (size, phase) bucket. Guard drops at fn end.
    #[cfg(feature = "profile")]
    let _mcg = McCycleGuard {
        b: mcstats::bucket(bw, bh, fx, fy),
        t: crate::prof::tick(),
    };
    if fx == 0 && fy == 0 {
        // Full-pel: a verbatim copy of the reference (`McCopy_c`). Interior → a
        // row-wise slice copy (auto-vectorized); edge → per-pixel clamped.
        if ix0 >= 0
            && iy0 >= 0
            && ix0 + bw as isize <= cw as isize
            && iy0 + bh as isize <= ch as isize
            && reference.len() >= cw * ch
        {
            let (rx, ry) = (ix0 as usize, iy0 as usize);
            // Fixed-size fast path for the full 16×16 MB (the overwhelming common
            // case). With `bw` a runtime parameter each `copy_from_slice(..bw)` is a
            // variable-length `memcpy` CALL per row (~38 ns / MB); const-16 slices
            // let LLVM emit inline 16-byte moves (~2 ns / MB). Byte-identical.
            if bw == 16 && bh == 16 {
                for dy in 0..16 {
                    let s = &reference[(ry + dy) * cw + rx..];
                    out[dy * 16..dy * 16 + 16].copy_from_slice(&s[..16]);
                }
            } else {
                for dy in 0..bh {
                    out[dy * bw..dy * bw + bw]
                        .copy_from_slice(&reference[(ry + dy) * cw + rx..][..bw]);
                }
            }
        } else {
            for dy in 0..bh {
                let orow = &mut out[dy * bw..][..bw];
                for (dx, o) in orow.iter_mut().enumerate() {
                    *o = at(reference, cw, ch, ix0 + dx as isize, iy0 + dy as isize) as u8;
                }
            }
        }
        return;
    }
    // Sub-pel: extract the clamped tile once, then run the openh264 block kernels.
    // Tile + half-pel staging live in per-thread scratch (see `McScratch`) so the
    // ~1.4 KB of zero-fill and the tile's return-by-value copy are not repaid on
    // every motion-search candidate. Destructuring gives `tile` and `a`/`b` as
    // disjoint borrows of the same scratch.
    MC_SCRATCH.with(|s| {
        let McScratch { tile, a, b } = &mut *s.borrow_mut();
        let ts = luma_tile_into(tile, reference, cw, ch, ix0, iy0, bw, bh);
        mc_luma_subpel(tile, ts, bw, bh, fx, fy, a, b, out);
    });
}

/// Eighth-pel bilinear motion compensation of a `bw`×`bh` chroma block (spec
/// §8.4.2.2.2). The chroma motion vector equals the luma MV; for 4:2:0 it is
/// interpreted at eighth-chroma-sample resolution.
#[allow(clippy::too_many_arguments)]
pub fn mc_chroma(
    reference: &[u8],
    cw: usize,
    ch: usize,
    x0: usize,
    y0: usize,
    bw: usize,
    bh: usize,
    mvx: i32,
    mvy: i32,
    out: &mut [u8],
) {
    let _g = crate::prof::scope(crate::prof::Stage::InterMc);
    if abl_mc() {
        out.fill(128);
        return;
    }
    let (ix0, iy0) = (
        x0 as isize + (mvx >> 3) as isize,
        y0 as isize + (mvy >> 3) as isize,
    );
    let (fx, fy) = (mvx & 7, mvy & 7);
    // Full-pel and fully inside the frame: `(64·a + 32) >> 6 == a`, a verbatim copy.
    // Skip the per-pixel bilinear + 4× clamped `at()`; copy row-wise. Bit-identical.
    if fx == 0
        && fy == 0
        && ix0 >= 0
        && iy0 >= 0
        && ix0 + bw as isize <= cw as isize
        && iy0 + bh as isize <= ch as isize
        && reference.len() >= cw * ch
    {
        let (rx, ry) = (ix0 as usize, iy0 as usize);
        // Const-8 fast path for the full 8×8 chroma block (skip / P_16x16 common
        // case): fixed-size slices → inline 8-byte moves, not a runtime-length
        // `memcpy` call per row. Byte-identical. See the luma twin in `mc_luma`.
        if bw == 8 && bh == 8 {
            for dy in 0..8 {
                let src = &reference[(ry + dy) * cw + rx..];
                out[dy * 8..dy * 8 + 8].copy_from_slice(&src[..8]);
            }
        } else {
            for dy in 0..bh {
                let src = &reference[(ry + dy) * cw + rx..][..bw];
                out[dy * bw..dy * bw + bw].copy_from_slice(src);
            }
        }
        return;
    }
    // Sub-pel (and full-pel edge): extract the clamped (bw+1)×(bh+1) tile once —
    // the edge-extended input `McChromaWithFragMv_c` reads — then the bilinear
    // `A·p + B·p₊₁ + C·p₊ₛ + D·p₊ₛ₊₁` (`g_kuiABCD` weights), `(·+32)>>6`. A chroma
    // block is at most 8×8 (half a 16×16 MB), so a 9×9 tile suffices. Bit-identical:
    // the clamped tile reproduces `at()`, and full-pel weights give `(64·a+32)>>6==a`.
    let ts = bw + 1;
    let mut t = [0u8; 9 * 9];
    // Interior fast path (see `luma_tile`): clamp-free contiguous row copies.
    if ix0 >= 0
        && iy0 >= 0
        && ix0 + ts as isize <= cw as isize
        && iy0 + (bh + 1) as isize <= ch as isize
        && reference.len() >= cw * ch
    {
        let (rx0, ry0) = (ix0 as usize, iy0 as usize);
        for ty in 0..bh + 1 {
            let src = (ry0 + ty) * cw + rx0;
            t[ty * ts..ty * ts + ts].copy_from_slice(&reference[src..src + ts]);
        }
    } else {
        for ty in 0..bh + 1 {
            let ry = (iy0 + ty as isize).clamp(0, ch as isize - 1) as usize * cw;
            let trow = &mut t[ty * ts..][..ts];
            for (tx, o) in trow.iter_mut().enumerate() {
                let rx = (ix0 + tx as isize).clamp(0, cw as isize - 1) as usize;
                *o = reference.get(ry + rx).copied().unwrap_or(0);
            }
        }
    }
    let (wa, wb, wc, wd) = ((8 - fx) * (8 - fy), fx * (8 - fy), (8 - fx) * fy, fx * fy);
    // 8-wide chroma (full-MB and 16×16-partition chroma — the common case) → the
    // openh264 SSE2 bilinear over the same clamped tile. Width 2/4 stay scalar
    // (width-4 is only an MMX kernel; width 2 has none). Bit-identical.
    #[cfg(accel)]
    if bw == 8 {
        let abcd = [wa as u8, wb as u8, wc as u8, wd as u8];
        rusty_h264_accel::mc_chroma_w8(&t, ts, out, bw, &abcd, bh);
        return;
    }
    // Fourth site of the two-row `windows(2)` bilinear (see `mc_chroma_padded`).
    for r in 0..bh {
        let (r0, r1) = (&t[r * ts..][..bw + 1], &t[(r + 1) * ts..][..bw + 1]);
        let orow = &mut out[r * bw..][..bw];
        for ((o, a), b) in orow.iter_mut().zip(r0.windows(2)).zip(r1.windows(2)) {
            let v = wa * a[0] as i32 + wb * a[1] as i32 + wc * b[0] as i32 + wd * b[1] as i32;
            *o = ((v + 32) >> 6) as u8;
        }
    }
}

// ---- Padded-reference MC (openh264 ExpandPicture style) ----
//
// A reference plane is stored with a replicated-edge border (`PAD_L` luma /
// `PAD_C` chroma pixels), the picture origin at `(pad, pad)` and `stride` = the
// padded width. MC then reads the frame DIRECTLY at the MV offset — no per-call
// clamped tile — because reads into the border hit valid replicated pixels. For
// the rare MV whose 6-tap halo would exceed the border, we fall back to the
// clamped tile (reading the padded interior), which is bit-identical to the
// exact-frame path.
//
// STATUS: implemented + bit-exact (the `mc_*_padded_matches_exact` tests + a full
// decoder wiring verified 35/35 corpus MATCH), but measured **~0** vs the tile
// path on x86-64 — once `luma_tile`'s interior fast path made extraction a
// vectorized copy, the remaining win (skipping the copy) is offset by the
// padded direct read's worse kernel cache locality (full-frame stride vs the
// L1-resident tile) plus the per-frame expand/copy cost. Kept UNWIRED as a ready
// option for a workload/target where tile extraction dominates (e.g. slower
// memory, or hand-asm kernels tuned for the big-stride read). To wire: store
// `RefFrame` planes padded (`expand_plane` in `as_reference`) and call these
// instead of `mc_luma`/`mc_chroma`.

/// Luma reference border width (covers full-pel MVs to ±30 px before fallback).
pub const PAD_L: usize = 32;
/// Chroma reference border width (= `PAD_L`/2, matching the half-rate chroma MV).
pub const PAD_C: usize = 16;

/// Fills the `pad`-wide replicated-edge border of a plane whose picture (`pw×ph`)
/// sits at offset `(pad, pad)` with `stride`. Mirrors openh264 `ExpandPictureLuma_c`:
/// left/right cols replicate the edge pixel; top/bottom rows replicate the (already
/// edge-filled) first/last picture row, so the corners come out right.
pub fn expand_plane(buf: &mut [u8], stride: usize, pad: usize, pw: usize, ph: usize) {
    // ONE slice per row, then two fills. The edge reads and the 2*pad writes were
    // all separate checks into the whole plane; the row segment covers them.
    for y in 0..ph {
        // Split rather than index: `seg[pad]` and `seg[pad + pw - 1]` still
        // checked, because nothing rules out `pw == 0`. `first`/`last` on the
        // middle run say the same thing without a panic path.
        let seg = &mut buf[(y + pad) * stride..][..2 * pad + pw];
        let (lpad, rest) = seg.split_at_mut(pad);
        let (mid, rpad) = rest.split_at_mut(pw);
        let (Some(&left), Some(&right)) = (mid.first(), mid.last()) else {
            continue;
        };
        lpad.fill(left);
        rpad.fill(right);
    }
    let first = pad * stride;
    let last = (pad + ph - 1) * stride;
    for y in 0..pad {
        buf.copy_within(first..first + stride, y * stride);
        buf.copy_within(last..last + stride, (pad + ph + y) * stride);
    }
}

/// Quarter-pel luma MC reading a padded reference directly (no clamped tile when
/// the halo is in-border). `x0,y0` are the block's picture coords; `stride`/`pad`
/// describe the padded plane; `pw,ph` are the picture dims. Bit-identical to
/// [`mc_luma`] on the equivalent exact frame.
#[allow(clippy::too_many_arguments)]
/// MEASUREMENT KNOB — `RFF_ABL_MC=1` makes both padded motion-compensation
/// primitives return immediately with a flat block, pricing INTER PREDICTION by
/// ablation on the UNINSTRUMENTED binary.
///
/// Placed inside the two primitives rather than at their ~13 decoder call sites so
/// the ablation cannot miss one, and so every caller, every call COUNT and all the
/// surrounding glue (partition walk, MV derivation, buffer setup) stay exactly as
/// they were — what is priced is the interpolation work itself, nothing else.
///
/// Frame count is unaffected: parsing and residual decoding are untouched. Output
/// pixels are wrong while this is set, by design — it is never a shipping path.
/// Read once; the branch predicts perfectly.
#[inline]
pub(crate) fn abl_mc() -> bool {
    // ROUTED AT BUILD TIME (routing round 2026-09-05): the shipped arm is the
    // constant below; the env arm exists only under `--features knobs`.
    #[cfg(not(feature = "knobs"))]
    {
        return false;
    }
    #[cfg(feature = "knobs")]
    {
        use core::sync::atomic::{AtomicU8, Ordering};
        static ON: AtomicU8 = AtomicU8::new(0);
        match ON.load(Ordering::Relaxed) {
            1 => true,
            2 => false,
            _ => {
                let on = crate::knob("RFF_ABL_MC").is_some_and(|v| v != "0");
                ON.store(if on { 1 } else { 2 }, Ordering::Relaxed);
                on
            }
        }
    }
}

pub fn mc_luma_padded(
    padded: &[u8],
    stride: usize,
    pad: usize,
    pw: usize,
    ph: usize,
    x0: usize,
    y0: usize,
    bw: usize,
    bh: usize,
    mvx: i32,
    mvy: i32,
    out: &mut [u8],
) {
    with_mc_scratch(|scr| {
        mc_luma_padded_pre(
            scr, padded, stride, pad, pw, ph, x0, y0, bw, bh, mvx, mvy, out,
        )
    })
}

/// [`mc_luma_padded`] with the scratch pre-borrowed (see [`with_mc_scratch`]).
#[allow(clippy::too_many_arguments)]
pub fn mc_luma_padded_pre(
    scr: &mut McScratch,
    padded: &[u8],
    stride: usize,
    pad: usize,
    pw: usize,
    ph: usize,
    x0: usize,
    y0: usize,
    bw: usize,
    bh: usize,
    mvx: i32,
    mvy: i32,
    out: &mut [u8],
) {
    let _g = crate::prof::scope(crate::prof::Stage::InterMc);
    let (ix0, iy0) = (
        x0 as isize + (mvx >> 2) as isize,
        y0 as isize + (mvy >> 2) as isize,
    );
    let (fx, fy) = (mvx & 3, mvy & 3);
    // Size x phase census for the DECODER's MC. The facility existed but was wired
    // only into `mc_luma` (the encoder's), so the decoder's own distribution had
    // never been measured -- and it is the one that decides which const-width fast
    // paths are worth having.
    #[cfg(feature = "profile")]
    mcstats::record(bw, bh, fx, fy);
    #[cfg(feature = "profile")]
    let _mcg = McCycleGuard {
        b: mcstats::bucket(bw, bh, fx, fy),
        t: crate::prof::tick(),
    };
    let p = pad as isize;
    let (lo_x, lo_y) = (ix0 - 2, iy0 - 2);
    // Malformed-stream armor: a mutated stream can hand a reference whose plane
    // no longer matches the caller's geometry (mid-stream SPS change). The fast
    // paths index arithmetically, so they require the buffer to be intact; the
    // fallback below reads checked. One compare on the hot path.
    let intact = stride >= pw + 2 * pad && padded.len() >= stride * (ph + 2 * pad);
    let in_range = intact
        && lo_x >= -p
        && lo_y >= -p
        && lo_x + (bw + 5) as isize <= pw as isize + p
        && lo_y + (bh + 5) as isize <= ph as isize + p;
    if in_range {
        if fx == 0 && fy == 0 {
            // CONST-WIDTH row copies. With a runtime `bw` each row lowers to a
            // variable-length `memcpy` CALL, and the census priced a 16x16 full-pel
            // copy at 377 cycles -- ~10x what 256 bytes should cost. H.264 emits
            // only these five widths.
            // ONE span each side, then const-width rows inside it: the source
            // and destination bounds checks are paid once for the block instead
            // of once per row, and the widths stay literal so each row is still
            // a fixed-size move rather than a variable-length memcpy CALL.
            let sb = ((iy0 + p) as usize) * stride + (ix0 + p) as usize;
            macro_rules! rows {
                ($n:expr) => {{
                    let sw = &padded[sb..sb + (bh - 1) * stride + $n];
                    let ow = &mut out[..(bh - 1) * $n + $n];
                    for dy in 0..bh {
                        ow[dy * $n..dy * $n + $n]
                            .copy_from_slice(&sw[dy * stride..dy * stride + $n]);
                    }
                }};
            }
            match bw {
                16 => rows!(16),
                8 => rows!(8),
                4 => rows!(4),
                _ => {
                    let sw = &padded[sb..sb + (bh - 1) * stride + bw];
                    let ow = &mut out[..(bh - 1) * bw + bw];
                    for dy in 0..bh {
                        ow[dy * bw..dy * bw + bw]
                            .copy_from_slice(&sw[dy * stride..dy * stride + bw]);
                    }
                }
            }
        } else {
            let halo = ((lo_y + p) as usize) * stride + (lo_x + p) as usize;
            mc_luma_subpel(
                &padded[halo..],
                stride,
                bw,
                bh,
                fx,
                fy,
                &mut scr.a,
                &mut scr.b,
                out,
            );
        }
        return;
    }
    // Extreme MV: clamp the halo to the real picture, read the padded interior.
    let ts = bw + 5;
    let mut t = [0u8; LUMA_TILE * LUMA_TILE];
    for ty in 0..bh + 5 {
        let py = (lo_y + ty as isize).clamp(0, ph as isize - 1) as usize;
        let ry = (py + pad) * stride;
        // Checked source (a grey sample beats a panic on garbage input); the
        // DESTINATION row is contiguous, so one slice covers the whole run.
        let trow = &mut t[ty * ts..][..ts];
        for (tx, o) in trow.iter_mut().enumerate() {
            let px = (lo_x + tx as isize).clamp(0, pw as isize - 1) as usize;
            *o = padded.get(ry + px + pad).copied().unwrap_or(128);
        }
    }
    if fx == 0 && fy == 0 {
        for dy in 0..bh {
            let s = (dy + 2) * ts + 2;
            out[dy * bw..dy * bw + bw].copy_from_slice(&t[s..s + bw]);
        }
    } else {
        mc_luma_subpel(&t, ts, bw, bh, fx, fy, &mut scr.a, &mut scr.b, out);
    }
}

/// Eighth-pel chroma MC reading a padded reference directly. Bit-identical to
/// [`mc_chroma`] on the equivalent exact frame.
#[allow(clippy::too_many_arguments)]
/// U+V pair form of [`mc_chroma_padded`]: both chroma planes share EVERY piece
/// of MC geometry (mv, frac weights, stride, coordinates, bounds) — only the
/// plane base differs — so one setup + one range check serves two kernel
/// invocations. Per-plane pixel math is byte-identical to two single calls.
/// The extreme-MV clamp path is rare and stays on the single-plane fn.
#[allow(clippy::too_many_arguments)]
pub fn mc_chroma_padded_pair(
    pu: &[u8],
    pv: &[u8],
    stride: usize,
    pad: usize,
    pw: usize,
    ph: usize,
    x0: usize,
    y0: usize,
    bw: usize,
    bh: usize,
    mvx: i32,
    mvy: i32,
    outu: &mut [u8],
    outv: &mut [u8],
) {
    let _g = crate::prof::scope(crate::prof::Stage::InterMc);
    let (ix0, iy0) = (
        x0 as isize + (mvx >> 3) as isize,
        y0 as isize + (mvy >> 3) as isize,
    );
    let (fx, fy) = (mvx & 7, mvy & 7);
    let p = pad as isize;
    let intact = stride >= pw + 2 * pad
        && pu.len() >= stride * (ph + 2 * pad)
        && pv.len() >= stride * (ph + 2 * pad);
    let in_range = intact
        && ix0 >= -p
        && iy0 >= -p
        && ix0 + (bw + 1) as isize <= pw as isize + p
        && iy0 + (bh + 1) as isize <= ph as isize + p;
    if !in_range {
        // Rare (extreme MV or short plane): fall back to the single-plane fn,
        // whose clamp path is the conformance reference.
        mc_chroma_padded(pu, stride, pad, pw, ph, x0, y0, bw, bh, mvx, mvy, outu);
        mc_chroma_padded(pv, stride, pad, pw, ph, x0, y0, bw, bh, mvx, mvy, outv);
        return;
    }
    if fx == 0 && fy == 0 {
        for dy in 0..bh {
            let src = ((iy0 + dy as isize + p) as usize) * stride + (ix0 + p) as usize;
            outu[dy * bw..dy * bw + bw].copy_from_slice(&pu[src..src + bw]);
            outv[dy * bw..dy * bw + bw].copy_from_slice(&pv[src..src + bw]);
        }
        return;
    }
    let (wa, wb, wc, wd) = ((8 - fx) * (8 - fy), fx * (8 - fy), (8 - fx) * fy, fx * fy);
    let halo = ((iy0 + p) as usize) * stride + (ix0 + p) as usize;
    #[cfg(accel)]
    {
        let abcd = [wa as u8, wb as u8, wc as u8, wd as u8];
        if bw == 8 {
            rusty_h264_accel::mc_chroma_w8(&pu[halo..], stride, outu, bw, &abcd, bh);
            rusty_h264_accel::mc_chroma_w8(&pv[halo..], stride, outv, bw, &abcd, bh);
            return;
        }
        if bw == 4 {
            rusty_h264_accel::mc_chroma_w4(&pu[halo..], stride, outu, bw, &abcd, bh);
            rusty_h264_accel::mc_chroma_w4(&pv[halo..], stride, outv, bw, &abcd, bh);
            return;
        }
        // 2-WIDE (the chroma of 4-wide luma partitions; finding #6): the w4
        // kernel into a 4-stride scratch, keep 2 columns per row. The two extra
        // source columns are in bounds by the same check the kernel asserts.
        if bw == 2
            && bh <= 8
            && pu.len() >= halo + bh * stride + 5
            && pv.len() >= halo + bh * stride + 5
        {
            for (pl, out) in [(pu, &mut *outu), (pv, &mut *outv)] {
                let mut s = [0u8; 4 * 8];
                rusty_h264_accel::mc_chroma_w4(&pl[halo..], stride, &mut s, 4, &abcd, bh);
                for (d, srow) in out.chunks_exact_mut(2).zip(s.chunks_exact(4)).take(bh) {
                    d.copy_from_slice(&srow[..2]);
                }
            }
            return;
        }
    }
    for (pl, out) in [(pu, &mut *outu), (pv, &mut *outv)] {
        // TWO ROWS, `windows(2)`. The bilinear tap reads (c, c+1) on this row
        // and the next — four separately checked whole-plane loads per
        // sample. Each window is a slice of EXACTLY two, so all four fold.
        for r in 0..bh {
            let base = halo + r * stride;
            let (r0, r1) = (&pl[base..][..bw + 1], &pl[base + stride..][..bw + 1]);
            let orow = &mut out[r * bw..][..bw];
            for ((o, a), b) in orow.iter_mut().zip(r0.windows(2)).zip(r1.windows(2)) {
                let v = wa * a[0] as i32 + wb * a[1] as i32 + wc * b[0] as i32 + wd * b[1] as i32;
                *o = ((v + 32) >> 6) as u8;
            }
        }
    }
}

pub fn mc_chroma_padded(
    padded: &[u8],
    stride: usize,
    pad: usize,
    pw: usize,
    ph: usize,
    x0: usize,
    y0: usize,
    bw: usize,
    bh: usize,
    mvx: i32,
    mvy: i32,
    out: &mut [u8],
) {
    let _g = crate::prof::scope(crate::prof::Stage::InterMc);
    let (ix0, iy0) = (
        x0 as isize + (mvx >> 3) as isize,
        y0 as isize + (mvy >> 3) as isize,
    );
    let (fx, fy) = (mvx & 7, mvy & 7);
    let p = pad as isize;
    let intact = stride >= pw + 2 * pad && padded.len() >= stride * (ph + 2 * pad);
    let in_range = intact
        && ix0 >= -p
        && iy0 >= -p
        && ix0 + (bw + 1) as isize <= pw as isize + p
        && iy0 + (bh + 1) as isize <= ph as isize + p;
    let (wa, wb, wc, wd) = ((8 - fx) * (8 - fy), fx * (8 - fy), (8 - fx) * fy, fx * fy);
    if in_range {
        if fx == 0 && fy == 0 {
            // ONE span each side: `bh` rows a fixed stride apart, so the
            // source and destination checks are paid once, not per row.
            let sb = ((iy0 + p) as usize) * stride + (ix0 + p) as usize;
            let sw = &padded[sb..sb + (bh - 1) * stride + bw];
            let ow = &mut out[..(bh - 1) * bw + bw];
            for dy in 0..bh {
                ow[dy * bw..dy * bw + bw].copy_from_slice(&sw[dy * stride..dy * stride + bw]);
            }
            return;
        }
        let halo = ((iy0 + p) as usize) * stride + (ix0 + p) as usize;
        #[cfg(accel)]
        {
            // H-38: BOTH widths now take asm. The 4-wide path was missing, so every
            // 8×8 B-direct sub-block and sub-8×8 partition fell to the scalar
            // bilinear below — chroma MC then cost as much as luma on half the
            // pixels. Both kernels are pinned bit-identical to that scalar arm.
            let abcd = [wa as u8, wb as u8, wc as u8, wd as u8];
            if bw == 8 {
                rusty_h264_accel::mc_chroma_w8(&padded[halo..], stride, out, bw, &abcd, bh);
                return;
            }
            if bw == 4 {
                rusty_h264_accel::mc_chroma_w4(&padded[halo..], stride, out, bw, &abcd, bh);
                return;
            }
            if bw == 2 && bh <= 8 && padded.len() >= halo + bh * stride + 5 {
                let mut s = [0u8; 4 * 8];
                rusty_h264_accel::mc_chroma_w4(&padded[halo..], stride, &mut s, 4, &abcd, bh);
                for (d, srow) in out.chunks_exact_mut(2).zip(s.chunks_exact(4)).take(bh) {
                    d.copy_from_slice(&srow[..2]);
                }
                return;
            }
        }
        // TWO ROWS, `windows(2)`. The bilinear tap reads (c, c+1) on this row
        // and the next — four separately checked whole-plane loads per
        // sample. Each window is a slice of EXACTLY two, so all four fold.
        for r in 0..bh {
            let base = halo + r * stride;
            let (r0, r1) = (
                &padded[base..][..bw + 1],
                &padded[base + stride..][..bw + 1],
            );
            let orow = &mut out[r * bw..][..bw];
            for ((o, a), b) in orow.iter_mut().zip(r0.windows(2)).zip(r1.windows(2)) {
                let v = wa * a[0] as i32 + wb * a[1] as i32 + wc * b[0] as i32 + wd * b[1] as i32;
                *o = ((v + 32) >> 6) as u8;
            }
        }
        return;
    }
    // Extreme MV: clamp the (bw+1)² halo to the real picture, read the padded interior.
    let ts = bw + 1;
    let mut t = [0u8; 9 * 9];
    for ty in 0..bh + 1 {
        let py = (iy0 + ty as isize).clamp(0, ph as isize - 1) as usize;
        let ry = (py + pad) * stride;
        let trow = &mut t[ty * ts..][..ts];
        for (tx, o) in trow.iter_mut().enumerate() {
            let px = (ix0 + tx as isize).clamp(0, pw as isize - 1) as usize;
            *o = padded.get(ry + px + pad).copied().unwrap_or(128);
        }
    }
    // Same two-row `windows(2)` shape as the padded chroma paths.
    for r in 0..bh {
        let (r0, r1) = (&t[r * ts..][..bw + 1], &t[(r + 1) * ts..][..bw + 1]);
        let orow = &mut out[r * bw..][..bw];
        for ((o, a), b) in orow.iter_mut().zip(r0.windows(2)).zip(r1.windows(2)) {
            let v = wa * a[0] as i32 + wb * a[1] as i32 + wc * b[0] as i32 + wd * b[1] as i32;
            *o = ((v + 32) >> 6) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rand_plane(pw: usize, ph: usize, seed: u32) -> Vec<u8> {
        let mut s = seed;
        (0..pw * ph)
            .map(|_| {
                s = s.wrapping_mul(1103515245).wrapping_add(12345);
                (s >> 16) as u8
            })
            .collect()
    }

    fn make_padded(exact: &[u8], pw: usize, ph: usize, pad: usize) -> (Vec<u8>, usize) {
        let stride = pw + 2 * pad;
        let mut padded = vec![0u8; stride * (ph + 2 * pad)];
        for y in 0..ph {
            let d = (y + pad) * stride + pad;
            padded[d..d + pw].copy_from_slice(&exact[y * pw..y * pw + pw]);
        }
        expand_plane(&mut padded, stride, pad, pw, ph);
        (padded, stride)
    }

    #[test]
    fn mc_luma_padded_matches_exact() {
        let (pw, ph) = (48usize, 32usize);
        let exact = rand_plane(pw, ph, 0x77);
        let (padded, stride) = make_padded(&exact, pw, ph, PAD_L);
        for &(bw, bh) in &[(16usize, 16usize), (8, 8), (16, 8), (8, 16), (4, 4)] {
            for x0 in [0usize, 8, pw - bw] {
                for y0 in [0usize, 8, ph - bh] {
                    for mvx in [-40i32, -20, -3, 0, 1, 2, 3, 7, 20, 40] {
                        for mvy in [-40i32, -20, -3, 0, 1, 2, 3, 7, 20, 40] {
                            let mut a = vec![0u8; bw * bh];
                            let mut b = vec![0u8; bw * bh];
                            mc_luma(&exact, pw, ph, x0, y0, bw, bh, mvx, mvy, &mut a);
                            mc_luma_padded(
                                &padded, stride, PAD_L, pw, ph, x0, y0, bw, bh, mvx, mvy, &mut b,
                            );
                            assert_eq!(a, b, "luma bw={bw} x0={x0} y0={y0} mv=({mvx},{mvy})");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn mc_chroma_padded_matches_exact() {
        let (pw, ph) = (24usize, 16usize);
        let exact = rand_plane(pw, ph, 0x99);
        let (padded, stride) = make_padded(&exact, pw, ph, PAD_C);
        for &(bw, bh) in &[(8usize, 8usize), (4, 4), (8, 4), (4, 8), (2, 2)] {
            for x0 in [0usize, 4, pw - bw] {
                for y0 in [0usize, 4, ph - bh] {
                    for mvx in [-40i32, -16, -3, 0, 1, 5, 8, 16, 40] {
                        for mvy in [-40i32, -16, -3, 0, 1, 5, 8, 16, 40] {
                            let mut a = vec![0u8; bw * bh];
                            let mut b = vec![0u8; bw * bh];
                            mc_chroma(&exact, pw, ph, x0, y0, bw, bh, mvx, mvy, &mut a);
                            mc_chroma_padded(
                                &padded, stride, PAD_C, pw, ph, x0, y0, bw, bh, mvx, mvy, &mut b,
                            );
                            assert_eq!(a, b, "chroma bw={bw} x0={x0} y0={y0} mv=({mvx},{mvy})");
                        }
                    }
                }
            }
        }
    }

    /// Campaign-3 oracle: the fused single-pass builder must fill H/V/C
    /// byte-for-byte like the tile-walk builder, across geometries whose padded
    /// dimensions are and are not multiples of 16 (partial edge tiles) and with
    /// enough randomness to exercise the clip.
    #[test]
    fn fused_hpel_builder_matches_tiles() {
        let mut st = 0x5eed_f00d_dead_beefu64;
        let mut lcg = move || {
            st = st
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (st >> 33) as u8
        };
        for &(cw, ch, pad) in &[(64usize, 48usize, 16usize), (80, 64, 12), (176, 144, 32)] {
            let reference: Vec<u8> = (0..cw * ch).map(|_| lcg()).collect();
            let (pw, ph) = (cw + 2 * pad, ch + 2 * pad);
            // The same edge-replicated padded source `build_hpel_planes` constructs.
            let mut f = vec![0u8; pw * ph];
            for y in 0..ph {
                let sy = (y as isize - pad as isize).clamp(0, ch as isize - 1) as usize;
                let row = &reference[sy * cw..sy * cw + cw];
                let d = &mut f[y * pw..y * pw + pw];
                d[..pad].fill(row[0]);
                d[pad..pad + cw].copy_from_slice(row);
                d[pad + cw..].fill(row[cw - 1]);
            }
            let (mut h1, mut v1, mut c1) =
                (vec![0u8; pw * ph], vec![0u8; pw * ph], vec![0u8; pw * ph]);
            let (mut h2, mut v2, mut c2) =
                (vec![0u8; pw * ph], vec![0u8; pw * ph], vec![0u8; pw * ph]);
            build_hpel_tiles(&f, pw, ph, &mut h1, &mut v1, &mut c1);
            build_hpel_fused(&f, pw, ph, &mut h2, &mut v2, &mut c2);
            assert_eq!(h1, h2, "H plane differs at {cw}x{ch} pad {pad}");
            assert_eq!(v1, v2, "V plane differs at {cw}x{ch} pad {pad}");
            assert_eq!(c1, c2, "C plane differs at {cw}x{ch} pad {pad}");
            // AVX2 fused kernel: byte-identical to the scalar fused builder (and
            // transitively to the tiles) on every geometry, including widths whose
            // vector interior leaves scalar tails.
            #[cfg(accel)]
            {
                let (mut h3, mut v3, mut c3) =
                    (vec![0u8; pw * ph], vec![0u8; pw * ph], vec![0u8; pw * ph]);
                if rusty_h264_accel::hpel_fused(&f, pw, ph, &mut h3, &mut v3, &mut c3) {
                    assert_eq!(h1, h3, "AVX2 H plane differs at {cw}x{ch} pad {pad}");
                    assert_eq!(v1, v3, "AVX2 V plane differs at {cw}x{ch} pad {pad}");
                    assert_eq!(c1, c3, "AVX2 C plane differs at {cw}x{ch} pad {pad}");
                }
            }
        }
    }

    /// The plane cache must be BIT-IDENTICAL to `mc_luma` wherever it applies —
    /// that identity is what keeps the motion search's decisions, and therefore the
    /// bitstream, unchanged. Sweeps every sub-pel phase, every block size, and a
    /// range of positions including ones near the frame edge (where it must decline).
    #[test]
    fn hpel_block_matches_mc_luma_exactly() {
        let (cw, ch) = (96usize, 64usize);
        let mut reference = vec![0u8; cw * ch];
        let mut s: u32 = 0x9E37_79B9;
        for p in reference.iter_mut() {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *p = (s >> 24) as u8;
        }
        let planes = build_hpel_planes(&reference, cw, ch);

        let mut checked = 0;
        for (bw, bh) in [(16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4)] {
            for fy in 0..4i32 {
                for fx in 0..4i32 {
                    if fx == 0 && fy == 0 {
                        continue; // full-pel is the copy path, not the plane path
                    }
                    for y0 in (0..ch - bh).step_by(7) {
                        for x0 in (0..cw - bw).step_by(5) {
                            for &(dx, dy) in &[
                                (0i32, 0i32),
                                (4, 0),
                                (0, 4),
                                (-4, -4),
                                (8, 8),
                                (-64, -64),
                                (96, 40),
                                (-96, 60),
                                (40, -96),
                            ] {
                                let (mvx, mvy) = (dx + fx, dy + fy);
                                let mut want = [0u8; 256];
                                mc_luma(&reference, cw, ch, x0, y0, bw, bh, mvx, mvy, &mut want);
                                let mut got = [0u8; 256];
                                if !hpel_block(&planes, x0, y0, bw, bh, mvx, mvy, &mut got) {
                                    continue; // declined (edge) -> caller falls back
                                }
                                assert_eq!(
                                    &got[..bw * bh],
                                    &want[..bw * bh],
                                    "bw={bw} bh={bh} fx={fx} fy={fy} x0={x0} y0={y0} mv=({mvx},{mvy})"
                                );
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 5_000, "too few positions exercised: {checked}");
    }

    #[test]
    fn mc_luma_block_kernels_match_per_pixel() {
        // The block-kernel MC must be bit-identical to the per-pixel `luma_sample`
        // reference for every quarter-pel position, across interior AND edge blocks
        // (negative / off-frame MVs that exercise the clamped tile vs `at()`).
        let (cw, ch) = (40usize, 32usize);
        let mut state = 0x1357_9bdfu32;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        };
        let reference: Vec<u8> = (0..cw * ch).map(|_| next()).collect();
        for &(bw, bh) in &[(16, 16), (8, 8), (4, 4), (8, 16), (16, 8)] {
            for &(x0, y0) in &[(8usize, 8usize), (0, 0), (cw - bw, ch - bh)] {
                for mvx in -9..=9 {
                    for mvy in -9..=9 {
                        let mut got = vec![0u8; bw * bh];
                        mc_luma(&reference, cw, ch, x0, y0, bw, bh, mvx, mvy, &mut got);
                        let ix0 = x0 as isize + (mvx >> 2) as isize;
                        let iy0 = y0 as isize + (mvy >> 2) as isize;
                        let (fx, fy) = (mvx & 3, mvy & 3);
                        for dy in 0..bh {
                            for dx in 0..bw {
                                let want = luma_sample(
                                    &reference,
                                    cw,
                                    ch,
                                    ix0 + dx as isize,
                                    iy0 + dy as isize,
                                    fx,
                                    fy,
                                ) as u8;
                                assert_eq!(
                                    got[dy * bw + dx],
                                    want,
                                    "bw{bw}x{bh} at ({x0},{y0}) mv({mvx},{mvy}) px({dx},{dy})"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn mc_chroma_block_matches_per_pixel() {
        let (cw, ch) = (24usize, 20usize);
        let mut state = 0xabcd_1234u32;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        };
        let reference: Vec<u8> = (0..cw * ch).map(|_| next()).collect();
        let pp = |ix: isize, iy: isize, fx: i32, fy: i32| -> u8 {
            let a = at(&reference, cw, ch, ix, iy);
            let b = at(&reference, cw, ch, ix + 1, iy);
            let c = at(&reference, cw, ch, ix, iy + 1);
            let d = at(&reference, cw, ch, ix + 1, iy + 1);
            (((8 - fx) * (8 - fy) * a + fx * (8 - fy) * b + (8 - fx) * fy * c + fx * fy * d + 32)
                >> 6) as u8
        };
        for &(bw, bh) in &[(8, 8), (4, 4), (8, 4), (4, 8)] {
            for &(x0, y0) in &[(4usize, 4usize), (0, 0), (cw - bw, ch - bh)] {
                for mvx in -12..=12 {
                    for mvy in -12..=12 {
                        let mut got = vec![0u8; bw * bh];
                        mc_chroma(&reference, cw, ch, x0, y0, bw, bh, mvx, mvy, &mut got);
                        let ix0 = x0 as isize + (mvx >> 3) as isize;
                        let iy0 = y0 as isize + (mvy >> 3) as isize;
                        let (fx, fy) = (mvx & 7, mvy & 7);
                        for dy in 0..bh {
                            for dx in 0..bw {
                                assert_eq!(
                                    got[dy * bw + dx],
                                    pp(ix0 + dx as isize, iy0 + dy as isize, fx, fy),
                                    "bw{bw}x{bh} at ({x0},{y0}) mv({mvx},{mvy}) px({dx},{dy})"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn median_of_three() {
        assert_eq!(median3(1, 2, 3), 2);
        assert_eq!(median3(3, 1, 2), 2);
        assert_eq!(median3(-5, 0, 5), 0);
        assert_eq!(median3(7, 7, 2), 7);
    }

    #[test]
    fn mv_predict_single_neighbor_uses_it() {
        let a = MvNeighbor {
            available: true,
            mv: (8, -4),
            ref_idx: 0,
        };
        // B and C unavailable, A available → predictor is A.
        assert_eq!(
            predict_mv(a, MvNeighbor::NONE, MvNeighbor::NONE, 0),
            (8, -4)
        );
    }

    #[test]
    fn mv_predict_median_when_all_inter() {
        let a = MvNeighbor {
            available: true,
            mv: (4, 0),
            ref_idx: 0,
        };
        let b = MvNeighbor {
            available: true,
            mv: (8, 0),
            ref_idx: 0,
        };
        let c = MvNeighbor {
            available: true,
            mv: (12, 0),
            ref_idx: 0,
        };
        assert_eq!(predict_mv(a, b, c, 0), (8, 0));
    }

    #[test]
    fn mv_predict_one_matching_ref_wins() {
        // Only B references ref 0; A and C are intra → predictor is B.
        let a = MvNeighbor {
            available: true,
            mv: (0, 0),
            ref_idx: -1,
        };
        let b = MvNeighbor {
            available: true,
            mv: (5, 7),
            ref_idx: 0,
        };
        let c = MvNeighbor {
            available: true,
            mv: (0, 0),
            ref_idx: -1,
        };
        assert_eq!(predict_mv(a, b, c, 0), (5, 7));
    }

    #[test]
    fn mv_predict_distinguishes_refs() {
        // A references ref 1, B references ref 0, C intra. For cur_ref 0 only B
        // matches → B; for cur_ref 1 only A matches → A.
        let a = MvNeighbor {
            available: true,
            mv: (4, 4),
            ref_idx: 1,
        };
        let b = MvNeighbor {
            available: true,
            mv: (5, 7),
            ref_idx: 0,
        };
        let c = MvNeighbor {
            available: true,
            mv: (0, 0),
            ref_idx: -1,
        };
        assert_eq!(predict_mv(a, b, c, 0), (5, 7));
        assert_eq!(predict_mv(a, b, c, 1), (4, 4));
    }

    #[test]
    fn mc_luma_zero_mv_copies() {
        let reference = vec![0, 1, 2, 3, 10, 11, 12, 13, 20, 21, 22, 23, 30, 31, 32, 33];
        let mut out = [0u8; 4];
        mc_luma(&reference, 4, 4, 1, 1, 2, 2, 0, 0, &mut out);
        assert_eq!(out, [11, 12, 21, 22]);
    }

    #[test]
    fn mc_luma_clamps_at_edges() {
        let reference = vec![5, 6, 7, 8];
        let mut out = [0u8; 4];
        mc_luma(&reference, 2, 2, 0, 0, 2, 2, -40, -40, &mut out);
        assert_eq!(out, [5, 5, 5, 5]);
    }

    #[test]
    fn mc_luma_halfpel_on_flat_is_flat() {
        // A flat reference must interpolate to the same flat value at any frac.
        let reference = vec![100u8; 8 * 8];
        let mut out = [0u8; 16];
        for &(fx, fy) in &[(2, 0), (0, 2), (2, 2), (1, 1), (3, 3)] {
            mc_luma(&reference, 8, 8, 2, 2, 4, 4, fx, fy, &mut out);
            assert!(out.iter().all(|&p| p == 100), "frac ({fx},{fy})");
        }
    }

    #[test]
    fn mc_chroma_zero_mv_copies() {
        let reference = vec![0, 1, 2, 3, 10, 11, 12, 13, 20, 21, 22, 23, 30, 31, 32, 33];
        let mut out = [0u8; 4];
        mc_chroma(&reference, 4, 4, 1, 1, 2, 2, 0, 0, &mut out);
        assert_eq!(out, [11, 12, 21, 22]);
    }

    #[test]
    fn mc_chroma_bilinear_midpoint() {
        // Horizontal ramp 0,8; chroma frac 4 (half) → midpoint 4.
        let reference = vec![0u8, 8, 0, 8];
        let mut out = [0u8; 1];
        mc_chroma(&reference, 2, 2, 0, 0, 1, 1, 4, 0, &mut out);
        assert_eq!(out[0], 4);
    }
}
