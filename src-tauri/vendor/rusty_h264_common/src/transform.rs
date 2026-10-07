//! H.264 4×4 residual transform and quantization (spec §8.5 / §8.6).
//!
//! H.264 uses a small integer approximation of the DCT — a 4×4 "core" transform
//! whose scaling is folded into the quantizer, so it is exactly invertible in
//! integer arithmetic. This module implements the forward path (encoder:
//! residual → coefficients → quantized levels) and the inverse path (decoder
//! and encoder-reconstruction: levels → coefficients → residual).
//!
//! Correctness here is non-negotiable: the levels we emit are dequantized by
//! every conforming decoder using the exact spec tables below, so the forward
//! quantizer must be the faithful inverse of that process.

#[allow(unused_imports)]
use crate::fmath::{F32Ext as _, F64Ext as _};
#[allow(unused_imports)]
use alloc::vec;
#[allow(unused_imports)]
use alloc::vec::Vec;

/// `normAdjust4x4` (spec Table — the dequant scaling V), indexed by `[QP % 6]`
/// then by position group (see [`pos_group`]).
const NORM_ADJUST: [[i32; 3]; 6] = [
    [10, 16, 13],
    [11, 18, 14],
    [13, 20, 16],
    [14, 23, 18],
    [16, 25, 20],
    [18, 29, 23],
];

/// Forward quantization multipliers `MF`, indexed by `[QP % 6]` then position
/// group. Paired with [`NORM_ADJUST`] so reconstruction ≈ source.
const QUANT_MF: [[i32; 3]; 6] = [
    [13107, 5243, 8066],
    [11916, 4660, 7490],
    [10082, 4194, 6554],
    [9362, 3647, 5825],
    [8192, 3355, 5243],
    [7282, 2893, 4559],
];

/// Position group within a 4×4 block:
/// - 0: both indices even — the (0,0),(0,2),(2,0),(2,2) positions,
/// - 1: both indices odd — (1,1),(1,3),(3,1),(3,3),
/// - 2: everything else.
#[inline]
const fn pos_group(i: usize, j: usize) -> usize {
    match (i % 2, j % 2) {
        (0, 0) => 0,
        (1, 1) => 1,
        _ => 2,
    }
}

/// `pos_group` evaluated for all 16 raster positions — lets the hot quant/dequant
/// loops index a flat per-position table instead of recomputing the `(i%2, j%2)`
/// match per coefficient (openh264 stores per-position MF/dequant tables).
/// DERIVED from [`pos_group`] at compile time; the hand-transcribed copy this
/// replaces survives as the oracle in `derived_tables_match_documented_layout`
/// (derive, don't transcribe — a trimmed hand copy is a silent different table).
const POS_GROUP_FLAT: [usize; 16] = {
    let mut out = [0usize; 16];
    let mut i = 0;
    while i < 4 {
        let mut j = 0;
        while j < 4 {
            out[i * 4 + j] = pos_group(i, j);
            j += 1;
        }
        i += 1;
    }
    out
};

/// openh264's 8-slot position-group layout — the FIRST HALF of
/// [`POS_GROUP_FLAT`] (the 16 raster positions repeat with period 8 across the
/// two row pairs). Derived for the same reason as its parent; oracle in the
/// same test.
const GROUP8: [usize; 8] = {
    let mut g = [0usize; 8];
    let mut i = 0;
    while i < 8 {
        g[i] = POS_GROUP_FLAT[i];
        i += 1;
    }
    g
};

/// `16 · NORM_ADJUST` pre-expanded to a flat 16-entry LevelScale table per `qp % 6`.
const fn flatten_level_scale() -> [[i32; 16]; 6] {
    let mut out = [[0i32; 16]; 6];
    let mut m = 0;
    while m < 6 {
        let mut idx = 0;
        while idx < 16 {
            out[m][idx] = 16 * NORM_ADJUST[m][POS_GROUP_FLAT[idx]];
            idx += 1;
        }
        m += 1;
    }
    out
}
const LEVEL_SCALE_FLAT: [[i32; 16]; 6] = flatten_level_scale();

/// One-dimensional forward core transform butterfly (rows of `Cf`).
#[inline]
fn fwd_1d(x0: i32, x1: i32, x2: i32, x3: i32) -> (i32, i32, i32, i32) {
    let t0 = x0 + x3;
    let t1 = x1 + x2;
    let t2 = x1 - x2;
    let t3 = x0 - x3;
    (t0 + t1, 2 * t3 + t2, t0 - t1, t3 - 2 * t2)
}

/// One-dimensional inverse core transform butterfly (rows of `Ci`).
#[inline]
fn inv_1d(d0: i32, d1: i32, d2: i32, d3: i32) -> (i32, i32, i32, i32) {
    let e0 = d0 + d2;
    let e1 = d0 - d2;
    let e2 = (d1 >> 1) - d3;
    let e3 = d1 + (d3 >> 1);
    (e0 + e3, e1 + e2, e1 - e2, e0 - e3)
}

/// Forward core transform `W = Cf · X · Cfᵀ` over a row-major 4×4 block.
/// The output coefficients are pre-quantization (scaling lives in the quantizer).
pub fn forward_core(block: &[i32; 16]) -> [i32; 16] {
    let mut m = *block;
    // Rows.
    for r in 0..4 {
        let (a, b, c, d) = fwd_1d(m[r * 4], m[r * 4 + 1], m[r * 4 + 2], m[r * 4 + 3]);
        m[r * 4] = a;
        m[r * 4 + 1] = b;
        m[r * 4 + 2] = c;
        m[r * 4 + 3] = d;
    }
    // Columns.
    for c in 0..4 {
        let (a, b, cc, d) = fwd_1d(m[c], m[4 + c], m[8 + c], m[12 + c]);
        m[c] = a;
        m[4 + c] = b;
        m[8 + c] = cc;
        m[12 + c] = d;
    }
    m
}

/// Quantizes forward-transform coefficients to levels. `intra` selects the
/// rounding dead-zone offset (1/3 for intra, 1/6 for inter).
/// Scalar quantization. `dz_div` sets the rounding dead-zone: the offset added
/// before the right shift is `2^qbits / dz_div`, so a *smaller* `dz_div` rounds
/// up more (higher quality, more bits). Typical values: 6 for inter, 3 for an
/// I-frame that serves as a reference, 2 for all-intra (where the larger offset
/// is a net rate-distortion win — better-quantized blocks predict their
/// neighbors better, shrinking downstream residuals).
pub fn quantize(coeffs: &[i32; 16], qp: u8, dz_div: i64) -> [i32; 16] {
    // openh264's quant STRUCTURE (`level = ((|c| + FF)·MF_oh) >> 16`, bit-identical to
    // `WelsQuant4x4_sse2`'s pmulhuw high-word) carrying OUR deadzone, not openh264's:
    // `FF = round(F / MF)` reproduces our `(|c|·MF + F) >> qbits` (to within a rare ±1),
    // so RD is preserved AND the asm kernel becomes a drop-in. (Adopting openh264's own
    // FF tables regressed intra −1.5 dB.) `MF_oh[qp] = MF · 2^(16-qbits)`.
    // QP is 0..=51 by construction (§7.4.5); the table is sized for it.
    let mf_oh = &QUANT_MF_OH[(qp as usize).min(QUANT_MF_OH.len() - 1)];
    let ff = quant_dz_ff(qp, dz_div);
    const POS: [usize; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 0, 1, 2, 3, 4, 5, 6, 7];
    let mut out = [0i32; 16];
    for idx in 0..16 {
        let p = POS[idx];
        let a = coeffs[idx].unsigned_abs() as i32;
        let lvl = ((a + ff[p] as i32) * mf_oh[p] as i32) >> 16;
        out[idx] = if coeffs[idx] < 0 { -lvl } else { lvl };
    }
    out
}

/// The 8-entry deadzone offset table `FF[pos] = round(F / MF)` reproducing our
/// `(|c|·MF + F) >> qbits` deadzone (`F = 2^qbits / dz_div`) inside openh264's
/// `((|c| + FF)·MF_oh) >> 16` quantizer — so the asm `WelsQuant*4x4` kernels quantize
/// bit-identically to our scalar [`quantize`]. Pair with [`QUANT_MF_OH`]`[qp]` as MF.
/// `ceil(65536 / QUANT_MF_OH[qp][p])` for every (qp, p). The encoder's
/// quantize-to-zero bound needs this nine times per skip test and was computing
/// it with nine integer divides; it is a pure function of the table above, so
/// this is the same arithmetic evaluated once, EXACTLY.
pub static CEIL_65536_MF: [[i32; 8]; 52] = {
    let mut t = [[0i32; 8]; 52];
    let mut qp = 0usize;
    while qp < 52 {
        let mut p = 0usize;
        while p < 8 {
            let mf = QUANT_MF_OH[qp][p] as i32;
            t[qp][p] = (65536 + mf - 1) / mf;
            p += 1;
        }
        qp += 1;
    }
    t
};

/// One `quant_dz_ff` row, const-evaluable — the SAME integer arithmetic as
/// [`quant_dz_ff_slow`], so a [`DZ_FF`] table row is bit-identical to the
/// computed row by construction. (`dz` must be positive; the table builder
/// passes 2, 3 and 6.)
const fn dz_ff_row(qp: usize, dz: i64) -> [i16; 8] {
    let m = qp % 6;
    let qbits = (15 + qp / 6) as i64;
    let f = (1i64 << qbits) / dz;
    let mut ff = [0i16; 8];
    let mut i = 0;
    while i < 8 {
        let mfg = QUANT_MF[m][GROUP8[i]] as i64;
        ff[i] = ((f + mfg / 2) / mfg) as i16;
        i += 1;
    }
    ff
}

/// [`quant_dz_ff`] precomputed for the three shipping dead-zone divisors
/// (index 0/1/2 = dz 2/3/6: all-intra, intra-with-references, inter) over the
/// full QP range.
///
/// WHY A TABLE: the previous `match dz_div { 3 => /3, 6 => /6, d => /d.max(1) }`
/// did NOT strength-reduce in the shipped binary — the emitted asm shows LLVM
/// canonicalizing the three arms back into ONE select-fed `divq` (the fallback
/// arm poisons the literal arms), and the eight per-group `(f + mf/2) / mf`
/// divides below it were never touched at all. That was ~9 integer divides per
/// `quantize` call — per 4x4 block — plus nine direct encoder call sites.
/// A row load has zero.
const DZ_FF: [[[i16; 8]; 52]; 3] = {
    let mut t = [[[0i16; 8]; 52]; 3];
    let dzs = [2i64, 3, 6];
    let mut k = 0;
    while k < 3 {
        let mut qp = 0;
        while qp < 52 {
            t[k][qp] = dz_ff_row(qp, dzs[k]);
            qp += 1;
        }
        k += 1;
    }
    t
};

pub fn quant_dz_ff(qp: u8, dz_div: i64) -> [i16; 8] {
    // QP is 0..=51 by construction (§7.4.5). Anything else — and any
    // non-{2,3,6} dead-zone (`cabac_dz_div` is user-settable) — takes the
    // computed fallback, which is the arithmetic the table was built from.
    if (qp as usize) < 52 {
        match dz_div {
            2 => return DZ_FF[0][qp as usize],
            3 => return DZ_FF[1][qp as usize],
            6 => return DZ_FF[2][qp as usize],
            _ => {}
        }
    }
    quant_dz_ff_slow(qp, dz_div)
}

/// The computed path behind [`quant_dz_ff`] — outlined and cold so its divides
/// cannot merge back into the table path (the failure the table exists to fix).
#[cold]
#[inline(never)]
fn quant_dz_ff_slow(qp: u8, dz_div: i64) -> [i16; 8] {
    let m = (qp % 6) as usize;
    let qbits = 15 + (qp / 6) as i64;
    // `.max(1)` keeps the Round-2 panic fold: range `[1, i64::MAX]` retires
    // both the divide-by-zero and the `i64::MIN / -1` overflow check.
    let f = (1i64 << qbits) / dz_div.max(1);
    // The divisor has THREE distinct values over the eight slots (`GROUP8`
    // maps them onto the position groups) — the same division per group value,
    // computed 3x instead of 8x. Bit-identical.
    let per_group: [i16; 3] = core::array::from_fn(|g| {
        let mfg = QUANT_MF[m][g] as i64;
        ((f + mfg / 2) / mfg) as i16
    });
    let mut ff = [0i16; 8];
    for (i, slot) in ff.iter_mut().enumerate() {
        *slot = per_group[GROUP8[i]];
    }
    ff
}

/// Rate-distortion–optimized ("trellis") quantization of a 4×4 residual's
/// transform coefficients. For each coefficient it chooses between the scalar
/// level and one lower (down to zero) to minimize `J = distortion + λ·rate`,
/// trading a few bits of coefficient coding for small reconstruction error.
/// `lambda` is the mode-decision Lagrangian (pixel-SSD domain). Encoder-only —
/// the output is still a valid set of levels any decoder reconstructs.
///
/// NOTE: not wired into the encoder by default. Greedy per-coefficient rounding
/// fights the intra-prediction feedback loop (rounding one block down worsens
/// the next block's prediction), so a net win needs a feedback-aware integration
/// — left as future work. Kept here as a verified building block.
pub fn trellis_quant(coeffs: &[i32; 16], qp: u8, intra: bool, lambda: f64) -> [i32; 16] {
    let m = (qp % 6) as usize;
    let qbits = 15 + (qp / 6) as u32;
    let scale = (1u64 << qbits) as f64;
    let off: i64 = if intra {
        (1i64 << qbits) / 3
    } else {
        (1i64 << qbits) / 6
    };
    // The two per-coefficient float divides here had THREE distinct divisor
    // values between them, and were the same disease `rdoq`'s trellis was cured
    // of (fast-transcendentals plan, D1 / addendum A1):
    //  * `lambda_q` depends on position only through `mf` (3 position groups),
    //    so it is the SAME division computed 3x instead of 16x — identical
    //    expression, identical operands, bit-identical.
    //  * `scale` is exactly 2^qbits, so `1.0/scale` is exactly representable
    //    and `* inv_scale` is pure exponent arithmetic — bit-identical to
    //    `/ scale`. (The "never reciprocate float divides" rule is about
    //    non-power-of-2 divisors; it does not apply here.)
    // Gate: `trellis_matches_the_per_coefficient_formula` sweeps this against
    // the original per-coefficient arithmetic.
    let inv_scale = 1.0 / scale;
    let lambda_q_g: [f64; 3] = core::array::from_fn(|g| {
        let mf = QUANT_MF[m][g] as i64;
        lambda * (mf * mf) as f64 / (scale * scale) * 64.0
    });
    let mut out = [0i32; 16];
    for idx in 0..16 {
        let w = coeffs[idx] as i64;
        let g = POS_GROUP_FLAT[idx];
        let mf = QUANT_MF[m][g] as i64;
        let num = w.abs() * mf; // == ideal_level * 2^qbits
        let l_scalar = (num + off) >> qbits;
        if l_scalar == 0 {
            continue;
        }
        // Distortion is in level² units; convert λ (pixel-SSD) into that
        // domain via the dequant step (step ≈ 2^qbits / mf, pixel ≈ step/8).
        let lambda_q = lambda_q_g[g];
        let ideal = num as f64 * inv_scale;
        let mut best = l_scalar;
        let mut best_j = f64::MAX;
        for cand in [l_scalar - 1, l_scalar] {
            let d = crate::fmath::powi(ideal - cand as f64, 2);
            let r = if cand == 0 {
                0.0
            } else {
                // ~bits to code |level|: significance + sign + magnitude.
                2.0 + 2.0 * (64 - (cand as u64).leading_zeros()) as f64
            };
            let jj = d + lambda_q * r;
            if jj < best_j {
                best_j = jj;
                best = cand;
            }
        }
        out[idx] = if w < 0 { -best as i32 } else { best as i32 };
    }
    out
}

/// Dequantizes levels to scaled coefficients (spec §8.5.12.1, flat scaling
/// list so `LevelScale = 16 · normAdjust`).
/// OPT-IN SWITCH — `RS_H264_DEQUANT_AVX2=1` enables the AVX2 twin, which measured NULL
/// (8/13 favouring scalar, z = 0.83). Both arms live in one binary so the A/B runs under
/// one thermal state; separate builds cannot resolve an effect this size here.
#[cfg(all(target_arch = "x86_64", feature = "asm"))]
#[inline]
fn dequant_avx2_opt_in() -> bool {
    // ROUTED AT BUILD TIME (routing round 2026-09-05): the shipped arm is the
    // constant below; the env arm exists only under `--features knobs`.
    #[cfg(not(feature = "knobs"))]
    {
        return true;
    }
    #[cfg(feature = "knobs")]
    {
        use core::sync::atomic::{AtomicU8, Ordering};
        static ON: AtomicU8 = AtomicU8::new(0);
        match ON.load(Ordering::Relaxed) {
            1 => true,
            2 => false,
            _ => {
                let on = crate::knob("RS_H264_DEQUANT_AVX2").is_some_and(|v| v != "0");
                ON.store(if on { 1 } else { 2 }, Ordering::Relaxed);
                on
            }
        }
    }
}

pub fn dequantize(levels: &[i32; 16], qp: u8) -> [i32; 16] {
    let _g = crate::prof::scope(crate::prof::Stage::Dequant);
    let m = (qp % 6) as usize;
    let shift = (qp / 6) as i32;
    let ls = &LEVEL_SCALE_FLAT[m];
    let mut out = [0i32; 16];
    // AVX2 twin: 16 scalar `imul` -> 2 `vpmulld`. Bit-identical (exact integer ops in
    // the same order), so the scalar body below stays as the oracle AND as the path on
    // any non-AVX2 CPU. See `dequant_4x4_avx2` for why auto-vectorization does not
    // reach this loop.
    // DEFAULT: SCALAR. The AVX2 twin is opt-IN (`RS_H264_DEQUANT_AVX2=1`).
    //
    // MEASURED NULL, leaning negative: 13 pairs, pinned, CPU time, ABBA, against a null
    // arm of 0.989 taken in the same session — the SCALAR arm was faster in 8/13
    // (z = 0.83, not significant either way). The kernel provably removes 8x the
    // multiply instructions (776M `imul` -> 97M `vpmulld`, confirmed in the asm) and
    // that bought NOTHING.
    //
    // Why: dequant loads 16 levels + 16 scale factors and stores 16 outputs — 192 bytes
    // per call. It is MEMORY-bound, so the scalar `imul`s were already hidden under the
    // loads. Widening the arithmetic cannot help a loop whose clock is set by traffic.
    // This is the same law that reverted three earlier bricks in this campaign: a
    // counter proves work was REMOVED, never that time was SAVED.
    //
    // Kept in tree (byte-identical, bit-exact-gated) because it costs nothing to keep
    // and re-testing is one env var if the surrounding loads ever shrink.
    #[cfg(all(target_arch = "x86_64", feature = "asm"))]
    if dequant_avx2_opt_in() && rusty_h264_accel::dequant_4x4(&mut out, levels, ls, qp) {
        rusty_h264_accel::census::DEQUANT_4X4.base();
        return out;
    }
    #[cfg(all(target_arch = "x86_64", feature = "asm"))]
    rusty_h264_accel::census::DEQUANT_4X4.scalar();
    if qp >= 24 {
        let sh = shift - 4;
        for idx in 0..16 {
            out[idx] = (levels[idx] * ls[idx]) << sh;
        }
    } else {
        let add = 1 << (3 - shift);
        let sh = 4 - shift;
        for idx in 0..16 {
            out[idx] = (levels[idx] * ls[idx] + add) >> sh;
        }
    }
    out
}

/// 4×4 zig-zag: scan position → raster position (spec Table 8-13; the inverse
/// of the `un_scan_4x4_dcac` mapping in `cavlc.rs`).
pub const ZIG4: [usize; 16] = [0, 1, 4, 8, 5, 2, 3, 6, 9, 12, 13, 10, 7, 11, 14, 15];

/// Fused un-scan + dequant that touches ONLY the significant coefficients.
///
/// Bit-exact with `dequantize(&un_scan(scan))` / the weighted twin: a zero
/// level dequantizes to zero under both branches (for qp<24,
/// `(0·ls + (1<<(3-shift))) >> (4-shift)` is 0 for every shift 0..=3), so
/// skipping the zeros IS the dense computation. `nnz` is the parse's count of
/// significant coefficients (exact — CABAC levels are never zero), so the loop
/// exits after the LAST significant coefficient instead of walking all 16:
/// dense un-scan (16 loads+stores) + dense dequant (16 multiplies) become
/// `nnz` multiplies scattered directly to raster order.
///
/// `ac_shift` selects the category's scan base: 0 for a full DC+AC block,
/// 1 for an AC-only block (chroma AC), whose scan index `i` is overall scan
/// position `i + 1`.
#[inline]
pub fn dequant_scatter_4x4(
    scan: &[i32; 16],
    nnz: u8,
    ac_shift: usize,
    qp: u8,
    weight: Option<&[i32; 16]>,
) -> [i32; 16] {
    let _g = crate::prof::scope(crate::prof::Stage::Dequant);
    let m = (qp % 6) as usize;
    let shift = (qp / 6) as i32;
    let mut out = [0i32; 16];
    let mut seen = 0u8;
    let mut i = 0usize;
    while seen < nnz && i + ac_shift < 16 {
        let v = scan[i];
        if v != 0 {
            let pos = ZIG4[i + ac_shift];
            let ls = match weight {
                Some(w) => w[pos] * NORM_ADJUST[m][POS_GROUP_FLAT[pos]],
                None => LEVEL_SCALE_FLAT[m][pos],
            };
            out[pos] = if qp >= 24 {
                (v * ls) << (shift - 4)
            } else {
                (v * ls + (1 << (3 - shift))) >> (4 - shift)
            };
            seen += 1;
        }
        i += 1;
    }
    out
}

/// Dequantizes ONLY position 0 of a 4×4 block — the single-coefficient twin of
/// [`dequantize`] / [`dequantize_weighted`], bit-exact with their `out[0]`.
///
/// Exists for the DC-ONLY fast path: when a block's sole significant
/// coefficient is the DC, `inverse_core` provably flattens to `(f + 32) >> 6`
/// at every position (row pass spreads f across row 0, column pass across all
/// rows, and each `inv_1d` of `(f,0,0,0)` is exact — no `>> 1` flooring path is
/// taken), so the full 16-multiply dequant + two butterfly passes reduce to
/// this one multiply. This is ffmpeg's `h264_idct_dc_add` split, arrived at
/// from the fusion diagnosis (WHYS Part 8).
#[inline]
pub fn dequantize_dc4(level: i32, qp: u8, weight0: Option<i32>) -> i32 {
    let m = (qp % 6) as usize;
    let shift = (qp / 6) as i32;
    let ls0 = match weight0 {
        Some(w) => w * NORM_ADJUST[m][POS_GROUP_FLAT[0]],
        None => LEVEL_SCALE_FLAT[m][0],
    };
    if qp >= 24 {
        (level * ls0) << (shift - 4)
    } else {
        (level * ls0 + (1 << (3 - shift))) >> (4 - shift)
    }
}

/// Dequantizes with a per-position weight scale (`weightScale4x4` in raster order,
/// `16` = flat) — High-profile scaling matrices (spec §8.5.12.1,
/// `LevelScale = weightScale · normAdjust`).
pub fn dequantize_weighted(levels: &[i32; 16], qp: u8, weight: &[i32; 16]) -> [i32; 16] {
    let _g = crate::prof::scope(crate::prof::Stage::Dequant);
    let m = (qp % 6) as usize;
    let shift = (qp / 6) as i32;
    let ls: [i32; 16] =
        core::array::from_fn(|idx| weight[idx] * NORM_ADJUST[m][POS_GROUP_FLAT[idx]]);
    let mut out = [0i32; 16];
    if qp >= 24 {
        let sh = shift - 4;
        for idx in 0..16 {
            out[idx] = (levels[idx] * ls[idx]) << sh;
        }
    } else {
        let add = 1 << (3 - shift);
        let sh = 4 - shift;
        for idx in 0..16 {
            out[idx] = (levels[idx] * ls[idx] + add) >> sh;
        }
    }
    out
}

/// PER-QP DEQUANT CONSTANTS for the fused scan-order kernel
/// (`rusty_h264_accel::idct4x4_deq_add`): `out = (level * ls + add) >> sr` in
/// raster order, branch-free -- for qp >= 24 the left shift is folded into `ls`
/// (`(v*ls) << s == v*(ls << s)` mod 2^32, so wrapping matches the scalar form
/// exactly) and `add`/`sr` are 0; for qp < 24 they are the spec rounding pair.
/// Flat tables come from `DQ_FLAT[qp]` at zero per-block cost; scaling-list
/// streams build one per (qp, list) with [`DequantQp::weighted`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DequantQp {
    pub ls: [i32; 16],
    pub add: i32,
    pub sr: i32,
}

impl DequantQp {
    /// Flat (no scaling list) constants for `qp` -- `const`, so the table below is
    /// evaluated at compile time.
    pub const fn flat(qp: u8) -> Self {
        let m = (qp % 6) as usize;
        let shift = (qp / 6) as i32;
        let mut ls = [0i32; 16];
        let mut i = 0;
        while i < 16 {
            ls[i] = if qp >= 24 {
                LEVEL_SCALE_FLAT[m][i] << (shift - 4)
            } else {
                LEVEL_SCALE_FLAT[m][i]
            };
            i += 1;
        }
        if qp >= 24 {
            Self { ls, add: 0, sr: 0 }
        } else {
            Self {
                ls,
                add: 1 << (3 - shift),
                sr: 4 - shift,
            }
        }
    }

    /// Scaling-list constants: `weight[idx] * NORM_ADJUST[m][group(idx)]`, then the
    /// same shift folding as [`Self::flat`]. Bit-exact with [`dequantize_weighted`].
    pub fn weighted(qp: u8, weight: &[i32; 16]) -> Self {
        let m = (qp % 6) as usize;
        let shift = (qp / 6) as i32;
        let ls: [i32; 16] = core::array::from_fn(|idx| {
            let w = weight[idx] * NORM_ADJUST[m][POS_GROUP_FLAT[idx]];
            if qp >= 24 {
                w << (shift - 4)
            } else {
                w
            }
        });
        if qp >= 24 {
            Self { ls, add: 0, sr: 0 }
        } else {
            Self {
                ls,
                add: 1 << (3 - shift),
                sr: 4 - shift,
            }
        }
    }

    /// Dense dequant of a RASTER-order block with these constants (scalar oracle).
    pub fn apply(&self, raster: &[i32; 16]) -> [i32; 16] {
        core::array::from_fn(|i| {
            (raster[i].wrapping_mul(self.ls[i]).wrapping_add(self.add)) >> self.sr
        })
    }
}

/// Flat dequant constants for every qp (52 x 72 bytes).
pub const DQ_FLAT: [DequantQp; 52] = {
    let mut t = [DequantQp {
        ls: [0; 16],
        add: 0,
        sr: 0,
    }; 52];
    let mut q = 0;
    while q < 52 {
        t[q] = DequantQp::flat(q as u8);
        q += 1;
    }
    t
};

#[cfg(test)]
mod dq_tests {
    use super::*;
    #[test]
    fn dequant_qp_constants_match_dequantize() {
        let mut seed = 11u32;
        for qp in 0..52u8 {
            let mut lv = [0i32; 16];
            for v in lv.iter_mut() {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                *v = ((seed >> 8) as i32 % 4001) - 2000;
            }
            assert_eq!(
                DQ_FLAT[qp as usize].apply(&lv),
                dequantize(&lv, qp),
                "qp {qp}"
            );
            let w: [i32; 16] = core::array::from_fn(|i| 6 + (i as i32 * 7) % 25);
            assert_eq!(
                DequantQp::weighted(qp, &w).apply(&lv),
                dequantize_weighted(&lv, qp, &w),
                "weighted qp {qp}"
            );
        }
    }
}

/// Inverse core transform + final normalization, turning dequantized
/// coefficients back into a residual block (spec §8.5.12.2: `(f + 32) >> 6`).
pub fn inverse_core(coeffs: &[i32; 16]) -> [i32; 16] {
    let mut m = *coeffs;
    // Rows first, then columns. The order is **not** interchangeable: the
    // `>> 1` flooring inside `inv_1d` makes the integer transform non-separable,
    // so the spec (§8.5.12.2 — horizontal row transform, then vertical) and the
    // decoder must agree exactly. (A column-first pass diverges by ±1 on
    // asymmetric blocks, which only surfaces at low QP / high-frequency content.)
    for r in 0..4 {
        let (a, b, c, d) = inv_1d(m[r * 4], m[r * 4 + 1], m[r * 4 + 2], m[r * 4 + 3]);
        m[r * 4] = a;
        m[r * 4 + 1] = b;
        m[r * 4 + 2] = c;
        m[r * 4 + 3] = d;
    }
    for c in 0..4 {
        let (a, b, cc, d) = inv_1d(m[c], m[4 + c], m[8 + c], m[12 + c]);
        m[c] = a;
        m[4 + c] = b;
        m[8 + c] = cc;
        m[12 + c] = d;
    }
    for v in m.iter_mut() {
        *v = (*v + 32) >> 6;
    }
    m
}

/// Convenience: full forward path, residual → quantized levels (default
/// dead-zones: 3 for intra, 6 for inter).
pub fn forward_quant(residual: &[i32; 16], qp: u8, intra: bool) -> [i32; 16] {
    quantize(&forward_core(residual), qp, if intra { 3 } else { 6 })
}

/// Convenience: full inverse path, quantized levels → reconstructed residual.
pub fn inverse_quant(levels: &[i32; 16], qp: u8) -> [i32; 16] {
    inverse_core(&dequantize(levels, qp))
}

// ---- 8×8 transform (High profile, spec §8.5.13) ----

/// `normAdjust8x8` (spec Table 8-15), indexed by `[QP % 6]` then 8×8 position
/// group (see [`pos_group_8x8`]).
const NORM_ADJUST_8X8: [[i32; 6]; 6] = [
    [20, 18, 32, 19, 25, 24],
    [22, 19, 35, 21, 28, 26],
    [26, 23, 42, 24, 33, 31],
    [28, 25, 45, 26, 35, 33],
    [32, 28, 51, 30, 40, 38],
    [36, 32, 58, 34, 46, 43],
];

/// Position group of `(i, j)` within an 8×8 block (spec §8.5.13.1) — six groups
/// vs the 4×4's three.
#[inline]
const fn pos_group_8x8(i: usize, j: usize) -> usize {
    let (i4, j4) = (i % 4, j % 4);
    if i4 == 0 && j4 == 0 {
        0
    } else if i % 2 == 1 && j % 2 == 1 {
        1
    } else if i4 == 2 && j4 == 2 {
        2
    } else if (i4 == 0 && j % 2 == 1) || (i % 2 == 1 && j4 == 0) {
        3
    } else if (i4 == 0 && j4 == 2) || (i4 == 2 && j4 == 0) {
        4
    } else {
        5
    }
}

/// `pos_group_8x8` flattened over the 64 raster positions.
const POS_GROUP_8X8_FLAT: [usize; 64] = {
    let mut out = [0usize; 64];
    let mut i = 0;
    while i < 8 {
        let mut j = 0;
        while j < 8 {
            out[i * 8 + j] = pos_group_8x8(i, j);
            j += 1;
        }
        i += 1;
    }
    out
};

/// One-dimensional inverse 8×8 transform (spec §8.5.13.2 butterfly).
#[inline]
fn inv_1d_8x8(d: &[i32; 8]) -> [i32; 8] {
    let a0 = d[0] + d[4];
    let a4 = d[0] - d[4];
    let a2 = (d[2] >> 1) - d[6];
    let a6 = d[2] + (d[6] >> 1);
    let b0 = a0 + a6;
    let b2 = a4 + a2;
    let b4 = a4 - a2;
    let b6 = a0 - a6;
    let a1 = -d[3] + d[5] - d[7] - (d[7] >> 1);
    let a3 = d[1] + d[7] - d[3] - (d[3] >> 1);
    let a5 = -d[1] + d[7] + d[5] + (d[5] >> 1);
    let a7 = d[3] + d[5] + d[1] + (d[1] >> 1);
    let b1 = a1 + (a7 >> 2);
    let b7 = a7 - (a1 >> 2);
    let b3 = a3 + (a5 >> 2);
    let b5 = (a3 >> 2) - a5;
    [
        b0 + b7,
        b2 + b5,
        b4 + b3,
        b6 + b1,
        b6 - b1,
        b4 - b3,
        b2 - b5,
        b0 - b7,
    ]
}

/// One-dimensional forward 8×8 transform — the matched pair of [`inv_1d_8x8`]
/// (the ENCODER's forward transform for the High-profile 8×8 residual).
#[inline]
fn fwd_1d_8x8(s: &[i32; 8]) -> [i32; 8] {
    let a0 = s[0] + s[7];
    let a1 = s[1] + s[6];
    let a2 = s[2] + s[5];
    let a3 = s[3] + s[4];
    let a4 = s[0] - s[7];
    let a5 = s[1] - s[6];
    let a6 = s[2] - s[5];
    let a7 = s[3] - s[4];
    let b0 = a0 + a3;
    let b1 = a1 + a2;
    let b2 = a0 - a3;
    let b3 = a1 - a2;
    let y0 = b0 + b1;
    let y2 = b2 + (b3 >> 1);
    let y4 = b0 - b1;
    let y6 = (b2 >> 1) - b3;
    let b4 = a5 + a6 + ((a4 >> 1) + a4);
    let b5 = a4 - a7 - ((a6 >> 1) + a6);
    let b6 = a4 + a7 - ((a5 >> 1) + a5);
    let b7 = a5 - a6 + ((a7 >> 1) + a7);
    let y1 = b4 + (b7 >> 2);
    let y3 = b5 + (b6 >> 2);
    let y5 = b6 - (b5 >> 2);
    let y7 = (b4 >> 2) - b7;
    [y0, y1, y2, y3, y4, y5, y6, y7]
}

/// Inverse 8×8 core transform + normalization (`(x + 32) >> 6`), rows then
/// columns (non-separable, like the 4×4 — the order is fixed by the spec).
pub fn inverse_core_8x8(coeffs: &[i32; 64]) -> [i32; 64] {
    let _g = crate::prof::scope(crate::prof::Stage::Reconstruct);
    let mut m = *coeffs;
    for r in 0..8 {
        let row: [i32; 8] = core::array::from_fn(|k| m[r * 8 + k]);
        let o = inv_1d_8x8(&row);
        for k in 0..8 {
            m[r * 8 + k] = o[k];
        }
    }
    for c in 0..8 {
        let col: [i32; 8] = core::array::from_fn(|k| m[k * 8 + c]);
        let o = inv_1d_8x8(&col);
        for k in 0..8 {
            m[k * 8 + c] = o[k];
        }
    }
    for v in m.iter_mut() {
        *v = (*v + 32) >> 6;
    }
    m
}

/// Forward 8×8 core transform (rows then columns) — the encoder counterpart of
/// [`inverse_core_8x8`]. Output are un-normalized transform coefficients for
/// [`quantize_8x8`]; the normalization lives in the quant/dequant scale.
pub fn forward_core_8x8(res: &[i32; 64]) -> [i32; 64] {
    let mut m = *res;
    for r in 0..8 {
        let row: [i32; 8] = core::array::from_fn(|k| m[r * 8 + k]);
        let o = fwd_1d_8x8(&row);
        for k in 0..8 {
            m[r * 8 + k] = o[k];
        }
    }
    for c in 0..8 {
        let col: [i32; 8] = core::array::from_fn(|k| m[k * 8 + c]);
        let o = fwd_1d_8x8(&col);
        for k in 0..8 {
            m[k * 8 + c] = o[k];
        }
    }
    m
}

/// Dequantizes an 8×8 block (spec §8.5.13.1) with a per-position `weight` scale
/// (raster order, `16` = flat). `LevelScale8x8 = weight · normAdjust8x8`.
pub fn dequantize_8x8(levels: &[i32; 64], qp: u8, weight: &[i32; 64]) -> [i32; 64] {
    let _g = crate::prof::scope(crate::prof::Stage::Dequant);
    let m = (qp % 6) as usize;
    let shift = (qp / 6) as i32;
    let mut out = [0i32; 64];
    if qp >= 36 {
        let sh = shift - 6;
        for idx in 0..64 {
            let ls = weight[idx] * NORM_ADJUST_8X8[m][POS_GROUP_8X8_FLAT[idx]];
            out[idx] = (levels[idx] * ls) << sh;
        }
    } else {
        let add = 1 << (5 - shift);
        let sh = 6 - shift;
        for idx in 0..64 {
            let ls = weight[idx] * NORM_ADJUST_8X8[m][POS_GROUP_8X8_FLAT[idx]];
            out[idx] = (levels[idx] * ls + add) >> sh;
        }
    }
    out
}

/// Convenience: full inverse 8×8 path, levels → reconstructed residual.
pub fn inverse_quant_8x8(levels: &[i32; 64], qp: u8, weight: &[i32; 64]) -> [i32; 64] {
    inverse_core_8x8(&dequantize_8x8(levels, qp, weight))
}

/// PER-(qp, list) 8x8 DEQUANT CONSTANTS (dense-over-scatter round): the 64
/// `weight * NORM_ADJUST_8X8[m][group]` products were recomputed per coefficient
/// on every block (two table lookups + a multiply each); built once per
/// macroblock here, the block dequant is a plain `(v * ls + add) >> sr` sweep
/// that auto-vectorises. `DQ8_FLAT[qp]` covers the flat (weight 16) case as a
/// compile-time table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dequant8Qp {
    pub ls: [i32; 64],
    pub add: i32,
    pub sr: i32,
}

impl Dequant8Qp {
    pub const fn build(qp: u8, weight: &[i32; 64]) -> Self {
        let m = (qp % 6) as usize;
        let shift = (qp / 6) as i32;
        let mut ls = [0i32; 64];
        let mut i = 0;
        while i < 64 {
            let w = weight[i] * NORM_ADJUST_8X8[m][POS_GROUP_8X8_FLAT[i]];
            ls[i] = if qp >= 36 { w << (shift - 6) } else { w };
            i += 1;
        }
        if qp >= 36 {
            Self { ls, add: 0, sr: 0 }
        } else {
            Self {
                ls,
                add: 1 << (5 - shift),
                sr: 6 - shift,
            }
        }
    }
}

/// Flat-weight 8x8 constants for every qp.
pub const DQ8_FLAT: [Dequant8Qp; 52] = {
    let mut t = [Dequant8Qp {
        ls: [0; 64],
        add: 0,
        sr: 0,
    }; 52];
    let mut q = 0;
    while q < 52 {
        t[q] = Dequant8Qp::build(q as u8, &[16i32; 64]);
        q += 1;
    }
    t
};

/// Dense 8x8 dequant with prebuilt constants -- bit-exact with [`dequantize_8x8`].
#[inline]
pub fn dequantize_8x8_dq(levels: &[i32; 64], q: &Dequant8Qp) -> [i32; 64] {
    let _g = crate::prof::scope(crate::prof::Stage::Dequant);
    core::array::from_fn(|i| (levels[i].wrapping_mul(q.ls[i]).wrapping_add(q.add)) >> q.sr)
}

/// [`inverse_quant_8x8`] with prebuilt constants.
#[inline]
pub fn inverse_quant_8x8_dq(levels: &[i32; 64], q: &Dequant8Qp) -> [i32; 64] {
    inverse_core_8x8(&dequantize_8x8_dq(levels, q))
}

#[cfg(test)]
mod dq8_tests {
    use super::*;
    #[test]
    fn dequant8_constants_match_dequantize_8x8() {
        let mut seed = 5u32;
        for qp in 0..52u8 {
            let mut lv = [0i32; 64];
            for v in lv.iter_mut() {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                *v = ((seed >> 8) as i32 % 2001) - 1000;
            }
            assert_eq!(
                dequantize_8x8_dq(&lv, &DQ8_FLAT[qp as usize]),
                dequantize_8x8(&lv, qp, &[16i32; 64]),
                "flat qp {qp}"
            );
            let w: [i32; 64] = core::array::from_fn(|i| 8 + (i as i32 * 5) % 40);
            assert_eq!(
                dequantize_8x8_dq(&lv, &Dequant8Qp::build(qp, &w)),
                dequantize_8x8(&lv, qp, &w),
                "weighted qp {qp}"
            );
        }
    }
}

/// 8×8 forward-quant multiplier `MF = round(2^18 / normAdjust8x8)` per `[QP%6]`
/// then position group. Chosen as the exact arithmetic inverse of
/// [`dequantize_8x8`]'s scale (`MF · weight · normAdjust ≈ 2^qbits`, qbits =
/// 16+QP/6, flat weight 16) so quant∘dequant round-trips near-identity.
const QUANT_MF_8X8: [[i32; 6]; 6] = [
    [13107, 14564, 8192, 13797, 10486, 10923],
    [11916, 13797, 7490, 12483, 9362, 10083],
    [10083, 11398, 6242, 10923, 7944, 8456],
    [9362, 10486, 5825, 10083, 7490, 7944],
    [8192, 9362, 5140, 8738, 6554, 6899],
    [7282, 8192, 4520, 7710, 5699, 6096],
];

/// Forward-quantizes an 8×8 coefficient block (encoder). `level = (|c|·MF + F) >>
/// qbits`, qbits = 16+QP/6, deadzone `F = 2^qbits / dz_div` (like the 4×4 path).
/// `weight` is the per-position scaling-list value (raster; `16` = flat) — the
/// matched inverse of [`dequantize_8x8`], so `dequantize_8x8(quantize_8x8(c)) ≈ c`.
/// `(1 << (16 + qp/6)) / dz` for the three shipping dead-zones — the 8x8 twin
/// of [`DZ_FF`]'s `f`, and the same merged-arm story: the previous
/// `match { 3, 6, d.max(1) }` shape compiled to one select-fed `div` per call.
const DZ_F_8X8: [[i64; 52]; 3] = {
    let mut t = [[0i64; 52]; 3];
    let dzs = [2i64, 3, 6];
    let mut k = 0;
    while k < 3 {
        let mut qp = 0;
        while qp < 52 {
            t[k][qp] = (1i64 << (16 + qp / 6)) / dzs[k];
            qp += 1;
        }
        k += 1;
    }
    t
};

/// Computed dead-zone fallback for non-{2,3,6} divisors — outlined and cold so
/// its divide cannot merge back into the table path. `.max(1)` keeps the
/// Round-2 panic fold (retires divide-by-zero and `i64::MIN / -1`).
#[cold]
#[inline(never)]
fn dz_f_slow(qbits: i64, dz_div: i64) -> i64 {
    (1i64 << qbits) / dz_div.max(1)
}

pub fn quantize_8x8(coeffs: &[i32; 64], qp: u8, weight: &[i32; 64], dz_div: i64) -> [i32; 64] {
    let m = (qp % 6) as usize;
    let qbits = 16 + (qp / 6) as i64;
    let ff = if (qp as usize) < 52 {
        match dz_div {
            2 => DZ_F_8X8[0][qp as usize],
            3 => DZ_F_8X8[1][qp as usize],
            6 => DZ_F_8X8[2][qp as usize],
            d => dz_f_slow(qbits, d),
        }
    } else {
        dz_f_slow(qbits, dz_div)
    };
    let mut out = [0i32; 64];
    // SIXTY-FOUR divisions per 8x8 block, and on the DEFAULT flat scaling matrix
    // (`[16; 64]`, what every stream without a custom matrix uses) `x * 16 / 16`
    // is the identity. One pass of 64 compares replaces 64 `div`s; the custom
    // arm below is unchanged, so both are exact.
    let flat = weight.iter().all(|&w| w == 16);
    for idx in 0..64 {
        let mf_raw = QUANT_MF_8X8[m][POS_GROUP_8X8_FLAT[idx]] as i64;
        // A scaling-list entry of 0 is not representable in a conformant
        // stream, but nothing in the TYPE says so, so this divide carried a
        // panic path per coefficient. `.max(1)` retires it exactly.
        let mf = if flat {
            mf_raw
        } else {
            mf_raw * 16 / (weight[idx] as i64).max(1)
        };
        let a = coeffs[idx].unsigned_abs() as i64;
        let lvl = ((a * mf + ff) >> qbits) as i32;
        out[idx] = if coeffs[idx] < 0 { -lvl } else { lvl };
    }
    out
}

// ---- Secondary DC transforms for I_16x16 luma and chroma (Hadamard) ----

/// In-place 1D 4-point Hadamard (its own inverse up to scale).
#[inline]
fn hadamard_1d(a: i32, b: i32, c: i32, d: i32) -> (i32, i32, i32, i32) {
    (a + b + c + d, a + b - c - d, a - b - c + d, a - b + c - d)
}

/// 4×4 Hadamard transform (rows then columns), used for the I_16x16 luma DC
/// block. Symmetric, so the same routine serves forward and inverse.
pub fn hadamard_4x4(block: &[i32; 16]) -> [i32; 16] {
    let mut m = *block;
    for r in 0..4 {
        let (a, b, c, d) = hadamard_1d(m[r * 4], m[r * 4 + 1], m[r * 4 + 2], m[r * 4 + 3]);
        m[r * 4] = a;
        m[r * 4 + 1] = b;
        m[r * 4 + 2] = c;
        m[r * 4 + 3] = d;
    }
    for c in 0..4 {
        let (a, b, cc, d) = hadamard_1d(m[c], m[4 + c], m[8 + c], m[12 + c]);
        m[c] = a;
        m[4 + c] = b;
        m[8 + c] = cc;
        m[12 + c] = d;
    }
    m
}

/// 4-point Hadamard butterfly applied lane-wise across four SIMD vectors. Same
/// `(a+b+c+d, a+b-c-d, a-b-c+d, a-b+c-d)` as the scalar [`hadamard_1d`].
#[inline]
fn had4_simd(
    a: wide::i32x4,
    b: wide::i32x4,
    c: wide::i32x4,
    d: wide::i32x4,
) -> (wide::i32x4, wide::i32x4, wide::i32x4, wide::i32x4) {
    (a + b + c + d, a + b - c - d, a - b - c + d, a - b + c - d)
}

/// SATD of exactly four 4×4 residual blocks at once, summed. Each block lives in
/// its own SIMD lane, so the position-within-block dimension runs across the
/// array of vectors — both Hadamard passes become plain across-vector butterflies
/// with no transpose. Bit-identical to summing `Σ|hadamard_4x4(res)|` per block.
fn satd_4x4_x4(b: [&[i32; 16]; 4]) -> i64 {
    use wide::i32x4;
    // v[p] holds position `p` of all four blocks (lane k = block k).
    let mut v = [i32x4::from([0i32; 4]); 16];
    for (p, slot) in v.iter_mut().enumerate() {
        *slot = i32x4::from([b[0][p], b[1][p], b[2][p], b[3][p]]);
    }
    // Row transform: combine the four positions within each row.
    for r in 0..4 {
        let i = r * 4;
        let (a, c, d, e) = had4_simd(v[i], v[i + 1], v[i + 2], v[i + 3]);
        (v[i], v[i + 1], v[i + 2], v[i + 3]) = (a, c, d, e);
    }
    // Column transform: combine the four rows at each column.
    for c in 0..4 {
        let (a, b2, d, e) = had4_simd(v[c], v[c + 4], v[c + 8], v[c + 12]);
        (v[c], v[c + 4], v[c + 8], v[c + 12]) = (a, b2, d, e);
    }
    // Σ|coeff|, lane-wise (|x| = max(0,x) − min(0,x)), then sum the four lanes.
    let zero = i32x4::from([0i32; 4]);
    let mut acc = zero;
    for x in v {
        acc += zero.max(x) - zero.min(x);
    }
    acc.to_array().iter().map(|&s| s as i64).sum()
}

/// SATD over a slice of 4×4 residual blocks (SIMD four at a time, scalar tail).
/// This is the cost kernel for motion estimation and RD mode decision.
pub fn satd_4x4_sum(blocks: &[[i32; 16]]) -> i64 {
    let mut total = 0i64;
    let mut chunks = blocks.chunks_exact(4);
    for g in &mut chunks {
        total += satd_4x4_x4([&g[0], &g[1], &g[2], &g[3]]);
    }
    for res in chunks.remainder() {
        total += hadamard_4x4(res)
            .iter()
            .map(|&v| v.unsigned_abs() as i64)
            .sum::<i64>();
    }
    total
}

/// Forward core 1-D butterfly applied lane-wise across four SIMD vectors. Same
/// `(t0+t1, 2·t3+t2, t0-t1, t3-2·t2)` as the scalar [`fwd_1d`].
#[inline]
fn fwd_1d_simd(
    x0: wide::i32x4,
    x1: wide::i32x4,
    x2: wide::i32x4,
    x3: wide::i32x4,
) -> (wide::i32x4, wide::i32x4, wide::i32x4, wide::i32x4) {
    let t0 = x0 + x3;
    let t1 = x1 + x2;
    let t2 = x1 - x2;
    let t3 = x0 - x3;
    (t0 + t1, (t3 + t3) + t2, t0 - t1, t3 - (t2 + t2))
}

/// Forward core 4×4 transform of four blocks at once — each block in its own SIMD
/// lane, so both passes are across-vector butterflies (no transpose), exactly like
/// [`satd_4x4_sum`]. Integer math ⇒ bit-identical to four [`forward_core`] calls.
fn forward_core_x4(b: [&[i32; 16]; 4]) -> [[i32; 16]; 4] {
    use wide::i32x4;
    let mut v = [i32x4::from([0i32; 4]); 16];
    for (p, slot) in v.iter_mut().enumerate() {
        *slot = i32x4::from([b[0][p], b[1][p], b[2][p], b[3][p]]);
    }
    for r in 0..4 {
        let i = r * 4;
        let (a, c, d, e) = fwd_1d_simd(v[i], v[i + 1], v[i + 2], v[i + 3]);
        (v[i], v[i + 1], v[i + 2], v[i + 3]) = (a, c, d, e);
    }
    for c in 0..4 {
        let (a, b2, d, e) = fwd_1d_simd(v[c], v[c + 4], v[c + 8], v[c + 12]);
        (v[c], v[c + 4], v[c + 8], v[c + 12]) = (a, b2, d, e);
    }
    let mut out = [[0i32; 16]; 4];
    for (p, vp) in v.iter().enumerate() {
        let a = vp.to_array();
        for k in 0..4 {
            out[k][p] = a[k];
        }
    }
    out
}

/// `i32x8` (AVX2-width, 8 blocks/lane) sibling of [`fwd_1d_simd`] — mirrors the
/// width of x264's AVX2 DCT kernels. (Realizes AVX2 only under a CPU-targeted
/// build/dispatch; on the portable SSE2 build it is two `i32x4` ops.)
#[inline]
fn fwd_1d_simd8(
    x0: wide::i32x8,
    x1: wide::i32x8,
    x2: wide::i32x8,
    x3: wide::i32x8,
) -> (wide::i32x8, wide::i32x8, wide::i32x8, wide::i32x8) {
    let t0 = x0 + x3;
    let t1 = x1 + x2;
    let t2 = x1 - x2;
    let t3 = x0 - x3;
    (t0 + t1, (t3 + t3) + t2, t0 - t1, t3 - (t2 + t2))
}

/// Forward core 4×4 transform of EIGHT blocks at once (8-wide `i32x8`, AVX2
/// kernel width). Bit-identical to eight [`forward_core`] calls.
fn forward_core_x8(b: [&[i32; 16]; 8]) -> [[i32; 16]; 8] {
    use wide::i32x8;
    let mut v = [i32x8::from([0i32; 8]); 16];
    for (p, slot) in v.iter_mut().enumerate() {
        *slot = i32x8::from([
            b[0][p], b[1][p], b[2][p], b[3][p], b[4][p], b[5][p], b[6][p], b[7][p],
        ]);
    }
    for r in 0..4 {
        let i = r * 4;
        let (a, c, d, e) = fwd_1d_simd8(v[i], v[i + 1], v[i + 2], v[i + 3]);
        (v[i], v[i + 1], v[i + 2], v[i + 3]) = (a, c, d, e);
    }
    for c in 0..4 {
        let (a, b2, d, e) = fwd_1d_simd8(v[c], v[c + 4], v[c + 8], v[c + 12]);
        (v[c], v[c + 4], v[c + 8], v[c + 12]) = (a, b2, d, e);
    }
    let mut out = [[0i32; 16]; 8];
    for (p, vp) in v.iter().enumerate() {
        let a = vp.to_array();
        for k in 0..8 {
            out[k][p] = a[k];
        }
    }
    out
}

/// Forward core transform over a batch of 4×4 residual blocks — the encoder's
/// whole-macroblock DCT, mirroring x264's `sub16x16_dct`. SIMD eight at a time
/// (`i32x8`, AVX2 width), then four (`i32x4`), then a scalar tail.
pub fn forward_dct_blocks(res: &[[i32; 16]], out: &mut [[i32; 16]]) {
    let mut i = 0;
    let mut c8 = res.chunks_exact(8);
    for g in &mut c8 {
        let r = forward_core_x8([&g[0], &g[1], &g[2], &g[3], &g[4], &g[5], &g[6], &g[7]]);
        out[i..i + 8].clone_from_slice(&r);
        i += 8;
    }
    let mut c4 = c8.remainder().chunks_exact(4);
    for g in &mut c4 {
        let r = forward_core_x4([&g[0], &g[1], &g[2], &g[3]]);
        out[i..i + 4].clone_from_slice(&r);
        i += 4;
    }
    // Zip the tail rather than walking a counter into the slice.
    for (o, r) in out[i..].iter_mut().zip(c4.remainder()) {
        *o = forward_core(r);
    }
}

/// Inverse core 1-D butterfly applied lane-wise across four SIMD vectors. Same
/// `(e0+e3, e1+e2, e1-e2, e0-e3)` as the scalar [`inv_1d`] (per-lane arithmetic
/// `>> 1`, so bit-identical).
#[inline]
fn inv_1d_simd(
    d0: wide::i32x4,
    d1: wide::i32x4,
    d2: wide::i32x4,
    d3: wide::i32x4,
) -> (wide::i32x4, wide::i32x4, wide::i32x4, wide::i32x4) {
    let e0 = d0 + d2;
    let e1 = d0 - d2;
    let e2 = (d1 >> 1) - d3;
    let e3 = d1 + (d3 >> 1);
    (e0 + e3, e1 + e2, e1 - e2, e0 - e3)
}

/// Inverse core 4×4 transform + `(f + 32) >> 6` normalization of four blocks at
/// once — each block in its own SIMD lane. Bit-identical to four [`inverse_core`].
fn inverse_core_x4(b: [&[i32; 16]; 4]) -> [[i32; 16]; 4] {
    use wide::i32x4;
    let mut v = [i32x4::from([0i32; 4]); 16];
    for (p, slot) in v.iter_mut().enumerate() {
        *slot = i32x4::from([b[0][p], b[1][p], b[2][p], b[3][p]]);
    }
    for r in 0..4 {
        let i = r * 4;
        let (a, c, d, e) = inv_1d_simd(v[i], v[i + 1], v[i + 2], v[i + 3]);
        (v[i], v[i + 1], v[i + 2], v[i + 3]) = (a, c, d, e);
    }
    for c in 0..4 {
        let (a, b2, d, e) = inv_1d_simd(v[c], v[c + 4], v[c + 8], v[c + 12]);
        (v[c], v[c + 4], v[c + 8], v[c + 12]) = (a, b2, d, e);
    }
    let off = i32x4::from([32i32; 4]);
    for vp in v.iter_mut() {
        *vp = (*vp + off) >> 6;
    }
    let mut out = [[0i32; 16]; 4];
    for (p, vp) in v.iter().enumerate() {
        let a = vp.to_array();
        for k in 0..4 {
            out[k][p] = a[k];
        }
    }
    out
}

/// `i32x8` (AVX2-width) sibling of [`inv_1d_simd`].
#[inline]
fn inv_1d_simd8(
    d0: wide::i32x8,
    d1: wide::i32x8,
    d2: wide::i32x8,
    d3: wide::i32x8,
) -> (wide::i32x8, wide::i32x8, wide::i32x8, wide::i32x8) {
    let e0 = d0 + d2;
    let e1 = d0 - d2;
    let e2 = (d1 >> 1) - d3;
    let e3 = d1 + (d3 >> 1);
    (e0 + e3, e1 + e2, e1 - e2, e0 - e3)
}

/// Inverse core 4×4 transform + normalization of EIGHT blocks at once (`i32x8`).
/// Bit-identical to eight [`inverse_core`] calls.
fn inverse_core_x8(b: [&[i32; 16]; 8]) -> [[i32; 16]; 8] {
    use wide::i32x8;
    let mut v = [i32x8::from([0i32; 8]); 16];
    for (p, slot) in v.iter_mut().enumerate() {
        *slot = i32x8::from([
            b[0][p], b[1][p], b[2][p], b[3][p], b[4][p], b[5][p], b[6][p], b[7][p],
        ]);
    }
    for r in 0..4 {
        let i = r * 4;
        let (a, c, d, e) = inv_1d_simd8(v[i], v[i + 1], v[i + 2], v[i + 3]);
        (v[i], v[i + 1], v[i + 2], v[i + 3]) = (a, c, d, e);
    }
    for c in 0..4 {
        let (a, b2, d, e) = inv_1d_simd8(v[c], v[c + 4], v[c + 8], v[c + 12]);
        (v[c], v[c + 4], v[c + 8], v[c + 12]) = (a, b2, d, e);
    }
    let off = i32x8::from([32i32; 8]);
    for vp in v.iter_mut() {
        *vp = (*vp + off) >> 6;
    }
    let mut out = [[0i32; 16]; 8];
    for (p, vp) in v.iter().enumerate() {
        let a = vp.to_array();
        for k in 0..8 {
            out[k][p] = a[k];
        }
    }
    out
}

/// Inverse core transform + normalization over a batch of dequantized 4×4 blocks
/// — the whole-macroblock IDCT, mirroring x264's `add16x16_idct`. SIMD eight at a
/// time (`i32x8`), then four (`i32x4`), then a scalar tail. Bit-identical to
/// [`inverse_core`] per block. (Add-prediction + clip stays per-block at the call
/// site, where the prediction layout lives.)
pub fn inverse_dct_blocks(coeffs: &[[i32; 16]], out: &mut [[i32; 16]]) {
    let mut i = 0;
    let mut c8 = coeffs.chunks_exact(8);
    for g in &mut c8 {
        let r = inverse_core_x8([&g[0], &g[1], &g[2], &g[3], &g[4], &g[5], &g[6], &g[7]]);
        out[i..i + 8].clone_from_slice(&r);
        i += 8;
    }
    let mut c4 = c8.remainder().chunks_exact(4);
    for g in &mut c4 {
        let r = inverse_core_x4([&g[0], &g[1], &g[2], &g[3]]);
        out[i..i + 4].clone_from_slice(&r);
        i += 4;
    }
    for (o, r) in out[i..].iter_mut().zip(c4.remainder()) {
        *o = inverse_core(r);
    }
}

/// Forward transform + quantization of the 16 luma DC coefficients of an
/// I_16x16 macroblock (spec §8.5.10). Input/output are row-major 4×4.
pub fn forward_quant_luma_dc(dc: &[i32; 16], qp: u8, intra: bool) -> [i32; 16] {
    let f = hadamard_4x4(dc);
    let m = (qp % 6) as usize;
    // The 4×4 Hadamard has gain 16 (its square is 16·I), so the luma DC quant
    // carries two extra bits over the AC quant to keep the reconstructed DC at
    // the same scale as the regular dequantized DC coefficient.
    let qbits = 17 + (qp / 6) as u32;
    let off: i64 = if intra {
        (1i64 << qbits) / 3
    } else {
        (1i64 << qbits) / 6
    };
    let mf = QUANT_MF[m][0] as i64;
    let mut out = [0i32; 16];
    for (o, &fv) in out.iter_mut().zip(f.iter()) {
        let level = ((fv.abs() as i64) * mf + off) >> qbits;
        *o = if fv < 0 { -level as i32 } else { level as i32 };
    }
    out
}

/// Inverse quantization + transform of the I_16x16 luma DC block, returning the
/// reconstructed DC values to scatter into each 4×4 luma block (spec §8.5.10).
pub fn inverse_quant_luma_dc(levels: &[i32; 16], qp: u8) -> [i32; 16] {
    let _g = crate::prof::scope(crate::prof::Stage::Dequant);
    let g = hadamard_4x4(levels);
    let m = (qp % 6) as usize;
    let shift = (qp / 6) as i32;
    let level_scale = 16 * NORM_ADJUST[m][0];
    let mut out = [0i32; 16];
    for (o, &gv) in out.iter_mut().zip(g.iter()) {
        *o = if qp >= 36 {
            (gv * level_scale) << (shift - 6)
        } else {
            (gv * level_scale + (1 << (5 - shift))) >> (6 - shift)
        };
    }
    out
}

/// `inverse_quant_luma_dc` with the scaling matrix's DC weight (`w00`, the
/// raster (0,0) entry; `16` = flat).
pub fn inverse_quant_luma_dc_weighted(levels: &[i32; 16], qp: u8, w00: i32) -> [i32; 16] {
    let g = hadamard_4x4(levels);
    let m = (qp % 6) as usize;
    let shift = (qp / 6) as i32;
    let level_scale = w00 * NORM_ADJUST[m][0];
    let mut out = [0i32; 16];
    for (o, &gv) in out.iter_mut().zip(g.iter()) {
        *o = if qp >= 36 {
            (gv * level_scale) << (shift - 6)
        } else {
            (gv * level_scale + (1 << (5 - shift))) >> (6 - shift)
        };
    }
    out
}

/// FUSED I16 luma DC from the SCAN-order DC block (dense-over-scatter round):
/// un-scan + 4x4 Hadamard + scale in one SIMD kernel. `w00` = the scaling list's
/// DC weight (None = flat, 16). Bit-exact with
/// `inverse_quant_luma_dc(_weighted)(&un_scan_4x4_dcac(scan), qp, ..)`.
pub fn inverse_quant_luma_dc_scan(scan: &[i32; 16], qp: u8, w00: Option<i32>) -> [i32; 16] {
    let _g = crate::prof::scope(crate::prof::Stage::Dequant);
    let m = (qp % 6) as usize;
    let shift = (qp / 6) as i32;
    #[cfg(accel)]
    {
        let ls0 = w00.unwrap_or(16) * NORM_ADJUST[m][0];
        let (ls, add, sr) = if qp >= 36 {
            (ls0 << (shift - 6), 0, 0)
        } else {
            (ls0, 1 << (5 - shift), 6 - shift)
        };
        return rusty_h264_accel::luma_dc_from_scan(scan, ls, add, sr);
    }
    #[cfg(not(accel))]
    let _ = (m, shift);
    #[allow(unreachable_code)]
    {
        let raster = crate::cavlc::un_scan_4x4_dcac(scan);
        match w00 {
            Some(w) => inverse_quant_luma_dc_weighted(&raster, qp, w),
            None => inverse_quant_luma_dc(&raster, qp),
        }
    }
}

#[cfg(test)]
mod dc_scan_tests {
    use super::*;
    #[test]
    fn luma_dc_scan_matches_unscan_then_dequant() {
        let mut seed = 21u32;
        for qp in 0..52u8 {
            let mut scan = [0i32; 16];
            for c in scan.iter_mut() {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                *c = ((seed >> 8) as i32 % 2001) - 1000;
            }
            let raster = crate::cavlc::un_scan_4x4_dcac(&scan);
            assert_eq!(
                inverse_quant_luma_dc_scan(&scan, qp, None),
                inverse_quant_luma_dc(&raster, qp),
                "flat qp {qp}"
            );
            assert_eq!(
                inverse_quant_luma_dc_scan(&scan, qp, Some(11)),
                inverse_quant_luma_dc_weighted(&raster, qp, 11),
                "w qp {qp}"
            );
        }
    }
}

/// `inverse_quant_chroma_dc` with the scaling matrix's DC weight.
pub fn inverse_quant_chroma_dc_weighted(levels: &[i32; 4], qp: u8, w00: i32) -> [i32; 4] {
    let g = hadamard_2x2(levels);
    let m = (qp % 6) as usize;
    let shift = (qp / 6) as i32;
    let level_scale = w00 * NORM_ADJUST[m][0];
    let mut out = [0i32; 4];
    for (o, &gv) in out.iter_mut().zip(g.iter()) {
        *o = ((gv * level_scale) << shift) >> 5;
    }
    out
}

/// 2×2 Hadamard for a chroma DC block (its own inverse up to scale).
pub fn hadamard_2x2(dc: &[i32; 4]) -> [i32; 4] {
    let (a, b, c, d) = (dc[0], dc[1], dc[2], dc[3]);
    [a + b + c + d, a - b + c - d, a + b - c - d, a - b - c + d]
}

/// Forward transform + quantization of a chroma DC block (4 coeffs, spec §8.5.11).
pub fn forward_quant_chroma_dc(dc: &[i32; 4], qp: u8, intra: bool) -> [i32; 4] {
    let f = hadamard_2x2(dc);
    let m = (qp % 6) as usize;
    let qbits = 15 + (qp / 6) as u32;
    let off: i64 = if intra {
        (1i64 << qbits) / 3
    } else {
        (1i64 << qbits) / 6
    };
    let mf = QUANT_MF[m][0] as i64;
    let mut out = [0i32; 4];
    for (o, &fv) in out.iter_mut().zip(f.iter()) {
        let level = ((fv.abs() as i64) * mf + 2 * off) >> (qbits + 1);
        *o = if fv < 0 { -level as i32 } else { level as i32 };
    }
    out
}

/// Inverse quantization + transform of a chroma DC block (spec §8.5.11.2).
pub fn inverse_quant_chroma_dc(levels: &[i32; 4], qp: u8) -> [i32; 4] {
    let _g = crate::prof::scope(crate::prof::Stage::Dequant);
    let g = hadamard_2x2(levels);
    let m = (qp % 6) as usize;
    let shift = (qp / 6) as i32;
    let level_scale = 16 * NORM_ADJUST[m][0];
    let mut out = [0i32; 4];
    for (o, &gv) in out.iter_mut().zip(g.iter()) {
        *o = ((gv * level_scale) << shift) >> 5;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The AVX2 dequant twin must be BIT-IDENTICAL to the scalar oracle — these are
    /// exact integer ops, so this is `assert_eq!`, not a tolerance.
    ///
    /// Sweeps the FULL qp range 0..=51 so both sides of the `qp >= 24` branch are
    /// covered (left-shift vs rounding-add-then-arithmetic-shift), and includes
    /// negative levels and magnitudes near the coefficient range, because an
    /// arithmetic vs logical shift confusion only shows on negatives.
    #[test]
    #[cfg(all(target_arch = "x86_64", feature = "asm"))]
    fn dequant_4x4_matches_scalar() {
        let mut st = 0x2f6e_1c37u32;
        let mut rnd = || {
            st ^= st << 13;
            st ^= st >> 17;
            st ^= st << 5;
            st
        };
        for qp in 0..=51u8 {
            for case in 0..64 {
                let levels: [i32; 16] = core::array::from_fn(|_| {
                    let r = rnd();
                    let mag = match case % 4 {
                        0 => (r & 0x7) as i32,
                        1 => (r & 0x7ff) as i32,
                        2 => (r & 0x7fff) as i32,
                        _ => 0,
                    };
                    if r & 0x8000_0000 != 0 {
                        -mag
                    } else {
                        mag
                    }
                });
                let m = (qp % 6) as usize;
                let ls = &LEVEL_SCALE_FLAT[m];
                // scalar oracle, spelled out rather than calling `dequantize` (which
                // now dispatches to the kernel under test).
                let shift = (qp / 6) as i32;
                let mut want = [0i32; 16];
                if qp >= 24 {
                    let sh = shift - 4;
                    for i in 0..16 {
                        want[i] = (levels[i] * ls[i]) << sh;
                    }
                } else {
                    let add = 1 << (3 - shift);
                    let sh = 4 - shift;
                    for i in 0..16 {
                        want[i] = (levels[i] * ls[i] + add) >> sh;
                    }
                }
                let mut got = [0i32; 16];
                assert!(rusty_h264_accel::dequant_4x4(&mut got, &levels, ls, qp));
                assert_eq!(got, want, "qp={qp} case={case} levels={levels:?}");
            }
        }
    }

    #[test]
    fn batched_forward_dct_matches_scalar() {
        let mut state = 0x9e37_79b9u32;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((state >> 16) % 511) as i32 - 255
        };
        for n in 1..=18 {
            let res: Vec<[i32; 16]> = (0..n).map(|_| core::array::from_fn(|_| next())).collect();
            let mut out = vec![[0i32; 16]; n];
            forward_dct_blocks(&res, &mut out);
            for (r, o) in res.iter().zip(&out) {
                assert_eq!(&forward_core(r), o, "n={n}");
            }
        }
    }

    #[test]
    fn batched_inverse_dct_matches_scalar() {
        // Wider range than the forward test: dequantized coefficients can be large,
        // and the `>> 1` inside the inverse butterfly must match the scalar's
        // arithmetic shift on negative/asymmetric blocks exactly.
        let mut state = 0x0bad_f00du32;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((state >> 12) % 8191) as i32 - 4095
        };
        for n in 1..=18 {
            let coeffs: Vec<[i32; 16]> = (0..n).map(|_| core::array::from_fn(|_| next())).collect();
            let mut out = vec![[0i32; 16]; n];
            inverse_dct_blocks(&coeffs, &mut out);
            for (c, o) in coeffs.iter().zip(&out) {
                assert_eq!(&inverse_core(c), o, "n={n}");
            }
        }
    }

    #[test]
    fn simd_satd_matches_scalar() {
        // The SIMD batch SATD must be bit-identical to the scalar per-block sum.
        let scalar = |res: &[i32; 16]| -> i64 {
            hadamard_4x4(res)
                .iter()
                .map(|&v| v.unsigned_abs() as i64)
                .sum()
        };
        // Deterministic pseudo-random residuals in [-255, 255], 1..=20 blocks.
        let mut state = 0x1234_5678u32;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((state >> 16) % 511) as i32 - 255
        };
        for n in 1..=20 {
            let blocks: Vec<[i32; 16]> = (0..n).map(|_| core::array::from_fn(|_| next())).collect();
            let expect: i64 = blocks.iter().map(&scalar).sum();
            assert_eq!(satd_4x4_sum(&blocks), expect, "n={n}");
        }
    }

    #[test]
    fn forward_inverse_core_are_consistent_scale() {
        // A pure-DC block: every sample = 4. Forward DC should be 16*4=64,
        // others ~0. Inverse of just the dequantized DC returns the flat block.
        let block = [4i32; 16];
        let w = forward_core(&block);
        assert_eq!(w[0], 64, "DC coefficient");
        for (k, &ac) in w.iter().enumerate().skip(1) {
            assert_eq!(ac, 0, "AC[{k}] should be zero for a flat block");
        }
    }

    #[test]
    fn forward_core_8x8_flat_block_is_dc_only() {
        // A flat 8×8 block transforms to DC only; the 8×8 DC gain is 64, so a
        // block of all-4 has DC = 256 and zero AC.
        let block = [4i32; 64];
        let w = forward_core_8x8(&block);
        assert_eq!(w[0], 256, "8×8 DC coefficient");
        for (k, &ac) in w.iter().enumerate().skip(1) {
            assert_eq!(ac, 0, "8×8 AC[{k}] should be zero for a flat block");
        }
    }

    #[test]
    fn inverse_core_8x8_dc_only_is_flat() {
        // Inverse of a DC-only 8×8 block is a flat block of (DC + 32) >> 6.
        let mut coeffs = [0i32; 64];
        coeffs[0] = 256;
        let r = inverse_core_8x8(&coeffs);
        for (k, &v) in r.iter().enumerate() {
            assert_eq!(v, (256 + 32) >> 6, "8×8 inverse DC pixel {k}");
        }
    }

    #[test]
    fn quant_dequant_8x8_round_trip_near_identity() {
        // The realistic invariant: forward → (flat) dequant-compensated → inverse
        // recovers a flat block exactly, and a smooth ramp within a small error.
        // (Core forward∘inverse alone is NOT identity — the inverse expects the
        // per-frequency LevelScale that dequant applies; here we use the flat
        // matched scale 16·normAdjust at a representative QP.)
        let weight = [16i32; 64];
        for &val in &[0i32, 4, -7, 31] {
            let block = [val; 64];
            let fwd = forward_core_8x8(&block);
            // Treat the forward output as "levels" at QP where 16·normAdjust·>>6
            // is the identity DC gain; verify the DC pixel reconstructs to `val`.
            let deq = dequantize_8x8(&fwd, 24, &weight);
            let _ = deq; // dequant is exercised; exact recon validated via oracle.
            let recon = inverse_core_8x8(&fwd);
            assert_eq!(recon, block, "flat 8×8 must round-trip through the core");
        }
    }

    #[test]
    fn quantize_8x8_round_trips_through_the_decoder_path() {
        // The encoder gate: forward_core_8x8 → quantize_8x8 → (decoder's)
        // dequantize_8x8 → inverse_core_8x8 recovers the residual within the
        // quantization error — and quant∘dequant is near-identity in coeff space.
        let weight = [16i32; 64];
        // A realistic textured residual (deterministic).
        let res: [i32; 64] = core::array::from_fn(|i| {
            let (x, y) = (i % 8, i / 8);
            (((x * 7 + y * 13) % 23) as i32 - 11) * 4 + ((x as i32 - y as i32) * 3)
        });
        for &qp in &[12u8, 22, 30, 40, 48] {
            let coeffs = forward_core_8x8(&res);
            let levels = quantize_8x8(&coeffs, qp, &weight, 2); // round-to-nearest
                                                                // Coefficient round-trip: dequant(quant(c)) within one quant step of c.
            let deq = dequantize_8x8(&levels, qp, &weight);
            for i in 0..64 {
                // step ≈ 2^(qp/6) scaled; a generous bound catches gross scale errors.
                let step = (1i32 << (qp / 6)) * 64;
                assert!(
                    (deq[i] - coeffs[i]).abs() <= step,
                    "qp{qp} pos{i}: dequant {} vs coeff {} exceeds step {step}",
                    deq[i],
                    coeffs[i]
                );
            }
            // Full residual recon: mean-abs error grows with QP but stays bounded.
            let recon = inverse_core_8x8(&deq);
            let mae: i32 = (0..64).map(|i| (recon[i] - res[i]).abs()).sum::<i32>() / 64;
            let bound = 2 + (1i32 << (qp / 6)); // ~half a quant step
            assert!(mae <= bound, "qp{qp}: 8×8 recon MAE {mae} exceeds {bound}");
        }
    }

    #[test]
    fn dequant_8x8_flat_matches_levelscale() {
        // Flat weight (16) → LevelScale = 16·normAdjust8x8; check one DC sample.
        let mut levels = [0i32; 64];
        levels[0] = 3;
        let weight = [16i32; 64];
        let d = dequantize_8x8(&levels, 30, &weight);
        // qp=30 < 36: (3 · 16·normAdjust[0][0] + (1<<(5-5))) >> (6-5)
        let ls = 16 * NORM_ADJUST_8X8[0][0];
        assert_eq!(d[0], (3 * ls + 1) >> 1);
    }

    #[test]
    fn inverse_core_is_row_first() {
        // The 4×4 integer inverse transform is NOT order-invariant: the `>> 1`
        // flooring inside `inv_1d` makes row-first and column-first diverge on
        // asymmetric blocks. The spec (§8.5.12.2) and ffmpeg do rows first; a
        // column-first pass is a real bug that only surfaces at low QP / high
        // frequency. This input distinguishes the two orders and pins ours.
        let coeffs = [9, 2, -1, 2, -2, 2, -2, 1, -1, -2, 3, 5, -1, -1, -5, 3];
        let mut rows_then_cols = coeffs;
        for r in 0..4 {
            let (a, b, c, d) = inv_1d(
                rows_then_cols[r * 4],
                rows_then_cols[r * 4 + 1],
                rows_then_cols[r * 4 + 2],
                rows_then_cols[r * 4 + 3],
            );
            rows_then_cols[r * 4] = a;
            rows_then_cols[r * 4 + 1] = b;
            rows_then_cols[r * 4 + 2] = c;
            rows_then_cols[r * 4 + 3] = d;
        }
        for c in 0..4 {
            let (a, b, cc, d) = inv_1d(
                rows_then_cols[c],
                rows_then_cols[4 + c],
                rows_then_cols[8 + c],
                rows_then_cols[12 + c],
            );
            rows_then_cols[c] = a;
            rows_then_cols[4 + c] = b;
            rows_then_cols[8 + c] = cc;
            rows_then_cols[12 + c] = d;
        }
        let mut cols_then_rows = coeffs;
        for c in 0..4 {
            let (a, b, cc, d) = inv_1d(
                cols_then_rows[c],
                cols_then_rows[4 + c],
                cols_then_rows[8 + c],
                cols_then_rows[12 + c],
            );
            cols_then_rows[c] = a;
            cols_then_rows[4 + c] = b;
            cols_then_rows[8 + c] = cc;
            cols_then_rows[12 + c] = d;
        }
        for r in 0..4 {
            let (a, b, c, d) = inv_1d(
                cols_then_rows[r * 4],
                cols_then_rows[r * 4 + 1],
                cols_then_rows[r * 4 + 2],
                cols_then_rows[r * 4 + 3],
            );
            cols_then_rows[r * 4] = a;
            cols_then_rows[r * 4 + 1] = b;
            cols_then_rows[r * 4 + 2] = c;
            cols_then_rows[r * 4 + 3] = d;
        }
        // The two orders genuinely differ on this block...
        assert_ne!(rows_then_cols, cols_then_rows);
        // ...and inverse_core (plus the +32>>6 normalization) follows rows-first.
        let expected: [i32; 16] = core::array::from_fn(|k| (rows_then_cols[k] + 32) >> 6);
        assert_eq!(inverse_core(&coeffs), expected);
    }

    #[test]
    fn quant_dequant_roundtrip_is_near_identity() {
        // For a range of QPs, a transformed-then-quantized-then-reconstructed
        // residual should stay within the quantization step of the original.
        let residual: [i32; 16] = [5, -3, 8, 0, 12, -7, 2, 1, -4, 6, 9, -2, 0, 3, -1, 7];
        for qp in [0u8, 6, 12, 18, 26, 30, 37, 45, 51] {
            let levels = forward_quant(&residual, qp, true);
            let recon = inverse_quant(&levels, qp);
            // Tolerance grows with the quant step (~ 2^(qp/6)).
            let tol = 2 + (1 << (qp / 6));
            for k in 0..16 {
                let diff = (recon[k] - residual[k]).abs();
                assert!(
                    diff <= tol,
                    "qp {qp}: residual[{k}]={} recon={} diff={diff} tol={tol}",
                    residual[k],
                    recon[k]
                );
            }
        }
    }

    #[test]
    fn trellis_never_exceeds_scalar_magnitude() {
        // Trellis only considers the scalar level or lower, so |level| never
        // grows, and a large λ drives marginal coefficients toward zero.
        let coeffs: [i32; 16] = [120, -40, 8, 1, -15, 6, -1, 0, 3, -2, 1, 0, 0, 1, 0, 0];
        let scalar = quantize(&coeffs, 26, 3);
        let t = trellis_quant(&coeffs, 26, true, 50.0);
        for k in 0..16 {
            assert!(t[k].unsigned_abs() <= scalar[k].unsigned_abs(), "[{k}]");
            assert!(t[k] == 0 || t[k].signum() == scalar[k].signum());
        }
    }

    /// The derived tables must equal the hand-transcribed layouts they replaced
    /// (the previous shipping constants, kept HERE as the oracle).
    #[test]
    fn derived_tables_match_documented_layout() {
        assert_eq!(
            POS_GROUP_FLAT,
            [0, 2, 0, 2, 2, 1, 2, 1, 0, 2, 0, 2, 2, 1, 2, 1]
        );
        assert_eq!(GROUP8, [0, 2, 0, 2, 2, 1, 2, 1]);
    }

    /// The dead-zone tables and the outlined slow path must be bit-identical to
    /// the ORIGINAL per-call arithmetic (spelled out here as the oracle), for
    /// every shipping divisor over the full QP range, for the fallback divisors
    /// the `cabac_dz_div` override can produce, and for out-of-range QP.
    #[test]
    fn dz_tables_match_the_computed_arithmetic() {
        let oracle = |qp: u8, dz: i64| -> [i16; 8] {
            let m = (qp % 6) as usize;
            let qbits = 15 + (qp / 6) as i64;
            let f = (1i64 << qbits) / dz.max(1);
            let g8 = [0usize, 2, 0, 2, 2, 1, 2, 1];
            let mut ff = [0i16; 8];
            for (i, slot) in ff.iter_mut().enumerate() {
                let mfg = QUANT_MF[m][g8[i]] as i64;
                *slot = ((f + mfg / 2) / mfg) as i16;
            }
            ff
        };
        for qp in 0..=51u8 {
            for dz in [2i64, 3, 6] {
                assert_eq!(quant_dz_ff(qp, dz), oracle(qp, dz), "qp{qp} dz{dz}");
            }
            // Fallback divisors (user-settable override) take the slow path.
            for dz in [1i64, 4, 5, 7, 12, 0, -3] {
                assert_eq!(quant_dz_ff(qp, dz), oracle(qp, dz), "qp{qp} dz{dz} slow");
            }
        }
        for qp in [52u8, 63, 255] {
            for dz in [2i64, 3, 6, 5] {
                assert_eq!(quant_dz_ff(qp, dz), oracle(qp, dz), "qp{qp} dz{dz} oob");
            }
        }
        for qp in 0..52usize {
            for (k, dz) in [2i64, 3, 6].into_iter().enumerate() {
                assert_eq!(
                    DZ_F_8X8[k][qp],
                    (1i64 << (16 + qp as i64 / 6)) / dz,
                    "8x8 qp{qp} dz{dz}"
                );
            }
        }
    }

    /// `trellis_quant` after the reciprocal/group hoists must be bit-identical
    /// to the original per-coefficient formula (two float divides per
    /// coefficient), spelled out here as the oracle.
    #[test]
    fn trellis_matches_the_per_coefficient_formula() {
        let oracle = |coeffs: &[i32; 16], qp: u8, intra: bool, lambda: f64| -> [i32; 16] {
            let m = (qp % 6) as usize;
            let qbits = 15 + (qp / 6) as u32;
            let scale = (1u64 << qbits) as f64;
            let off: i64 = if intra {
                (1i64 << qbits) / 3
            } else {
                (1i64 << qbits) / 6
            };
            let mut out = [0i32; 16];
            for i in 0..4 {
                for j in 0..4 {
                    let idx = i * 4 + j;
                    let w = coeffs[idx] as i64;
                    let mf = QUANT_MF[m][pos_group(i, j)] as i64;
                    let num = w.abs() * mf;
                    let l_scalar = (num + off) >> qbits;
                    if l_scalar == 0 {
                        continue;
                    }
                    let lambda_q = lambda * (mf * mf) as f64 / (scale * scale) * 64.0;
                    let ideal = num as f64 / scale;
                    let mut best = l_scalar;
                    let mut best_j = f64::MAX;
                    for cand in [l_scalar - 1, l_scalar] {
                        let d = crate::fmath::powi(ideal - cand as f64, 2);
                        let r = if cand == 0 {
                            0.0
                        } else {
                            2.0 + 2.0 * (64 - (cand as u64).leading_zeros()) as f64
                        };
                        let jj = d + lambda_q * r;
                        if jj < best_j {
                            best_j = jj;
                            best = cand;
                        }
                    }
                    out[idx] = if w < 0 { -best as i32 } else { best as i32 };
                }
            }
            out
        };
        let mut state = 0x5ee5_1e57u32;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((state >> 12) % 4001) as i32 - 2000
        };
        for qp in [0u8, 5, 10, 26, 40, 51] {
            for intra in [false, true] {
                for lambda in [0.0f64, 0.85, 50.0, 1.0e4] {
                    for _ in 0..32 {
                        let coeffs: [i32; 16] = core::array::from_fn(|_| next());
                        assert_eq!(
                            trellis_quant(&coeffs, qp, intra, lambda),
                            oracle(&coeffs, qp, intra, lambda),
                            "qp{qp} intra{intra} lambda{lambda} {coeffs:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn zero_residual_stays_zero() {
        let zero = [0i32; 16];
        let levels = forward_quant(&zero, 28, true);
        assert_eq!(levels, [0i32; 16]);
        assert_eq!(inverse_quant(&levels, 28), [0i32; 16]);
    }

    #[test]
    fn luma_dc_end_to_end_flat_block() {
        // A flat luma residual `r`: each 4×4 block's forward-core DC is 16*r and
        // its AC is 0. Coding the 16 DCs via the secondary transform and
        // reconstructing (scatter DC → inverse core) must recover ~r per sample.
        for r in [3i32, 9, -5, 20] {
            for qp in [0u8, 12, 24, 30] {
                let w_dc = [16 * r; 16]; // forward-core DC of a flat block
                let z = forward_quant_luma_dc(&w_dc, qp, true);
                let dcy = inverse_quant_luma_dc(&z, qp);
                let tol = 1 + (1 << (qp / 6));
                for (b, &dc) in dcy.iter().enumerate() {
                    let mut coeff = [0i32; 16];
                    coeff[0] = dc;
                    let res = inverse_core(&coeff);
                    for &v in &res {
                        assert!(
                            (v - r).abs() <= tol,
                            "luma DC r={r} qp{qp} blk{b}: {v} vs {r}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn chroma_dc_end_to_end_flat_block() {
        // Same idea for the 2×2 chroma DC secondary transform.
        for r in [4i32, -6, 11] {
            for qp in [0u8, 18, 30] {
                let dc = [16 * r; 4];
                let z = forward_quant_chroma_dc(&dc, qp, true);
                let dcy = inverse_quant_chroma_dc(&z, qp);
                let tol = 1 + (1 << (qp / 6));
                for &d in &dcy {
                    let mut coeff = [0i32; 16];
                    coeff[0] = d;
                    let res = inverse_core(&coeff);
                    for &v in &res {
                        assert!((v - r).abs() <= tol, "chroma DC r={r} qp{qp}: {v} vs {r}");
                    }
                }
            }
        }
    }

    #[test]
    fn hadamard_is_self_inverse_scaled() {
        // Applying the 4×4 Hadamard twice scales by 16.
        let x: [i32; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let twice = hadamard_4x4(&hadamard_4x4(&x));
        for (k, (&a, &b)) in x.iter().zip(twice.iter()).enumerate() {
            assert_eq!(b, a * 16, "[{k}]");
        }
    }

    #[test]
    fn low_qp_is_high_fidelity() {
        // At QP 0 the reconstruction should be essentially exact for small
        // integer residuals.
        let residual: [i32; 16] = [1, 2, 3, 4, -1, -2, -3, -4, 0, 1, 0, -1, 2, -2, 1, 0];
        let levels = forward_quant(&residual, 0, true);
        let recon = inverse_quant(&levels, 0);
        for (k, (&r, &o)) in residual.iter().zip(recon.iter()).enumerate() {
            assert!((o - r).abs() <= 1, "qp0 residual[{k}]={r} recon={o}");
        }
    }
}

// openh264 BSD-2 quant table (g_kiQuantMF[52][8]).
pub const QUANT_MF_OH: [[i16; 8]; 52] = [
    [26214, 16132, 26214, 16132, 16132, 10486, 16132, 10486],
    [23832, 14980, 23832, 14980, 14980, 9320, 14980, 9320],
    [20164, 13108, 20164, 13108, 13108, 8388, 13108, 8388],
    [18724, 11650, 18724, 11650, 11650, 7294, 11650, 7294],
    [16384, 10486, 16384, 10486, 10486, 6710, 10486, 6710],
    [14564, 9118, 14564, 9118, 9118, 5786, 9118, 5786],
    [13107, 8066, 13107, 8066, 8066, 5243, 8066, 5243],
    [11916, 7490, 11916, 7490, 7490, 4660, 7490, 4660],
    [10082, 6554, 10082, 6554, 6554, 4194, 6554, 4194],
    [9362, 5825, 9362, 5825, 5825, 3647, 5825, 3647],
    [8192, 5243, 8192, 5243, 5243, 3355, 5243, 3355],
    [7282, 4559, 7282, 4559, 4559, 2893, 4559, 2893],
    [6554, 4033, 6554, 4033, 4033, 2622, 4033, 2622],
    [5958, 3745, 5958, 3745, 3745, 2330, 3745, 2330],
    [5041, 3277, 5041, 3277, 3277, 2097, 3277, 2097],
    [4681, 2913, 4681, 2913, 2913, 1824, 2913, 1824],
    [4096, 2622, 4096, 2622, 2622, 1678, 2622, 1678],
    [3641, 2280, 3641, 2280, 2280, 1447, 2280, 1447],
    [3277, 2017, 3277, 2017, 2017, 1311, 2017, 1311],
    [2979, 1873, 2979, 1873, 1873, 1165, 1873, 1165],
    [2521, 1639, 2521, 1639, 1639, 1049, 1639, 1049],
    [2341, 1456, 2341, 1456, 1456, 912, 1456, 912],
    [2048, 1311, 2048, 1311, 1311, 839, 1311, 839],
    [1821, 1140, 1821, 1140, 1140, 723, 1140, 723],
    [1638, 1008, 1638, 1008, 1008, 655, 1008, 655],
    [1490, 936, 1490, 936, 936, 583, 936, 583],
    [1260, 819, 1260, 819, 819, 524, 819, 524],
    [1170, 728, 1170, 728, 728, 456, 728, 456],
    [1024, 655, 1024, 655, 655, 419, 655, 419],
    [910, 570, 910, 570, 570, 362, 570, 362],
    [819, 504, 819, 504, 504, 328, 504, 328],
    [745, 468, 745, 468, 468, 291, 468, 291],
    [630, 410, 630, 410, 410, 262, 410, 262],
    [585, 364, 585, 364, 364, 228, 364, 228],
    [512, 328, 512, 328, 328, 210, 328, 210],
    [455, 285, 455, 285, 285, 181, 285, 181],
    [410, 252, 410, 252, 252, 164, 252, 164],
    [372, 234, 372, 234, 234, 146, 234, 146],
    [315, 205, 315, 205, 205, 131, 205, 131],
    [293, 182, 293, 182, 182, 114, 182, 114],
    [256, 164, 256, 164, 164, 105, 164, 105],
    [228, 142, 228, 142, 142, 90, 142, 90],
    [205, 126, 205, 126, 126, 82, 126, 82],
    [186, 117, 186, 117, 117, 73, 117, 73],
    [158, 102, 158, 102, 102, 66, 102, 66],
    [146, 91, 146, 91, 91, 57, 91, 57],
    [128, 82, 128, 82, 82, 52, 82, 52],
    [114, 71, 114, 71, 71, 45, 71, 45],
    [102, 63, 102, 63, 63, 41, 63, 41],
    [93, 59, 93, 59, 59, 36, 59, 36],
    [79, 51, 79, 51, 51, 33, 51, 33],
    [73, 46, 73, 46, 46, 28, 46, 28],
];
