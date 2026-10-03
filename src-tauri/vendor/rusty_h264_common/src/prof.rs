//! Feature-gated decode/encode **stage profiler** — the instrument for perf work.
//!
//! Zero cost unless the `profile` feature is enabled: with it off, [`scope`] is a
//! no-op returning a ZST guard that the optimizer elides entirely, so release
//! builds are byte-identical and the hot path is untouched. With it on, each
//! kernel times itself into an atomic nanosecond bucket; [`dump`] prints the
//! per-stage breakdown.
//!
//! Design mirrors `rff-codec-mp3`'s `encode::prof`. The kernels (`mc_luma`,
//! `reconstruct_4x4`, `decode_residual_block`, the intra predictors, `deblock`)
//! each open a [`scope`] at their top, so every call is captured with one edit.
//! A [`Stage::Total`] scope wraps the whole `decode()` call; the **`mgmt/other`**
//! line is the residue (`Total − Σ stages`) — i.e. per-MB management, MV
//! prediction, nnz/grid bookkeeping, dequant — the bucket we most want to shrink.
//!
//! Caveat for honest reading: the fine-grained buckets (`reconstruct`, `entropy`)
//! are entered millions of times, so each carries ~one `Instant::now()` of timer
//! overhead — their share is mildly inflated and `mgmt/other` mildly deflated.
//! The `(N calls)` column lets you judge ns/call. Measure **throughput** with the
//! `profile` feature OFF (no timer overhead); use this breakdown only to rank
//! stages.

#[allow(unused_imports)]
use alloc::vec;

/// A timed pipeline stage. Order matters: everything before [`Total`](Stage::Total)
/// is a sub-component summed for the `mgmt/other` residue.
#[derive(Clone, Copy)]
pub enum Stage {
    Entropy = 0,
    IntraPred = 1,
    InterMc = 2,
    Reconstruct = 3,
    Deblock = 4,
    // --- Phase 1: decomposition of the former "mgmt/other" residue ---
    /// Inverse quantization (`dequantize*`, `inverse_quant_8x8`).
    Dequant = 5,
    /// Scattering a reconstructed block into the strided frame plane (`store`).
    Scatter = 6,
    /// Re-striding the MC output into the per-MB prediction buffer.
    PredBuf = 7,
    /// MV prediction + per-block motion/ref/coded grid writes.
    MvGrid = 8,
    // --- Phase 3 / ghost-tracking: further decomposition of the residue ---
    /// Neighbour derivation for prediction (MV/ref/intra-mode availability + reads).
    Neighbors = 9,
    /// P_Skip / B_Skip reconstruction (the pred→rec copies + grid writes, no residual).
    SkipRecon = 10,
    /// Per-frame finalize: output-frame build (crop), DPB / reference management.
    Finalize = 11,
    /// Per-MB non-residual syntax parse (mb_skip_run, mb_type, cbp, mb_qp_delta).
    Syntax = 12,
    /// `as_reference` DPB plane clone (rec_y/u/v → RefFrame), split out of Finalize.
    DpbClone = 13,
    // --- Encoder stages (a disjoint top-level partition of encode(); the shared
    // primitive scopes above — IntraPred/InterMc/Reconstruct/Deblock/Entropy —
    // nest INSIDE these and give the within-stage breakdown) ---
    /// `coded_source`: clamped copy of the source planes to the MB-aligned grid.
    EncSource = 14,
    /// P_Skip prediction + free-skip check (skip MC + SAD + commit).
    EncSkip = 15,
    /// Motion estimation: `best_part` (integer SAD search + sub-pel + SATD/λ cost).
    EncMe = 16,
    /// Intra mode cost inside the inter decision (`best_i16_sad`/`best_i16_satd`).
    EncIntraCost = 17,
    /// Coding a chosen inter MB (`encode_inter_mb`: MC, residual, T/Q, entropy, recon).
    EncInterCode = 18,
    /// Coding an intra MB (`encode_mb`: mode search + T/Q + entropy + recon).
    EncIntraCode = 19,
    /// Per-frame encoder finalize (deblock-info build + RefFrame handoff).
    EncFinal = 20,
    /// Forward transform + quantize (+ recon dequant/idct) inside MB coding — INFO (nested).
    EncTq = 21,
    /// CAVLC residual bit-writing — INFO (nested inside Enc*Code).
    EncWrite = 22,
    /// The skip free-check's forward T/Q proof — INFO (nested inside EncSkip).
    EncFree = 23,
    /// Per-frame encoder PREP before the macroblock loop (FrameEncoder grid
    /// allocation, source copy, AQ/mb-tree QP maps, content pre-passes) — INFO
    /// (nested; contains `EncSource`).
    EncPrep = 24,
    /// NAL assembly of the coded slice: RBSP emulation-prevention scan + the
    /// Annex-B copy into the output buffer. A full byte-wise pass over the frame's
    /// bitstream, invisible until it was named.
    EncNal = 25,
    /// The whole macroblock double-loop — INFO (nested; contains the Enc* per-MB
    /// stages). `EncMbLoop − Σ(per-MB stages)` is the per-MB GLUE, the part of the
    /// old `mgmt/other` that lives between the named steps.
    EncMbLoop = 26,
    /// Per-MB motion-vector predictor / neighbour-candidate build in the encode
    /// loop (`mv_neighbors_block`) — INFO (nested inside `EncMbLoop`, part of the
    /// per-MB glue being decomposed).
    EncMvPred = 27,
    /// CAVLC entropy EMIT for a planned macroblock (`emit_inter_cavlc` and the
    /// intra equivalent): mb_type, ref_idx, mvd, cbp, mb_qp_delta and every
    /// residual block. This sits OUTSIDE `plan_inter_mb`, so before it was named
    /// the whole encoder-side entropy coder was landing in `mgmt/other`.
    EncEmit = 28,
    /// Per-MB boundary-strength derivation done INSIDE the encode loop — INFO
    /// (nested in `EncMbLoop`). Named so the work moved out of deblocking can be
    /// priced at its new location rather than inferred from stage deltas.
    EncBs = 29,
    /// Adaptive-quantization per-MB QP map (`aq_qp_map`) — a full-frame variance
    /// pass that runs every frame because AQ is on by default. INFO (nested in
    /// `EncPrep`).
    EncAq = 30,
    /// Sub-pel prediction served from the cached half-pel planes (`hpel_block`) —
    /// INFO (nested in `EncMe`). Named because the plane cache MOVED the sub-pel
    /// motion-search work out of `inter-mc`, and unnamed work is invisible work.
    MeHpel = 31,
    /// The motion search's SATD/SAD cost metric itself — INFO (nested in `EncMe`).
    MeCost = 32,
    /// Building the cached half-pel planes for one reference picture — INFO.
    MeHpelBuild = 33,
    /// ME: coarse-to-fine full-pel diamond — INFO (nested in `EncMe`).
    MeDiamond = 34,
    /// ME: sub-pel (half + quarter) refinement rings — INFO (nested in `EncMe`).
    MeSubpel = 35,
    /// ME: the stalled-diamond wide rescue grid — INFO (nested in `EncMe`).
    MeRescue = 36,
    /// Wraps the whole `decode()`/`encode()` call — the denominator.
    Total = 37,
    // --- H-32: decomposition of the decoder's per-MB residue (INFO, nested;
    // indexes past `Total` are excluded from dump()'s residue sum) ---
    /// CABAC P-inter MB branch, whole body (parse + MC + recon nested inside).
    DecMbP = 38,
    /// CABAC B MB branch, whole body.
    DecMbB = 39,
    /// CABAC intra path (mode parse + residual + recon), whole body.
    DecMbI = 40,
    /// B-direct derivation + MC (`decode_b_direct`) — INFO (nested in DecMbB).
    DecBDirect = 41,
    /// Bi-/uni-pred region MC + blend (`b_mc`) — INFO (nested in DecMbB).
    DecBMc = 42,
    /// Per-picture + per-slice setup: FrameDecoder grids, slice neighbour vecs.
    DecSetup = 43,
    /// B-direct DERIVATION only (neighbours, ref pick, colZero gather) — INFO.
    DecBDeriv = 44,
    /// B motion-grid commit (`b_set_motion`) — INFO (nested in DecMbB).
    DecBSet = 45,
    // --- H-38: decomposition of `b_mc` (INFO, nested in DecBMc) ---
    /// Implicit-weight derivation (POC math + the integer divide).
    DecBWeights = 46,
    /// Luma MC calls inside `b_mc`.
    DecBLuma = 47,
    /// Chroma MC calls inside `b_mc`.
    DecBChroma = 48,
    /// The bi-pred blend / uni-pred row copies out of the staging buffers.
    DecBBlend = 49,
    // --- Entropy-stage decomposition (INFO, nested inside `Entropy`) — added for
    // the CABAC/CAVLC diagnosis once sampled profiling made sub-scopes affordable.
    /// CABAC residual: the coded_block_flag bin + its neighbour-context read.
    EntCbf = 50,
    /// CABAC residual: the significance-map loop (sig + last bins).
    EntSig = 51,
    /// CABAC residual: the level loop (one/abs/UEG0 bins + sign bypass).
    EntLvl = 52,
    /// CAVLC residual: the coeff_token VLC read.
    CavTok = 53,
    /// CAVLC residual: trailing-one signs + level prefix/suffix reads.
    CavLvl = 54,
    /// CAVLC residual: total_zeros + run_before reads and the scatter to scan order.
    CavRun = 55,
    /// Per-MB neighbour/state cache shuffling in the CABAC MB branches: the
    /// mvd/ref 30-entry cache build, the 48-entry nzc cache build, and the mb_nzc
    /// write-back — the state plumbing BETWEEN parse steps (INFO, nested).
    DecStateCache = 56,
    /// `add_inter_residual` whole body (INFO, nested): contains Dequant /
    /// Reconstruct / Scatter leaves; body minus leaves = the residual-add glue
    /// (nnz re-derivation scans, un-scan, pred gathers, loop skeleton).
    DecResidAdd = 57,
    /// The P-body MC staging block (INFO, nested): contains InterMc + PredBuf
    /// leaves; block minus leaves = rect-ladder + staging-buffer cost.
    DecMcStage = 58,
    /// `pack_frame_into` — the per-frame MbPack build (INFO, nested in Deblock).
    DebPack = 59,
    /// Per-MB bS derivation inside `filter_frame`: kind gate, predicates,
    /// packed/tile/blind derivation, bs materialization (INFO, nested in Deblock).
    DebDerive = 60,
    /// Annex-B start-code scan (`split_annex_b`) — a full byte-wise pass over the
    /// WHOLE stream plus two Vec allocations. Entered ~once per access unit, so it
    /// is in the low-call-count regime the profiler measures reliably.
    DecNalSplit = 61,
    /// RBSP emulation-prevention removal (`emulation_unprevent`) — a second full
    /// byte-wise pass, one bounds-checked `push` per byte, into a fresh Vec per NAL.
    DecRbsp = 62,
    /// The 11 per-slice `vec![...; total_mbs]` neighbour-state allocations at the top
    /// of `decode_slice_cabac_inner` (mvd grids alone are 64 B/MB each, twice).
    DecSliceAlloc = 63,
    /// The whole CABAC macroblock loop — INFO (nested; contains dec-mb-P/B/I).
    /// `DecMbLoop - sum(dec-mb-*)` is the per-MB loop GLUE outside the MB bodies.
    DecMbLoop = 64,
    /// `row_hook` — on every MB after decode. Mid-row calls early-out (no scope)
    /// unless `RS_H264_ROWHOOK_EAGER=1`. On row crossings: bS derive + filter /
    /// EDC row handoff. INFO (nested in DecMbLoop; contains deb:derive + deblock).
    ///
    /// Call COUNT with the early-out ≈ MB rows/picture (exact). Quote ms only with
    /// that count; per-MB eager scoping inflated both calls and self-tax.
    DecRowHook = 65,
    /// B_Skip forced-continuation prefix (`b_skip_hot`) — the bitmap-forced
    /// zero-bi path that never gathers neighbours. INFO (nested in dec-mb-B).
    BSkipHot = 66,
    /// B_Skip cold path (`decode_b_skip`) — neighbour gather + spatial-direct
    /// derivation + region. INFO (nested in dec-mb-B).
    BSkipCold = 67,
    /// Deferred B/P span commit + BANDED RECON (`bz_flush_slow`/`pz_flush_slow`).
    /// The pixels of a skipped macroblock are written HERE, not in its own
    /// macroblock body. INFO — it fires under whichever scope hit the flush.
    BSpanRecon = 68,
    /// `parse_mb_type_b_cabac`. INFO (nested in dec-mb-B).
    BTypeParse = 69,
    /// The two `fill!` neighbour-cache builds (L0 + L1) on a non-skip B MB.
    /// INFO (nested in dec-mb-B).
    BFillCache = 70,
    /// B motion parse: sub_mb_type + mvd + per-partition predict/commit.
    /// INFO (nested in dec-mb-B).
    BMvdParse = 71,
    /// B residual: cbp + coefficient parse + add_inter_residual.
    /// INFO (nested in dec-mb-B).
    BResid = 72,
}

/// Number of buckets.
pub const N: usize = 73;

#[cfg(feature = "profile")]
mod imp {
    use super::{Stage, N};
    use crate::atomic::AtomicU64;
    use core::sync::atomic::Ordering;
    use std::sync::Mutex;
    use std::time::Instant;

    /// Index of the first non-`Total` stage — the residue sum runs `0..SUB`.
    const SUB: usize = Stage::Total as usize;

    /// A cheap monotonic tick. On x86_64 this is `rdtsc` (~5-10 ns, ~3-5× cheaper
    /// than `Instant::now()` = QueryPerformanceCounter ~20-30 ns on Windows), which
    /// is what dominated the profiler's own overhead (~1M scope entries × 2 calls).
    /// Buckets accumulate *ticks*; `dump()` converts to ns via a run-length TSC
    /// calibration (invariant TSC → ticks are wall-time-proportional). Elsewhere we
    /// fall back to `Instant` nanos so the profiler still builds cross-arch.
    #[cfg(target_arch = "x86_64")]
    #[inline(always)]
    fn ticks() -> u64 {
        // SAFETY: `_rdtsc` is a pure timestamp read with no memory effects; it is
        // `unsafe` only because it is a target intrinsic. Reordering is immaterial to
        // coarse scope timing. Compiled only under `feature = "profile"` (dev tool).
        unsafe { core::arch::x86_64::_rdtsc() }
    }
    #[cfg(not(target_arch = "x86_64"))]
    #[inline(always)]
    fn ticks() -> u64 {
        use std::sync::OnceLock;
        static EPOCH: OnceLock<Instant> = OnceLock::new();
        EPOCH.get_or_init(Instant::now).elapsed().as_nanos() as u64
    }

    /// (wall-clock, tick-count) sampled at `reset()` — the calibration anchor read at
    /// `dump()` to recover ns-per-tick. Touched twice per run, so its `Mutex` cost is
    /// irrelevant next to the per-scope path.
    static ANCHOR: Mutex<Option<(Instant, u64)>> = Mutex::new(None);

    const NAMES: [&str; N] = [
        "entropy/cavlc",
        "intra-pred",
        "inter-mc",
        "reconstruct",
        "deblock",
        "dequant",
        "scatter(store)",
        "pred-buf copy",
        "mv+grid",
        "neighbors",
        "skip-recon",
        "finalize",
        "syntax-parse",
        "dpb-clone",
        "enc-source-copy",
        "enc-skip-check",
        "enc-me(best_part)",
        "enc-intra-cost",
        "enc-inter-code",
        "enc-intra-code",
        "enc-finalize",
        "enc-T/Q(nested)",
        "enc-cavlc-write(nested)",
        "enc-skip-freecheck(nested)",
        "enc-prep(nested)",
        "enc-nal-assembly",
        "enc-mb-loop(nested)",
        "enc-mvpred(nested)",
        "enc-cavlc-emit",
        "enc-bs-derive(nested)",
        "enc-aq-map(nested)",
        "me-hpel-read(nested)",
        "me-cost/satd(nested)",
        "me-hpel-BUILD(nested)",
        "me-diamond(nested)",
        "me-subpel(nested)",
        "me-rescue(nested)",
        "TOTAL",
        "dec-mb-P(nested)",
        "dec-mb-B(nested)",
        "dec-mb-I(nested)",
        "b-direct(nested)",
        "b-mc(nested)",
        "dec-setup",
        "b-deriv(nested)",
        "b-setmotion(nested)",
        "b:weights(nested)",
        "b:luma-mc(nested)",
        "b:chroma-mc(nested)",
        "b:blend(nested)",
        "ent:cbf(nested)",
        "ent:sigmap(nested)",
        "ent:levels(nested)",
        "cav:token(nested)",
        "cav:levels(nested)",
        "cav:runs(nested)",
        "state-cache(nested)",
        "resid-add(nested)",
        "mc-stage(nested)",
        "deb:pack(nested)",
        "deb:derive(nested)",
        "dec-nal-split",
        "dec-rbsp-unescape",
        "dec-slice-alloc",
        "dec-mb-loop(nested)",
        "dec-row-hook(nested)",
        "b:skip-hot(nested)",
        "b:skip-cold(nested)",
        "b:span-recon(nested)",
        "b:type-parse(nested)",
        "b:fill-cache(nested)",
        "b:mvd-parse(nested)",
        "b:resid(nested)",
    ];

    static NS: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];
    static CALLS: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];

    /// SAMPLED PROFILING — the fix for the profiler's own tax.
    ///
    /// The scope guard is an rdtsc pair. At ~20M per-macroblock scopes that tax was
    /// measured at **1.32-1.43x of whole decode**, which is why every per-MB stage
    /// share in this codec has been untrustworthy and why the campaign fell back to
    /// ablation (which can only price stages someone already put a knob on).
    ///
    /// Fix: only TIME one call in `RS_H264_PROF_SAMPLE` (default 1 = time every call,
    /// preserving the old behaviour exactly). Sampled cycles are scaled back up by the
    /// sampling period, so the SHARE each stage reports is unbiased — a stage that
    /// takes x% of time is entered proportionally often, so timing 1-in-N calls
    /// estimates the same x% with 1/N the tax. Call COUNTS stay exact (counting is a
    /// single relaxed add, not an rdtsc pair, so it is not the expensive part).
    ///
    /// This is the standard statistical-profiling trade: precision for a smaller
    /// probe. At N=64 the tax drops ~64x while a stage above ~1% still lands within a
    /// few percent relative over 20M+ entries.
    pub struct Guard {
        stage: usize,
        start: u64,
        /// 0 = not timed. Otherwise the weight this sample carries: 1 for an exactly
        /// timed call, N for a sampled one.
        scale: u64,
    }

    /// Calls below this are ALWAYS timed exactly, whatever the sampling period.
    ///
    /// Two reasons. (a) The tax is proportional to call count, so a stage entered a
    /// few thousand times costs nothing to time in full — sampling it buys no speed.
    /// (b) Sampling a low-count stage is statistically worthless: `Total` is entered
    /// once per FRAME (60 calls on a 60-frame clip), so at N=64 it would estimate the
    /// entire denominator from ONE sample. Since every share is a ratio to `Total`,
    /// that one bad estimate would skew every share on the table — which is exactly
    /// what the first N=64 run showed (every stage share rose together).
    const EXACT_PREFIX: u64 = 8192;

    /// Sampling period. 1 = time everything (previous behaviour, and the default so
    /// no existing measurement silently changes meaning).
    pub(crate) static SAMPLE_N: crate::atomic::AtomicU64 = crate::atomic::AtomicU64::new(0);

    /// Sampling period, ROUNDED UP TO A POWER OF TWO so the selection test is a mask
    /// (1 cycle) and not a `u64` division (~20-40 cycles — as expensive as the rdtsc
    /// it is meant to avoid, which would make the whole scheme pointless).
    #[inline(always)]
    fn sample_period() -> u64 {
        match SAMPLE_N.load(Ordering::Relaxed) {
            0 => {
                let n = crate::knob("RS_H264_PROF_SAMPLE")
                    .and_then(|v| v.parse::<u64>().ok())
                    .filter(|v| *v >= 1)
                    .unwrap_or(1)
                    .next_power_of_two();
                SAMPLE_N.store(n, Ordering::Relaxed);
                n
            }
            n => n,
        }
    }

    impl Drop for Guard {
        #[inline]
        fn drop(&mut self) {
            if self.scale != 0 {
                let d = ticks().wrapping_sub(self.start);
                // Weight by this sample's scale so the bucket estimates TOTAL cycles.
                // The first EXACT_PREFIX calls carry weight 1 (they were all timed);
                // the tail carries weight N. Sum = exact prefix + unbiased estimate of
                // the remainder, so a short-running stage degrades to "fully timed"
                // rather than to "one sample times N".
                NS[self.stage].fetch_add(d.wrapping_mul(self.scale), Ordering::Relaxed);
            }
            CALLS[self.stage].fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Descent E: the raw tick source, for census modules that accumulate cycles into
    /// their own buckets rather than a `Stage`.
    #[inline(always)]
    pub fn tick() -> u64 {
        ticks()
    }

    pub fn scope(s: Stage) -> Guard {
        let n = sample_period();
        let c = CALLS[s as usize].load(Ordering::Relaxed);
        // Pick 1-in-N by HASHING the call index, not by striding it.
        //
        // A plain `c % N` stride aliases: decode work is intensely periodic (blocks
        // per macroblock, macroblocks per row), so a power-of-two stride can lock onto
        // the same position in that pattern every time and systematically sample only
        // the cheap — or only the expensive — calls. Multiplying by the 64-bit golden
        // ratio and taking high bits decorrelates the selection from any workload
        // period, giving an unbiased 1-in-N at ~4 cycles (mul, shift, and, test).
        const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;
        let scale = if n == 1 || c < EXACT_PREFIX {
            1
        } else if (c.wrapping_mul(GOLDEN) >> 32) & (n - 1) == 0 {
            n
        } else {
            0
        };
        Guard {
            stage: s as usize,
            start: if scale != 0 { ticks() } else { 0 },
            scale,
        }
    }

    /// Zero all buckets and sample the calibration anchor — call before a clean run.
    pub fn reset() {
        for a in NS.iter().chain(CALLS.iter()) {
            a.store(0, Ordering::Relaxed);
        }
        *ANCHOR.lock().unwrap() = Some((Instant::now(), ticks()));
    }

    /// Human-readable name for stage index `i` (`SUB` = the `TOTAL` row).
    pub fn name(i: usize) -> &'static str {
        NAMES.get(i).copied().unwrap_or("?")
    }

    /// One calibrated reading: `(ms, calls)` per stage index `0..N` (index `SUB` is
    /// `Total`). Buckets hold `rdtsc` cycles; ns/tick is recovered from the reset→now
    /// anchor (elapsed wall / elapsed cycles — invariant TSC, so cycles are wall-
    /// proportional). Lets a driver run many passes and take a per-stage median.
    pub fn snapshot() -> [(f64, u64); N] {
        let load = |i: usize| NS[i].load(Ordering::Relaxed);
        let ns_per_tick = ANCHOR
            .lock()
            .unwrap()
            .map(|(t0, c0)| {
                let wall = t0.elapsed().as_nanos() as f64;
                let cyc = ticks().wrapping_sub(c0) as f64;
                if cyc > 0.0 {
                    wall / cyc
                } else {
                    1.0
                }
            })
            .unwrap_or(1.0);
        let mut out = [(0.0f64, 0u64); N];
        for (i, o) in out.iter_mut().enumerate() {
            *o = (
                load(i) as f64 * ns_per_tick / 1e6,
                CALLS[i].load(Ordering::Relaxed),
            );
        }
        out
    }

    /// Print the per-stage breakdown (does not reset).
    pub fn dump() {
        let s = snapshot();
        let total = s[SUB].0.max(1e-9);
        let sub_sum: f64 = (0..SUB).map(|i| s[i].0).sum();
        let mgmt = (total - sub_sum).max(0.0);
        let pct = |ms: f64| 100.0 * ms / total;

        eprintln!("\n--- decode stage profile (decode() wall = {total:.1} ms) ---");
        for i in 0..SUB {
            eprintln!(
                "  {:<15} {:>8.1} ms  {:>5.1}%   ({} calls)",
                NAMES[i],
                s[i].0,
                pct(s[i].0),
                s[i].1,
            );
        }
        eprintln!(
            "  {:<15} {:>8.1} ms  {:>5.1}%   <- the OTHER bucket: unnamed decode glue",
            "mgmt/other",
            mgmt,
            pct(mgmt),
        );
        eprintln!("  {:<15} {:>8.1} ms  100.0%", NAMES[SUB], total);
        // Attribute OTHER using INFO scopes (nested; shares can overlap parents).
        // These are the deployment-signal names for the 1T residue campaign.
        let info = |st: Stage| s[st as usize];
        let loop_ms = info(Stage::DecMbLoop).0;
        let mb_bodies = info(Stage::DecMbP).0 + info(Stage::DecMbB).0 + info(Stage::DecMbI).0;
        let loop_glue = (loop_ms - mb_bodies).max(0.0);
        eprintln!("--- OTHER attribution (INFO scopes; parents include children) ---");
        eprintln!(
            "  dec-mb-loop glue≈ {:>8.1} ms  {:>5.1}%   (loop {:.1} − P/B/I bodies {:.1})",
            loop_glue,
            pct(loop_glue),
            loop_ms,
            mb_bodies
        );
        let row_hook = info(Stage::DecRowHook).0;
        let deblock = s[Stage::Deblock as usize].0;
        let syntax = s[Stage::Syntax as usize].0;
        let inter = s[Stage::InterMc as usize].0;
        let entropy = s[Stage::Entropy as usize].0;
        // Residue naming: top-level OTHER is work NOT wrapped by stages 0..Total.
        // INFO scopes after Total do not shrink OTHER — they NAME pieces of it.
        // Reconcile the big pieces that live in OTHER (nested times overlap).
        eprintln!("--- OTHER named (why ~45% is not a mystery kernel) ---");
        eprintln!(
            "  top-level leaves already subtract: entropy {:.1}%  inter-mc {:.1}%  deblock {:.1}%  syntax {:.1}%",
            pct(entropy),
            pct(inter),
            pct(deblock),
            pct(syntax),
        );
        eprintln!(
            "  in-OTHER orchestration (INFO; overlaps leaves): row-hook {:.1}%  loop-glue {:.1}%  (glue−hook≈{:.1}%)",
            pct(row_hook),
            pct(loop_glue),
            pct((loop_glue - row_hook).max(0.0)),
        );
        eprintln!(
            "  in-OTHER B glue (INFO nested): b-mc {:.1}%  b-direct {:.1}%  blend {:.1}%  setmot {:.1}%  deriv {:.1}%",
            pct(info(Stage::DecBMc).0),
            pct(info(Stage::DecBDirect).0),
            pct(info(Stage::DecBBlend).0),
            pct(info(Stage::DecBSet).0),
            pct(info(Stage::DecBDeriv).0),
        );
        eprintln!(
            "  in-OTHER resid/mc-stage (INFO): resid-add {:.1}%  mc-stage {:.1}%  — remainder ≈ timer tax + tiny parse glue",
            pct(info(Stage::DecResidAdd).0),
            pct(info(Stage::DecMcStage).0),
        );
        for st in [
            Stage::DecMbB,
            Stage::DecMbP,
            Stage::DecMbI,
            Stage::DecBDirect,
            Stage::DecBMc,
            Stage::DecRowHook,
            Stage::DecResidAdd,
            Stage::DecMcStage,
            Stage::DecSetup,
            Stage::DecSliceAlloc,
            Stage::DecNalSplit,
            Stage::DecRbsp,
        ] {
            let (ms, calls) = info(st);
            if ms > 0.05 || calls > 0 {
                eprintln!(
                    "  {:<18} {:>8.1} ms  {:>5.1}%   ({} calls)",
                    NAMES[st as usize],
                    ms,
                    pct(ms),
                    calls
                );
            }
        }
    }
}

#[cfg(not(feature = "profile"))]
mod imp {
    use super::{Stage, N};

    /// No-op guard (ZST) — elided in release.
    pub struct Guard;

    #[inline(always)]
    pub fn scope(_s: Stage) -> Guard {
        Guard
    }
    #[inline(always)]
    pub fn reset() {}
    #[inline(always)]
    pub fn dump() {}
    #[inline(always)]
    pub fn snapshot() -> [(f64, u64); N] {
        [(0.0, 0); N]
    }
    #[inline(always)]
    pub fn name(_i: usize) -> &'static str {
        ""
    }
}

#[cfg(feature = "profile")]
pub use imp::tick;
pub use imp::{dump, name, reset, scope, snapshot, Guard};
