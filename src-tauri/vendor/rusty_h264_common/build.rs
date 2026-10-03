//! Defines the internal accel cfgs from the `asm` feature and the target arch:
//!
//! * `accel` — the `asm` feature is on AND `rusty_h264-accel`'s **portable**
//!   kernel modules exist for this target (x86-64 SSE2/AVX2 or aarch64 NEON):
//!   deblock_simd, luma_mc, chroma_mc, transform_quant, satd_sad, intra_pred.
//! * `accel_x86` — additionally, the x86-64-only `x86_asm` module is available
//!   (MeCtx, sad_x4/satd_x4/satd_avg/satd_avg_x4/satd_x4p, hpel_fused). Call
//!   sites for THOSE kernels gate on `accel_x86`, never plain `accel`.
//!
//! History: `accel` used to require x86-64 because the kernels were vendored
//! openh264 NASM. The NASM was fully ripped on 2026-08-12 and the portable
//! modules carry NEON twins — but this consumer-side gate still said
//! x86-only, so every NEON kernel was compiled and unreachable on ARM
//! (docs/plans/inline-execution.md H1). Split into the two cfgs 2026-08-26.
//! On any other architecture both cfgs stay unset and the codec runs its
//! pure-Rust scalar path — so e.g. `rff` still builds anywhere without nasm.
fn main() {
    // Declare the custom cfgs so `unexpected_cfgs` (Rust 1.80+) stays quiet; older
    // Cargo ignores the lines (and lacks the lint anyway).
    println!("cargo::rustc-check-cfg=cfg(accel)");
    println!("cargo::rustc-check-cfg=cfg(accel_x86)");
    let asm = std::env::var_os("CARGO_FEATURE_ASM").is_some();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH");
    let x86_64 = arch.as_deref() == Ok("x86_64");
    let aarch64 = arch.as_deref() == Ok("aarch64");
    if asm && (x86_64 || aarch64) {
        // Single-colon form: understood by every Cargo version.
        println!("cargo:rustc-cfg=accel");
    }
    if asm && x86_64 {
        println!("cargo:rustc-cfg=accel_x86");
    }
}
