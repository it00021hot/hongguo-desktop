//! SIMD-arm introspection + the measurement-knob audit (2026-08-27, sites 3/5-9
//! of the SIMD-reachability audit).
//!
//! Two defect classes motivated this module, both with a track record here:
//!
//! 1. **The arm is invisible.** A scalar build is byte-identical to an accel
//!    build, so every correctness gate passes and only the wall clock knows —
//!    the 0.11.0 chroma-deblock bug shipped precisely because nothing ever
//!    said which arm a binary carried. [`simd_arms`] names the arm at runtime.
//! 2. **A lingering env knob silently reverts a shipped win.** The codebase
//!    carries ~30 `RS_H264_*`/`RFF_*` measurement knobs; several pin scalar
//!    oracle arms, disable fast paths, add verification work — and one class
//!    (`RFF_ABL_*`) makes the OUTPUT WRONG while set. A knob exported in a
//!    bench shell or CI YAML outlives the experiment it served.
//!    [`active_knobs`] surfaces every one that is set.
//!
//! Deliberately no polarity logic here: the audit reports a knob as ACTIVE
//! whenever its variable is present, with its raw value, and leaves the
//! interpretation to the classification string. Re-parsing each knob's
//! polarity in a second place is exactly the instrument-fork hazard this
//! codebase has already paid for once — the site that reads the knob stays
//! the only parser.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// The kernel arm this build carries, resolved at runtime. One line, stable
/// shape, safe to print from any driver.
pub fn simd_arms() -> String {
    #[cfg(accel)]
    {
        #[cfg(target_arch = "x86_64")]
        {
            #[cfg(not(feature = "std"))]
            {
                return "accel x86-64: arms fixed at build time (no runtime detection without std)"
                    .to_string();
            }
            #[cfg(feature = "std")]
            return if std::arch::is_x86_feature_detected!("avx2") {
                "accel x86-64: SSE2 baseline + AVX2 (all kernel arms live)".to_string()
            } else {
                // Reachable only on a baseline (non target-cpu=x86-64-v3)
                // build running on a pre-AVX2 CPU: the SSE2 kernels run, the
                // AVX2-only helpers (packed-bS masks, mb_uniform, w16 MC
                // second tiers) fall back per call.
                "accel x86-64: SSE2 only — AVX2 ABSENT, AVX2-gated kernels fall back".to_string()
            };
        }
        #[cfg(target_arch = "aarch64")]
        {
            return "accel aarch64: NEON".to_string();
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            // build.rs never sets `accel` off x86-64/aarch64; if this prints,
            // the cfg chain regressed.
            return "accel cfg set on an arch without kernels (cfg regression?)".to_string();
        }
    }
    #[cfg(not(accel))]
    {
        "SCALAR — built without the `asm` feature (no SIMD kernels compiled in)".to_string()
    }
}

/// Effect classes for the knob audit. UNLISTED knobs still get reported (the
/// scan is by prefix), just without a description — the list only adds context,
/// so a new knob is never invisible merely because nobody registered it.
#[cfg_attr(not(feature = "std"), allow(dead_code))]
const KNOB_CLASS: &[(&str, &str)] = &[
    (
        "RFF_ABL_RECON",
        "ABLATION — decoder OUTPUT IS WRONG while set",
    ),
    (
        "RFF_ABL_INTRA",
        "ABLATION — decoder OUTPUT IS WRONG while set",
    ),
    ("RFF_ABL_MC", "ABLATION — decoder OUTPUT IS WRONG while set"),
    (
        "RFF_ABL_DEBLOCK",
        "ABLATION — decoder OUTPUT IS WRONG while set",
    ),
    (
        "RFF_TQ_SCALAR",
        "pins ALL transform/quant dispatch to the scalar oracle arm",
    ),
    (
        "RS_H264_QPEL_COMPOSE",
        "=1 reverts the fused qpel kernels to the compose path",
    ),
    (
        "RS_H264_DEQUANT_AVX2",
        "opt-in AVX2 dequant (measured null; default scalar)",
    ),
    (
        "RFF_HPEL_AVX2",
        "=0 pins the pre-kernel hpel-build path (encoder ME)",
    ),
    ("RFF_HPEL_FUSED", "hpel builder A/B arm (encoder ME)"),
    ("RFF_HPEL_PAD", "hpel plane border sweep knob (encoder ME)"),
    (
        "RS_H264_BS_PACKED",
        "=0 disables the packed-bS fast arm (+3-7% win)",
    ),
    ("RS_H264_NO_MBKIND", "disables deblock kind-routing"),
    (
        "RS_H264_DEBLOCK_BRANCHY",
        "selects the branchy deblock fallback arm",
    ),
    ("RS_H264_BS_TWOPASS", "bS scan-predicate A/B arm"),
    ("RS_H264_BS_PRE", "bS precompute A/B arm"),
    (
        "RS_H264_NO_SKIPBAND",
        "=1 disables the mb_skip_run band coalescer",
    ),
    (
        "RS_H264_NO_RUNMV",
        "=1 disables the P_Skip run-theorem forced-(0,0) branch",
    ),
    ("RS_H264_NO_SKIPFP", "=1 disables the P_Skip fast paths"),
    (
        "RS_H264_NO_BSKIPFAST",
        "=1 disables the B_Skip zero-bi fast path",
    ),
    (
        "RS_H264_DOUBLE_RECON",
        "=1 runs every pixel reconstruction TWICE (ablation)",
    ),
    ("RS_H264_VERIFY_MBKIND", "adds per-MB verification work"),
    ("RS_H264_VERIFY_PACKED", "adds per-MB verification work"),
    ("RS_H264_NO_POOL", "disables DPB plane pooling"),
    (
        "RS_H264_FAT_SLICE",
        "=1 allocates B-only grids on every slice (A/B)",
    ),
    (
        "RS_H264_EDC",
        "=0 disables entropy-decoupled reconstruction",
    ),
    ("RS_H264_EDC_MT", "recon worker-thread arm (default inline)"),
    ("RS_H264_EDC_BOUND", "EDC queue bound override"),
    ("RS_H264_EDC_STATS", "EDC counter telemetry"),
    (
        "RS_H264_BATCH",
        "=0 per-MB job sends (latency mode; default row batches)",
    ),
    (
        "RS_H264_NORES",
        "=0 restores full-payload no-residual jobs (A/B)",
    ),
    ("RS_H264_ROWDB", "=0 disables row-deferred deblocking"),
    (
        "RS_H264_ROWHOOK_EAGER",
        "=1 restores the per-MB row-hook body (A/B)",
    ),
    ("RS_H264_ROW_PROGRESS", "frame-MT row progress publishing"),
    ("RS_H264_ROW_PUB", "frame-MT row publishing arm"),
    ("RS_H264_FRAME_THREADS", "frame-MT worker count"),
    (
        "RS_H264_DIRECT_MEMO",
        "=0 rewalks spatial-direct neighbours every 8x8",
    ),
    ("RS_H264_KIND_LOADS", "kind-routing load-count telemetry"),
    ("RS_H264_ROUTE_DUMP", "GATE-1 router telemetry dump"),
    ("RS_H264_PROF_SAMPLE", "profiler sampling divisor"),
];

/// Every `RS_H264_*` / `RFF_*` variable present in the environment, with its
/// value and effect class. Empty means the process runs the shipped defaults.
#[cfg(not(feature = "std"))]
pub fn active_knobs() -> Vec<(String, String, &'static str)> {
    Vec::new()
}
/// Every `RS_H264_*` / `RFF_*` variable present in the environment, with its
/// value and effect class. Empty means the process runs the shipped defaults.
#[cfg(feature = "std")]
pub fn active_knobs() -> Vec<(String, String, &'static str)> {
    let mut v: Vec<(String, String, &'static str)> = std::env::vars_os()
        .filter_map(|(k, val)| {
            let k = k.to_string_lossy().into_owned();
            if !(k.starts_with("RS_H264_") || k.starts_with("RFF_")) {
                return None;
            }
            let class = KNOB_CLASS
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, c)| *c)
                .unwrap_or("unregistered knob — read its site before trusting any number");
            Some((k, val.to_string_lossy().into_owned(), class))
        })
        .collect();
    v.sort();
    v
}

/// Standard stderr banner for drivers (CLI, bench harnesses, profile tools):
/// one arm line always, plus a loud block iff any knob is live. Every number a
/// harness records should have this above it in the log.
pub fn print_arm_banner(tool: &str) {
    eprintln!("{tool}: kernel arm = {}", simd_arms());
    let knobs = active_knobs();
    if !knobs.is_empty() {
        eprintln!(
            "{tool}: *** {} RS_H264_/RFF_ knob(s) ACTIVE — this run is NOT the shipped default ***",
            knobs.len()
        );
        for (k, v, class) in &knobs {
            eprintln!("{tool}:   {k}={v}  [{class}]");
        }
    }
}
