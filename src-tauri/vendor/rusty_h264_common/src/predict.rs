//! Intra prediction + shared block reconstruction for I_16x16 macroblocks.
//!
//! These routines are used identically by the encoder (to reconstruct each
//! macroblock so the next one can predict from it) and the decoder. Keeping
//! them in one place guarantees the two stay bit-for-bit in agreement, which is
//! what intra prediction requires.

use crate::transform::inverse_core;

/// 4×4 luma block scan order → (block-x, block-y) within a macroblock, in units
/// of 4 samples. This is the order luma residual blocks are coded and the order
/// neighbor `nnz` values are indexed by.
pub const LUMA_4X4_SCAN_XY: [(usize, usize); 16] = [
    (0, 0),
    (1, 0),
    (0, 1),
    (1, 1),
    (2, 0),
    (3, 0),
    (2, 1),
    (3, 1),
    (0, 2),
    (1, 2),
    (0, 3),
    (1, 3),
    (2, 2),
    (3, 2),
    (2, 3),
    (3, 3),
];

/// Chroma 4×4 block scan order → (block-x, block-y) within the 8×8, in units of
/// 4 samples (simple raster for 4:2:0).
pub const CHROMA_4X4_SCAN_XY: [(usize, usize); 4] = [(0, 0), (1, 0), (0, 1), (1, 1)];

/// Maps a luma QP to its chroma QP (with `chroma_qp_index_offset == 0`).
/// MEASUREMENT KNOBS — ablation switches for pricing stages the scope profiler cannot
/// read (its per-call rdtsc tax at ~20M calls exceeds the thing being measured).
///
/// `RFF_ABL_INTRA=1` makes every intra predictor return a flat block; `RFF_ABL_RECON=1`
/// makes reconstruction return the prediction unchanged (residual discarded). Both leave
/// PARSING and every call count untouched — what is priced is the pixel work alone, so
/// the frame count is unaffected and the work-parity check still holds. Output pixels
/// are wrong while either is set, by design; these are never a shipping path.
/// Read once; the branch predicts perfectly.
#[inline]
pub(crate) fn abl_intra() -> bool {
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
                let on = crate::knob("RFF_ABL_INTRA").is_some_and(|v| v != "0");
                ON.store(if on { 1 } else { 2 }, Ordering::Relaxed);
                on
            }
        }
    }
}

#[inline]
pub(crate) fn abl_recon() -> bool {
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
                let on = crate::knob("RFF_ABL_RECON").is_some_and(|v| v != "0");
                ON.store(if on { 1 } else { 2 }, Ordering::Relaxed);
                on
            }
        }
    }
}

pub fn chroma_qp(qp: u8) -> u8 {
    const QPC: [u8; 22] = [
        29, 30, 31, 32, 32, 33, 34, 34, 35, 35, 36, 36, 37, 37, 37, 38, 38, 38, 39, 39, 39, 39,
    ];
    if qp < 30 {
        qp
    } else {
        // QP is 0..=51 by construction on both codec sides (encoder clamps,
        // decoder wraps `rem_euclid(52)`), so `qp - 30 <= 21` always — but the
        // TYPE admits 255 and LLVM cannot see the construction, so this
        // per-macroblock helper carried a panic path. `.min(21)` retires it
        // exactly (the clamp is inert for every reachable input) — the same
        // move as the quantizer's `.max(1)` in the panic round.
        QPC[((qp - 30) as usize).min(21)]
    }
}

/// CAVLC `nC` context from the left (`a`) and top (`b`) neighbor block `nnz`
/// counts (`None` = neighbor unavailable).
pub fn nc_from_neighbors(a: Option<u8>, b: Option<u8>) -> i32 {
    match (a, b) {
        (Some(na), Some(nb)) => (na as i32 + nb as i32 + 1) >> 1,
        (Some(na), None) => na as i32,
        (None, Some(nb)) => nb as i32,
        (None, None) => 0,
    }
}

fn sum(s: &[u8]) -> i32 {
    s.iter().map(|&x| x as i32).sum()
}

/// Intra_16x16 DC luma prediction: a single value used for all 256 samples.
pub fn luma16x16_dc(avail_top: bool, avail_left: bool, top: &[u8; 16], left: &[u8; 16]) -> u8 {
    let v = if avail_top && avail_left {
        (sum(top) + sum(left) + 16) >> 5
    } else if avail_top {
        (sum(top) + 8) >> 4
    } else if avail_left {
        (sum(left) + 8) >> 4
    } else {
        128
    };
    v as u8
}

/// The four `Intra_16x16` prediction modes (spec §8.3.3). The discriminant is
/// the `intra16x16_pred_mode` value carried in `mb_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I16Mode {
    Vertical = 0,
    Horizontal = 1,
    Dc = 2,
    Plane = 3,
}

impl I16Mode {
    /// Parses a mode number (0..=3).
    pub fn from_id(v: u32) -> Self {
        match v & 3 {
            0 => I16Mode::Vertical,
            1 => I16Mode::Horizontal,
            2 => I16Mode::Dc,
            _ => I16Mode::Plane,
        }
    }

    /// Whether this mode is usable given neighbor availability.
    pub fn available(self, avail_top: bool, avail_left: bool) -> bool {
        match self {
            I16Mode::Vertical => avail_top,
            I16Mode::Horizontal => avail_left,
            I16Mode::Dc => true,
            I16Mode::Plane => avail_top && avail_left,
        }
    }
}

/// Computes the 16×16 luma prediction for an `Intra_16x16` macroblock, returning
/// 256 raster samples. `corner` is the above-left neighbor (used by Plane).
pub fn luma16x16_pred(
    mode: I16Mode,
    avail_top: bool,
    avail_left: bool,
    top: &[u8; 16],
    left: &[u8; 16],
    corner: u8,
) -> [u8; 256] {
    if abl_intra() {
        return [128; 256];
    }
    // REFUTED, do not retry: rewriting the per-pixel stores in these predictors
    // as `chunks_exact_mut` + `copy_from_slice`/`fill` measured NET ZERO across
    // the five predictors (intra4x4 -3, intra8x8 -8, luma16x16 0, chroma8x8_dc 0,
    // and chroma8x8_pred +11 = +4.5% WORSE). LLVM already recognises
    // `out[y * W + x] = top[x]` as a row copy and `= left[y]` as a fill, and
    // emits the same memcpy/memset either way. Same class as `sqrt`/`min`/`max`
    // in the fast-transcendentals targeting rule: do not hand-write what the
    // compiler already does.
    let _g = crate::prof::scope(crate::prof::Stage::IntraPred);
    let mut out = [0u8; 256];
    match mode {
        // Row copy / row fill (the shape the accel twin uses). NOTE: the accel
        // `i16x16_luma_pred` is plane-addressed and reads the top row from the
        // reconstruction plane; this decoder feeds the top row from `bak_y` (the
        // UNFILTERED copy) once the row above has been deblocked, so that kernel
        // cannot be wired here -- this function is the decoder's vector form.
        I16Mode::Vertical => {
            for row in out.chunks_exact_mut(16) {
                row.copy_from_slice(top);
            }
        }
        I16Mode::Horizontal => {
            for (row, &l) in out.chunks_exact_mut(16).zip(left.iter()) {
                row.fill(l);
            }
        }
        I16Mode::Dc => {
            let v = luma16x16_dc(avail_top, avail_left, top, left);
            out.fill(v);
        }
        I16Mode::Plane => {
            // p[x,-1] = top[x] (x 0..15), p[-1,y] = left[y], p[-1,-1] = corner.
            let tp = |i: isize| -> i32 {
                if i < 0 {
                    corner as i32
                } else {
                    top[i as usize] as i32
                }
            };
            let lf = |i: isize| -> i32 {
                if i < 0 {
                    corner as i32
                } else {
                    left[i as usize] as i32
                }
            };
            let mut h = 0i32;
            let mut v = 0i32;
            for xp in 0..8i32 {
                h += (xp + 1) * (tp(8 + xp as isize) - tp(6 - xp as isize));
            }
            for yp in 0..8i32 {
                v += (yp + 1) * (lf(8 + yp as isize) - lf(6 - yp as isize));
            }
            let a = 16 * (left[15] as i32 + top[15] as i32);
            let b = (5 * h + 32) >> 6;
            let c = (5 * v + 32) >> 6;
            for y in 0..16i32 {
                for x in 0..16i32 {
                    let val = (a + b * (x - 7) + c * (y - 7) + 16) >> 5;
                    out[(y * 16 + x) as usize] = clip_u8(val);
                }
            }
        }
    }
    out
}

/// Intra 4×4 luma prediction (spec §8.3.1.2), all 9 modes. Returns 16 raster
/// samples. Neighbors: `top[0..4]` above, `top[4..8]` above-right (the caller
/// substitutes `top[3]` when the above-right block is unavailable), `left[0..4]`
/// to the left, `corner` above-left. `avail_top`/`avail_left` gate DC only;
/// callers must not select a mode whose neighbors are unavailable.
pub fn intra4x4_pred(
    mode: u8,
    avail_top: bool,
    avail_left: bool,
    top: &[u8; 8],
    left: &[u8; 4],
    corner: u8,
) -> [u8; 16] {
    if abl_intra() {
        return [128; 16];
    }
    let _g = crate::prof::scope(crate::prof::Stage::IntraPred);
    // ONE EDGE ARRAY, TWO FILTERED ARRAYS, SIXTEEN PICKS (SIMD census
    // 2026-09-05, finding #7). The six directional modes are all `(a + b + 1) >> 1`
    // or `(a + 2b + c + 2) >> 2` over the neighbour run
    //   e = [l3, l2, l1, l0, corner, t0, t1, ..., t7]
    // so the whole mode is: filter the run ONCE (straight-line, 12/11 lanes,
    // vectorisable) and gather 16 values by a fixed index. The per-pixel form it
    // replaces recomputed the filter at every sample behind a data-dependent
    // branch, in i32, then clipped -- and the clip was a no-op (averages of u8
    // stay in 0..=255). Every formula below is the spec (8.3.1.2) one rewritten
    // in `e`-indices; the corpus gate is the oracle for the rewrite.
    let mut e = [0i32; 13];
    for k in 0..4 {
        e[3 - k] = left[k] as i32;
    }
    e[4] = corner as i32;
    for k in 0..8 {
        e[5 + k] = top[k] as i32;
    }
    // f2[i] = avg(e[i], e[i+1]);  f3[i] = (e[i] + 2 e[i+1] + e[i+2] + 2) >> 2
    let mut f2 = [0i32; 16];
    let mut f3 = [0i32; 16];
    for i in 0..12 {
        f2[i] = (e[i] + e[i + 1] + 1) >> 1;
    }
    for i in 0..11 {
        f3[i] = (e[i] + 2 * e[i + 1] + e[i + 2] + 2) >> 2;
    }
    let mut out = [0u8; 16];
    match mode {
        0 => {
            for row in out.chunks_exact_mut(4) {
                row.copy_from_slice(&top[..4]);
            }
        }
        1 => {
            for (row, &l) in out.chunks_exact_mut(4).zip(left.iter()) {
                row.fill(l);
            }
        }
        2 => {
            let (ts, ls) = (e[5] + e[6] + e[7] + e[8], e[0] + e[1] + e[2] + e[3]);
            let v = if avail_top && avail_left {
                (ts + ls + 4) >> 3
            } else if avail_top {
                (ts + 2) >> 2
            } else if avail_left {
                (ls + 2) >> 2
            } else {
                128
            };
            out.fill(v as u8);
        }
        3 => {
            // Diagonal down-left: f3 over the top run at k = x + y; (3,3) is
            // (t6 + 3 t7 + 2) >> 2.
            for y in 0..4 {
                for x in 0..4 {
                    out[y * 4 + x] = if x == 3 && y == 3 {
                        ((e[11] + 3 * e[12] + 2) >> 2) as u8
                    } else {
                        f3[(5 + x + y) & 15] as u8
                    };
                }
            }
        }
        4 => {
            // Diagonal down-right: f3 centred on the corner, index 3 + x - y.
            for y in 0..4 {
                for x in 0..4 {
                    out[y * 4 + x] = f3[(3 + x + 16 - y) & 15] as u8;
                }
            }
        }
        5 => {
            // Vertical-right.
            for y in 0..4i32 {
                for x in 0..4i32 {
                    let zvr = 2 * x - y;
                    let k = x - (y >> 1);
                    let v = if zvr >= 0 && zvr % 2 == 0 {
                        f2[(4 + k) as usize & 15]
                    } else if zvr >= 0 {
                        f3[(3 + k) as usize & 15]
                    } else if zvr == -1 {
                        f3[3]
                    } else {
                        f3[(4 - y) as usize & 15]
                    };
                    out[(y * 4 + x) as usize] = v as u8;
                }
            }
        }
        6 => {
            // Horizontal-down.
            for y in 0..4i32 {
                for x in 0..4i32 {
                    let zhd = 2 * y - x;
                    let k = y - (x >> 1);
                    let v = if zhd >= 0 && zhd % 2 == 0 {
                        f2[(3 - k) as usize & 15]
                    } else if zhd >= 0 {
                        f3[(3 - k) as usize & 15]
                    } else if zhd == -1 {
                        f3[3]
                    } else {
                        f3[(2 + x) as usize & 15]
                    };
                    out[(y * 4 + x) as usize] = v as u8;
                }
            }
        }
        7 => {
            // Vertical-left.
            for y in 0..4 {
                for x in 0..4 {
                    let k = x + (y >> 1);
                    out[y * 4 + x] = if y % 2 == 0 {
                        f2[(5 + k) & 15]
                    } else {
                        f3[(5 + k) & 15]
                    } as u8;
                }
            }
        }
        _ => {
            // Horizontal-up.
            for y in 0..4 {
                for x in 0..4 {
                    let zhu = x + 2 * y;
                    let k = y + (x >> 1);
                    let v = if zhu <= 4 && zhu % 2 == 0 {
                        f2[(2 + 16 - k) & 15]
                    } else if zhu < 5 {
                        f3[(1 + 16 - k) & 15]
                    } else if zhu == 5 {
                        (e[1] + 3 * e[0] + 2) >> 2
                    } else {
                        e[0]
                    };
                    out[y * 4 + x] = v as u8;
                }
            }
        }
    }
    out
}

/// Intra chroma 8×8 DC prediction (spec §8.3.4): the 8×8 is split into four 4×4
/// regions, each predicted from its available neighbor run. Returns the 64
/// predicted samples in raster order. `top`/`left` are the 8 neighbor samples.
pub fn chroma8x8_dc(avail_top: bool, avail_left: bool, top: &[u8; 8], left: &[u8; 8]) -> [u8; 64] {
    let mut out = [128u8; 64];
    for &(bx, by) in &CHROMA_4X4_SCAN_XY {
        let (x0, y0) = (bx * 4, by * 4);
        let top4 = &top[x0..x0 + 4];
        let left4 = &left[y0..y0 + 4];
        // Which neighbor each region prefers (spec §8.3.4.1).
        let prefer_top = (x0, y0) == (4, 0);
        let prefer_left = (x0, y0) == (0, 4);
        let dc: i32 = if prefer_top {
            if avail_top {
                (sum(top4) + 2) >> 2
            } else if avail_left {
                (sum(left4) + 2) >> 2
            } else {
                128
            }
        } else if prefer_left {
            if avail_left {
                (sum(left4) + 2) >> 2
            } else if avail_top {
                (sum(top4) + 2) >> 2
            } else {
                128
            }
        } else {
            // (0,0) and (4,4): use both when available.
            if avail_top && avail_left {
                (sum(top4) + sum(left4) + 4) >> 3
            } else if avail_top {
                (sum(top4) + 2) >> 2
            } else if avail_left {
                (sum(left4) + 2) >> 2
            } else {
                128
            }
        };
        for dy in 0..4 {
            for dx in 0..4 {
                out[(y0 + dy) * 8 + (x0 + dx)] = dc as u8;
            }
        }
    }
    out
}

/// Intra chroma 8×8 prediction (spec §8.3.4), all four modes. `mode`:
/// 0 = DC, 1 = Horizontal, 2 = Vertical, 3 = Plane. `top`/`left` are the 8
/// neighbor samples; `corner` is the above-left (Plane only).
pub fn chroma8x8_pred(
    mode: u8,
    avail_top: bool,
    avail_left: bool,
    top: &[u8; 8],
    left: &[u8; 8],
    corner: u8,
) -> [u8; 64] {
    if abl_intra() {
        return [128; 64];
    }
    let _g = crate::prof::scope(crate::prof::Stage::IntraPred);
    match mode {
        1 => {
            // Horizontal
            let mut out = [0u8; 64];
            for y in 0..8 {
                for x in 0..8 {
                    out[y * 8 + x] = left[y];
                }
            }
            out
        }
        2 => {
            // Vertical
            let mut out = [0u8; 64];
            for y in 0..8 {
                for x in 0..8 {
                    out[y * 8 + x] = top[x];
                }
            }
            out
        }
        3 => {
            // Plane
            let tt = |k: i32| {
                if k < 0 {
                    corner as i32
                } else {
                    top[k as usize] as i32
                }
            };
            let ll = |k: i32| {
                if k < 0 {
                    corner as i32
                } else {
                    left[k as usize] as i32
                }
            };
            let mut h = 0i32;
            let mut v = 0i32;
            for xp in 0..4i32 {
                h += (xp + 1) * (top[(4 + xp) as usize] as i32 - tt(2 - xp));
            }
            for yp in 0..4i32 {
                v += (yp + 1) * (left[(4 + yp) as usize] as i32 - ll(2 - yp));
            }
            let a = 16 * (left[7] as i32 + top[7] as i32);
            let b = (17 * h + 16) >> 5;
            let c = (17 * v + 16) >> 5;
            let mut out = [0u8; 64];
            for y in 0..8i32 {
                for x in 0..8i32 {
                    out[(y * 8 + x) as usize] = clip_u8((a + b * (x - 3) + c * (y - 3) + 16) >> 5);
                }
            }
            out
        }
        _ => chroma8x8_dc(avail_top, avail_left, top, left),
    }
}

/// Whether a chroma prediction mode is usable given neighbor availability.
pub fn chroma_mode_available(mode: u8, avail_top: bool, avail_left: bool) -> bool {
    match mode {
        0 => true,                    // DC
        1 => avail_left,              // Horizontal
        2 => avail_top,               // Vertical
        _ => avail_top && avail_left, // Plane
    }
}

/// Clips a value to the 8-bit pixel range.
#[inline]
pub fn clip_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// Reconstructs a 4×4 block: inverse-transform the dequantized coefficients and
/// add the prediction, clipping to 8-bit. `dequant` and `pred` are raster 4×4.
pub fn reconstruct_4x4(dequant: &[i32; 16], pred: &[i32; 16]) -> [u8; 16] {
    if abl_recon() {
        return core::array::from_fn(|i| clip_u8(pred[i]));
    }
    let _g = crate::prof::scope(crate::prof::Stage::Reconstruct);
    add_residual_4x4(&inverse_core(dequant), pred)
}

/// Reconstructs a 4×4 block DIRECTLY into the frame plane: IDCT + add + clip +
/// store in one pass, with the prediction read strided from its buffer. Kills
/// the per-block `predb` i32 gather, the `[u8; 16]` result array, and the
/// separate `store` call — the resid-add boundary the WHYS Part 8/9 descent
/// priced. Honors the same ablation knob as `reconstruct_4x4`.
#[allow(unreachable_code)]
#[inline]
pub fn reconstruct_4x4_into(
    deq: &[i32; 16],
    pred: &[u8],
    p_off: usize,
    p_stride: usize,
    rec: &mut [u8],
    r_off: usize,
    r_stride: usize,
) {
    if abl_recon() {
        for r in 0..4 {
            let src = &pred[p_off + r * p_stride..][..4];
            rec[r_off + r * r_stride..][..4].copy_from_slice(src);
        }
        return;
    }
    let _g = crate::prof::scope(crate::prof::Stage::Reconstruct);
    // SIMD twin (SIMD census 2026-09-05, kernel round): in-register 4x4 IDCT + add,
    // exact on i32 input; the scalar body below is the oracle and the non-accel arm.
    #[cfg(accel)]
    {
        rusty_h264_accel::idct4x4_add(deq, pred, p_off, p_stride, rec, r_off, r_stride);
        return;
    }
    let res = crate::transform::inverse_core(deq);
    // ROW SLICES, as in `reconstruct_4x4_dc_into` — and the residual row too, so
    // `res[r * 4 + c]` stops being a computed index into a 16-entry array.
    for r in 0..4 {
        let src = &pred[p_off + r * p_stride..][..4];
        let dst = &mut rec[r_off + r * r_stride..][..4];
        let row = &res[r * 4..][..4];
        for c in 0..4 {
            dst[c] = clip_u8(src[c] as i32 + row[c]);
        }
    }
}

/// FUSED scan-order reconstruct (dense-over-scatter round, 2026-09-05):
/// `rec = clip(pred + idct(dequant(unscan(scan))))` in ONE kernel call. `q` is the
/// per-qp dequant constant (`DQ_FLAT[qp]` or `DequantQp::weighted`); `AC` = the
/// 15-coefficient AC form whose position 0 is the caller's already-dequantised
/// `dc` (I16 luma AC, chroma AC). Retires the per-block `un_scan_4x4_*` and
/// `dequantize` passes. The scalar body below is the oracle and the non-accel arm.
#[allow(clippy::too_many_arguments)]
#[allow(unreachable_code)]
#[inline]
pub fn reconstruct_4x4_scan_into<const AC: bool>(
    scan: &[i32; 16],
    q: &crate::transform::DequantQp,
    dc: i32,
    pred: &[u8],
    p_off: usize,
    p_stride: usize,
    rec: &mut [u8],
    r_off: usize,
    r_stride: usize,
) {
    if abl_recon() {
        for r in 0..4 {
            let src = &pred[p_off + r * p_stride..][..4];
            rec[r_off + r * r_stride..][..4].copy_from_slice(src);
        }
        return;
    }
    let _g = crate::prof::scope(crate::prof::Stage::Reconstruct);
    #[cfg(accel)]
    {
        rusty_h264_accel::idct4x4_deq_add::<AC>(
            scan, &q.ls, q.add, q.sr, dc, pred, p_off, p_stride, rec, r_off, r_stride,
        );
        return;
    }
    let raster = if AC {
        let mut d = [0i32; 16];
        crate::cavlc::un_scan_4x4_ac_into(&scan[..15], &mut d);
        d
    } else {
        crate::cavlc::un_scan_4x4_dcac(scan)
    };
    let mut deq = q.apply(&raster);
    if AC {
        deq[0] = dc;
    }
    reconstruct_4x4_into(&deq, pred, p_off, p_stride, rec, r_off, r_stride);
}

/// Flat-residual twin of [`reconstruct_4x4_into`] (the DC-only fast path).
#[allow(unreachable_code)]
#[inline]
pub fn reconstruct_4x4_dc_into(
    rval: i32,
    pred: &[u8],
    p_off: usize,
    p_stride: usize,
    rec: &mut [u8],
    r_off: usize,
    r_stride: usize,
) {
    if abl_recon() {
        for r in 0..4 {
            let src = &pred[p_off + r * p_stride..][..4];
            rec[r_off + r * r_stride..][..4].copy_from_slice(src);
        }
        return;
    }
    let _g = crate::prof::scope(crate::prof::Stage::Reconstruct);
    #[cfg(accel)]
    {
        rusty_h264_accel::flat_add_4x4(rval, pred, p_off, p_stride, rec, r_off, r_stride);
        return;
    }
    // ROW SLICES. Sixteen reads and sixteen writes, each separately bounds
    // checked against a whole plane, for a 4x4 block whose rows are contiguous:
    // four slice pairs make every `[c]` provable from `c < 4`.
    for r in 0..4 {
        let src = &pred[p_off + r * p_stride..][..4];
        let dst = &mut rec[r_off + r * r_stride..][..4];
        for c in 0..4 {
            dst[c] = clip_u8(src[c] as i32 + rval);
        }
    }
}

/// Add+clip tail of [`reconstruct_4x4_into`] for a residual that was ALREADY
/// inverse-transformed: the per-block half of a BATCHED IDCT. The decoder's
/// recon drivers collect the dequantised blocks of a macroblock, run
/// [`crate::transform::inverse_dct_blocks`] once (8 blocks per `i32x8` pass,
/// bit-identical to [`crate::transform::inverse_core`] per block) and finish
/// each block here. Before this the 4x4 IDCT ran as scalar butterflies inside
/// `reconstruct_4x4_into`, 2.4-3.2M times per 60 frames, while the batched
/// SIMD form was reachable only from the ENCODER.
#[inline]
pub fn add_residual_4x4_into(
    res: &[i32; 16],
    pred: &[u8],
    p_off: usize,
    p_stride: usize,
    rec: &mut [u8],
    r_off: usize,
    r_stride: usize,
) {
    if abl_recon() {
        for r in 0..4 {
            let src = &pred[p_off + r * p_stride..][..4];
            rec[r_off + r * r_stride..][..4].copy_from_slice(src);
        }
        return;
    }
    let _g = crate::prof::scope(crate::prof::Stage::Reconstruct);
    for r in 0..4 {
        let src = &pred[p_off + r * p_stride..][..4];
        let dst = &mut rec[r_off + r * r_stride..][..4];
        let row = &res[r * 4..][..4];
        for c in 0..4 {
            dst[c] = clip_u8(src[c] as i32 + row[c]);
        }
    }
}

/// Adds an already-inverse-transformed residual block to its prediction and clips
/// to `u8`. Split out of [`reconstruct_4x4`] so callers that batch the inverse DCT
/// ([`crate::transform::inverse_dct_blocks`]) can share the add+clip tail.
#[inline]
pub fn add_residual_4x4(res: &[i32; 16], pred: &[i32; 16]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for i in 0..16 {
        out[i] = clip_u8(pred[i] + res[i]);
    }
    out
}

/// Intra_8x8 luma prediction (spec §8.3.2.2), all nine modes. Reference samples
/// are first low-pass filtered (§8.3.2.2.1) then the per-mode formulas — which
/// mirror the 4×4 ones over the 8-wide block — are applied. `top` holds the 16
/// samples p[0..15,-1] (8..15 substituted by the caller when no top-right),
/// `left` the 8 samples p[-1,0..7], `corner` is p[-1,-1]; `avail_corner` is the
/// above-left neighbor's availability. Returns 64 predicted samples (raster).
#[allow(clippy::needless_range_loop)] // filter loops read neighbors p[k-1..k+1]
pub fn intra8x8_pred(
    mode: u8,
    avail_top: bool,
    avail_left: bool,
    avail_corner: bool,
    top: &[u8; 16],
    left: &[u8; 8],
    corner: u8,
) -> [u8; 64] {
    let _g = crate::prof::scope(crate::prof::Stage::IntraPred);
    // ---- reference sample filtering (§8.3.2.2.1) ----
    let t = |k: usize| top[k] as i32;
    let l = |k: usize| left[k] as i32;
    let cc = corner as i32;
    let mut ft = [0i32; 16];
    ft[0] = if avail_corner {
        (cc + 2 * t(0) + t(1) + 2) >> 2
    } else {
        (3 * t(0) + t(1) + 2) >> 2
    };
    for k in 1..15 {
        ft[k] = (t(k - 1) + 2 * t(k) + t(k + 1) + 2) >> 2;
    }
    ft[15] = (t(14) + 3 * t(15) + 2) >> 2;
    let mut fl = [0i32; 8];
    fl[0] = if avail_corner {
        (cc + 2 * l(0) + l(1) + 2) >> 2
    } else {
        (3 * l(0) + l(1) + 2) >> 2
    };
    for k in 1..7 {
        fl[k] = (l(k - 1) + 2 * l(k) + l(k + 1) + 2) >> 2;
    }
    fl[7] = (l(6) + 3 * l(7) + 2) >> 2;
    let fc = if avail_corner {
        if avail_top && avail_left {
            (t(0) + 2 * cc + l(0) + 2) >> 2
        } else if avail_top {
            (3 * cc + t(0) + 2) >> 2
        } else if avail_left {
            (3 * cc + l(0) + 2) >> 2
        } else {
            cc
        }
    } else {
        cc
    };
    // Indexers with -1 → filtered corner (for the diagonal modes).
    let ttf = |k: i32| -> i32 {
        if k < 0 {
            fc
        } else {
            ft[k as usize]
        }
    };
    let llf = |k: i32| -> i32 {
        if k < 0 {
            fc
        } else {
            fl[k as usize]
        }
    };

    let mut p = [0i32; 64];
    match mode {
        0 => {
            for y in 0..8 {
                for x in 0..8 {
                    p[y * 8 + x] = ft[x];
                }
            }
        }
        1 => {
            for y in 0..8 {
                for x in 0..8 {
                    p[y * 8 + x] = fl[y];
                }
            }
        }
        2 => {
            let st: i32 = ft[0..8].iter().sum();
            let sl: i32 = fl[0..8].iter().sum();
            let v = if avail_top && avail_left {
                (st + sl + 8) >> 4
            } else if avail_top {
                (st + 4) >> 3
            } else if avail_left {
                (sl + 4) >> 3
            } else {
                128
            };
            p.fill(v);
        }
        3 => {
            for y in 0..8 {
                for x in 0..8 {
                    p[y * 8 + x] = if x == 7 && y == 7 {
                        (ft[14] + 3 * ft[15] + 2) >> 2
                    } else {
                        (ft[x + y] + 2 * ft[x + y + 1] + ft[x + y + 2] + 2) >> 2
                    };
                }
            }
        }
        4 => {
            for y in 0..8i32 {
                for x in 0..8i32 {
                    p[(y * 8 + x) as usize] = if x > y {
                        (ttf(x - y - 2) + 2 * ttf(x - y - 1) + ttf(x - y) + 2) >> 2
                    } else if x < y {
                        (llf(y - x - 2) + 2 * llf(y - x - 1) + llf(y - x) + 2) >> 2
                    } else {
                        (ft[0] + 2 * fc + fl[0] + 2) >> 2
                    };
                }
            }
        }
        5 => {
            for y in 0..8i32 {
                for x in 0..8i32 {
                    let zvr = 2 * x - y;
                    let k = x - (y >> 1);
                    p[(y * 8 + x) as usize] = if zvr >= 0 && zvr % 2 == 0 {
                        (ttf(k - 1) + ttf(k) + 1) >> 1
                    } else if zvr >= 0 {
                        (ttf(k - 2) + 2 * ttf(k - 1) + ttf(k) + 2) >> 2
                    } else if zvr == -1 {
                        (fl[0] + 2 * fc + ft[0] + 2) >> 2
                    } else {
                        let j = y - 2 * x;
                        (llf(j - 1) + 2 * llf(j - 2) + llf(j - 3) + 2) >> 2
                    };
                }
            }
        }
        6 => {
            for y in 0..8i32 {
                for x in 0..8i32 {
                    let zhd = 2 * y - x;
                    let k = y - (x >> 1);
                    p[(y * 8 + x) as usize] = if zhd >= 0 && zhd % 2 == 0 {
                        (llf(k - 1) + llf(k) + 1) >> 1
                    } else if zhd >= 0 {
                        (llf(k - 2) + 2 * llf(k - 1) + llf(k) + 2) >> 2
                    } else if zhd == -1 {
                        (fl[0] + 2 * fc + ft[0] + 2) >> 2
                    } else {
                        let j = x - 2 * y;
                        (ttf(j - 1) + 2 * ttf(j - 2) + ttf(j - 3) + 2) >> 2
                    };
                }
            }
        }
        7 => {
            for y in 0..8 {
                for x in 0..8 {
                    let k = x + (y >> 1);
                    p[y * 8 + x] = if y % 2 == 0 {
                        (ft[k] + ft[k + 1] + 1) >> 1
                    } else {
                        (ft[k] + 2 * ft[k + 1] + ft[k + 2] + 2) >> 2
                    };
                }
            }
        }
        _ => {
            // Horizontal-up (mode 8) — 8×8 thresholds (zHU up to 13 special).
            for y in 0..8 {
                for x in 0..8 {
                    let zhu = x + 2 * y;
                    let k = y + (x >> 1);
                    p[y * 8 + x] = if zhu < 13 && zhu % 2 == 0 {
                        (fl[k] + fl[k + 1] + 1) >> 1
                    } else if zhu < 13 {
                        (fl[k] + 2 * fl[k + 1] + fl[k + 2] + 2) >> 2
                    } else if zhu == 13 {
                        (fl[6] + 3 * fl[7] + 2) >> 2
                    } else {
                        fl[7]
                    };
                }
            }
        }
    }
    let mut out = [0u8; 64];
    for i in 0..64 {
        out[i] = clip_u8(p[i]);
    }
    out
}

/// Reconstructs an 8×8 block: inverse-transform the dequantized coefficients,
/// add the prediction, clip. (The 8×8 inverse transform lives in `transform`.)
pub fn add_residual_8x8(res: &[i32; 64], pred: &[i32; 64]) -> [u8; 64] {
    let _g = crate::prof::scope(crate::prof::Stage::Reconstruct);
    let mut out = [0u8; 64];
    for i in 0..64 {
        out[i] = clip_u8(pred[i] + res[i]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intra8x8_dc_no_neighbors_is_128() {
        let p = intra8x8_pred(2, false, false, false, &[0; 16], &[0; 8], 0);
        assert!(p.iter().all(|&v| v == 128));
    }

    #[test]
    fn intra8x8_vertical_copies_filtered_top() {
        // Flat top → filtered top is the same flat value → every column equals it.
        let p = intra8x8_pred(0, true, false, false, &[90; 16], &[0; 8], 0);
        assert!(p.iter().all(|&v| v == 90));
    }

    #[test]
    fn intra8x8_horizontal_is_constant_per_row() {
        let mut left = [0u8; 8];
        for (i, v) in left.iter_mut().enumerate() {
            *v = (10 * i + 20) as u8;
        }
        let p = intra8x8_pred(1, false, true, false, &[0; 16], &left, 0);
        // Each row constant; filtering blurs values but row 0..7 stay monotone-ish.
        for y in 0..8 {
            for x in 1..8 {
                assert_eq!(p[y * 8 + x], p[y * 8], "row {y} not constant");
            }
        }
    }

    #[test]
    fn luma_dc_no_neighbors_is_128() {
        assert_eq!(luma16x16_dc(false, false, &[0; 16], &[0; 16]), 128);
    }

    #[test]
    fn luma_dc_averages_neighbors() {
        let top = [100u8; 16];
        let left = [200u8; 16];
        // (1600 + 3200 + 16) >> 5 = 150
        assert_eq!(luma16x16_dc(true, true, &top, &left), 150);
        assert_eq!(luma16x16_dc(true, false, &top, &left), 100);
        assert_eq!(luma16x16_dc(false, true, &top, &left), 200);
    }

    #[test]
    fn chroma_dc_unavailable_is_128() {
        let p = chroma8x8_dc(false, false, &[0; 8], &[0; 8]);
        assert!(p.iter().all(|&x| x == 128));
    }

    #[test]
    fn chroma_qp_mapping() {
        assert_eq!(chroma_qp(20), 20);
        assert_eq!(chroma_qp(30), 29);
        assert_eq!(chroma_qp(51), 39);
    }

    #[test]
    fn nc_derivation() {
        assert_eq!(nc_from_neighbors(Some(3), Some(4)), 4);
        assert_eq!(nc_from_neighbors(Some(5), None), 5);
        assert_eq!(nc_from_neighbors(None, None), 0);
    }

    #[test]
    fn intra4x4_vertical_copies_top() {
        let top = [10, 20, 30, 40, 0, 0, 0, 0];
        let p = intra4x4_pred(0, true, false, &top, &[0; 4], 0);
        for y in 0..4 {
            assert_eq!(&p[y * 4..y * 4 + 4], &[10, 20, 30, 40]);
        }
    }

    #[test]
    fn intra4x4_horizontal_copies_left() {
        let left = [11, 22, 33, 44];
        let p = intra4x4_pred(1, false, true, &[0; 8], &left, 0);
        for y in 0..4 {
            assert!(p[y * 4..y * 4 + 4].iter().all(|&v| v == left[y]));
        }
    }

    #[test]
    fn intra4x4_dc_averages() {
        let top = [10, 10, 10, 10, 0, 0, 0, 0];
        let left = [30, 30, 30, 30];
        // (40 + 120 + 4) >> 3 = 20
        let p = intra4x4_pred(2, true, true, &top, &left, 0);
        assert!(p.iter().all(|&v| v == 20));
        // top only: (40 + 2) >> 2 = 10
        let p2 = intra4x4_pred(2, true, false, &top, &left, 0);
        assert!(p2.iter().all(|&v| v == 10));
        // neither: 128
        let p3 = intra4x4_pred(2, false, false, &top, &left, 0);
        assert!(p3.iter().all(|&v| v == 128));
    }
}

/// z-order -> raster permutation of a macroblock's 24 nnz bytes (16 luma + 8
/// chroma). SIMD lane shuffles under `accel`; the scalar form is the oracle.
#[inline]
pub fn nnz_raster_from_z(n: &[u8; 24]) -> [u8; 24] {
    #[cfg(accel)]
    {
        return rusty_h264_accel::nnz_raster_from_z(n);
    }
    #[allow(unreachable_code)]
    [
        n[0], n[1], n[4], n[5], n[2], n[3], n[6], n[7], n[8], n[9], n[12], n[13], n[10], n[11],
        n[14], n[15], n[16], n[17], n[20], n[21], n[18], n[19], n[22], n[23],
    ]
}
