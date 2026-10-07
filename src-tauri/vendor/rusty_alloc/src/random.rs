//! ChaCha8 CSPRNG (mirrors `random.c`): per-heap streams for free-list
//! encoding keys, guarded-object sampling, and page-start randomization.
//!
//! Self-contained (no rand crate — the allocator cannot depend on code that
//! allocates). Seeded from OS entropy where available, else from a mix of
//! clock, addresses and thread id; the seed path is documented per platform
//! so `secure` builds can state what they rest on.

use core::sync::atomic::{AtomicUsize, Ordering};

/// A ChaCha8 stream. Not `Sync`: each heap owns one (no sharing, no locks).
pub struct Random {
    state: [u32; 16],
    out: [u32; 16],
    used: usize,
}

#[inline]
fn qround(x: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    x[a] = x[a].wrapping_add(x[b]);
    x[d] = (x[d] ^ x[a]).rotate_left(16);
    x[c] = x[c].wrapping_add(x[d]);
    x[b] = (x[b] ^ x[c]).rotate_left(12);
    x[a] = x[a].wrapping_add(x[b]);
    x[d] = (x[d] ^ x[a]).rotate_left(8);
    x[c] = x[c].wrapping_add(x[d]);
    x[b] = (x[b] ^ x[c]).rotate_left(7);
}

impl Random {
    /// Empty (unseeded) stream — call [`reseed`](Self::reseed) before use.
    pub const fn new() -> Random {
        Random {
            state: [0; 16],
            out: [0; 16],
            used: 16,
        }
    }

    /// Seed from `key` (32 bytes as 8 words) and a stream id.
    pub fn seed_from(&mut self, key: [u32; 8], stream: u64) {
        // "expand 32-byte k"
        self.state[0] = 0x6170_7865;
        self.state[1] = 0x3320_646e;
        self.state[2] = 0x7962_2d32;
        self.state[3] = 0x6b20_6574;
        self.state[4..12].copy_from_slice(&key);
        self.state[12] = 0; // counter
        self.state[13] = 0;
        self.state[14] = stream as u32;
        self.state[15] = (stream >> 32) as u32;
        self.used = 16;
    }

    /// Seed from OS entropy plus environmental mixing.
    pub fn reseed(&mut self) {
        let mut key = [0u32; 8];
        if !os_entropy(&mut key) {
            // Fallback mixing: clock, a stack address, a heap-ish address,
            // thread id, and a global counter (documented weaker path).
            // `AtomicUsize`, not `AtomicU64`: this is a seed-mixing counter,
            // not a value with a width contract, and 32-bit RISC-V / Xtensa
            // have no 64-bit atomic (P3 of `docs/plans/small-metal.md`). The
            // constant is the golden ratio truncated to the target's word.
            const GOLDEN: usize = 0x9E37_79B9_usize;
            static COUNTER: AtomicUsize = AtomicUsize::new(GOLDEN);
            let stack = core::ptr::from_ref(&key) as usize as u64;
            let mut acc = crate::prim::clock_now()
                ^ stack.rotate_left(17)
                ^ (crate::prim::thread_id() as u64).rotate_left(33)
                ^ COUNTER.fetch_add(GOLDEN, Ordering::Relaxed) as u64;
            for k in key.iter_mut() {
                // splitmix64 step
                acc = acc.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut z = acc;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                *k = ((z ^ (z >> 31)) & 0xFFFF_FFFF) as u32;
            }
        }
        let stream = crate::prim::thread_id() as u64;
        self.seed_from(key, stream);
    }

    fn refill(&mut self) {
        let mut x = self.state;
        for _ in 0..4 {
            // 8 rounds = 4 double-rounds
            qround(&mut x, 0, 4, 8, 12);
            qround(&mut x, 1, 5, 9, 13);
            qround(&mut x, 2, 6, 10, 14);
            qround(&mut x, 3, 7, 11, 15);
            qround(&mut x, 0, 5, 10, 15);
            qround(&mut x, 1, 6, 11, 12);
            qround(&mut x, 2, 7, 8, 13);
            qround(&mut x, 3, 4, 9, 14);
        }
        for (o, (xi, si)) in self.out.iter_mut().zip(x.iter().zip(self.state.iter())) {
            *o = xi.wrapping_add(*si);
        }
        // Bump the 64-bit counter.
        let (lo, carry) = self.state[12].overflowing_add(1);
        self.state[12] = lo;
        if carry {
            self.state[13] = self.state[13].wrapping_add(1);
        }
        self.used = 0;
    }

    /// Next 32 random bits.
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        if self.used >= 16 {
            self.refill();
        }
        let v = self.out[self.used];
        self.used += 1;
        v
    }

    /// A full `usize` of random bits.
    ///
    /// Two draws on a 64-bit target, one on a 32-bit target (wasm32 and
    /// friends): there `usize` is already filled by a single `u32`, and
    /// `hi << 32` would be a constant shift past the width — which rustc
    /// rejects outright, so this cannot be written width-agnostically.
    #[inline]
    pub fn next_usize(&mut self) -> usize {
        let lo = self.next_u32() as usize;
        #[cfg(target_pointer_width = "64")]
        {
            let hi = self.next_u32() as usize;
            (hi << 32) | lo
        }
        #[cfg(not(target_pointer_width = "64"))]
        {
            lo
        }
    }

    /// Uniform-ish in `[0, n)` (n > 0).
    ///
    /// Lemire's multiply-shift rather than `% n`. A modulo by a RUNTIME value
    /// is a `div` — 20-40 cycles, not pipelined — and it was the only division
    /// left in `Heap::try_guarded` and `guarded_set_sample_rate`. Taking the
    /// high half of a widening multiply is ONE instruction (`mul`), so unlike
    /// the reciprocal substitutions this file's sibling plan records as
    /// refuted, this is strictly fewer instructions as well as fewer cycles.
    ///
    /// The distribution is the same "uniform-ish" this already promised: both
    /// forms are biased for an `n` that does not divide the generator's range,
    /// and Lemire's bias is the smaller of the two. Both callers pick a
    /// sampling interval for the `guarded` feature, where the bias is
    /// immaterial.
    #[inline]
    pub fn below(&mut self, n: usize) -> usize {
        if n <= 1 {
            return 0;
        }
        ((self.next_usize() as u128 * n as u128) >> usize::BITS) as usize
    }
}

impl Default for Random {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(windows, not(miri)))]
fn os_entropy(key: &mut [u32; 8]) -> bool {
    // RtlGenRandom via SystemFunction036 is the classic mimalloc path; the
    // supported modern equivalent is BCryptGenRandom with the system RNG.
    use windows_sys::Win32::Security::Cryptography::{
        BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
    };
    // SAFETY: writing 32 bytes into our own buffer with the system RNG.
    let status = unsafe {
        BCryptGenRandom(
            core::ptr::null_mut(),
            key.as_mut_ptr().cast::<u8>(),
            (key.len() * 4) as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    status == 0
}

// `feature = "std"`, not just `unix`: `/dev/urandom` is reached through
// `std::fs`, so a `no_std` build on a unix HOST — which is what CI's `no_std`
// clippy step is — matched this arm and failed to resolve `std`. The fifth case
// again (P0 in `prim/mod.rs`, P3 here, P5 in `stats.rs`): a platform selection
// written when `std` was unconditional.
#[cfg(all(unix, feature = "std", not(miri)))]
fn os_entropy(key: &mut [u32; 8]) -> bool {
    // /dev/urandom: universally available and needs no libc feature probing.
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open("/dev/urandom") else {
        return false;
    };
    // SAFETY: reinterpreting our own [u32; 8] as bytes for the read.
    let buf =
        unsafe { core::slice::from_raw_parts_mut(key.as_mut_ptr().cast::<u8>(), key.len() * 4) };
    f.read_exact(buf).is_ok()
}

#[cfg(miri)]
fn os_entropy(_key: &mut [u32; 8]) -> bool {
    false // deterministic fallback under the interpreter
}

/// Wasm exposes no host RNG without JS bindings, so the documented weaker
/// fallback in [`Random::reseed`] is the only path. On `wasm32-unknown-unknown`
/// that fallback is weaker still: [`crate::prim::clock_now`] is a counter and
/// the thread id is a constant, leaving the stack address and the global
/// counter as the only varying inputs. Free-list encoding under `secure`
/// therefore has MUCH less entropy on wasm than on a native target — treat it
/// as corruption detection, not as an exploit-mitigation claim.
/// No OS, so no OS entropy — the fifth arm this four-way selection was
/// missing, exactly as `prim/mod.rs` was missing one (P0 bucket A). The
/// caller's documented fallback mixing (clock, stack address, thread id, a
/// global counter) is what a bare-metal build gets, and the same caveat the
/// wasm arm carries applies with more force: free-list encoding under `secure`
/// is corruption DETECTION here, not an exploit-mitigation claim. A part with
/// a hardware RNG should wire it through `prim` rather than weaken this.
#[cfg(all(
    not(miri),
    not(windows),
    not(target_arch = "wasm32"),
    // Bare metal, AND unix-without-`std`: both reach here because neither can
    // open `/dev/urandom`.
    not(all(unix, feature = "std"))
))]
fn os_entropy(_key: &mut [u32; 8]) -> bool {
    false
}

#[cfg(all(target_arch = "wasm32", not(miri)))]
fn os_entropy(_key: &mut [u32; 8]) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_is_deterministic_and_varied() {
        let mut a = Random::new();
        let mut b = Random::new();
        a.seed_from([1, 2, 3, 4, 5, 6, 7, 8], 42);
        b.seed_from([1, 2, 3, 4, 5, 6, 7, 8], 42);
        let xs: Vec<u32> = (0..64).map(|_| a.next_u32()).collect();
        let ys: Vec<u32> = (0..64).map(|_| b.next_u32()).collect();
        assert_eq!(xs, ys, "same seed must give the same stream");
        assert!(xs.windows(2).any(|w| w[0] != w[1]), "stream is constant");
        // Different stream id ⇒ different output.
        let mut c = Random::new();
        c.seed_from([1, 2, 3, 4, 5, 6, 7, 8], 43);
        let zs: Vec<u32> = (0..64).map(|_| c.next_u32()).collect();
        assert_ne!(xs, zs);
    }

    #[test]
    fn reseed_produces_distinct_streams() {
        let mut a = Random::new();
        let mut b = Random::new();
        a.reseed();
        b.reseed();
        let xs: Vec<u32> = (0..8).map(|_| a.next_u32()).collect();
        let ys: Vec<u32> = (0..8).map(|_| b.next_u32()).collect();
        assert_ne!(xs, ys, "two reseeds collided");
    }

    // ---------------------------------------------------------------------
    // H-34: this is a BESPOKE crypto primitive (the crate cannot depend on a
    // crypto library — an allocator must not depend on code that allocates),
    // so the hardening audit requires it be vetted rather than trusted. The
    // vetting is structural and bottoms out in a published vector:
    //
    //   1. the quarter-round — the entire cryptographic core — is checked
    //      against RFC 8439 §2.1.1's test vector;
    //   2. the block function's STRUCTURE (constants, key/counter/nonce
    //      placement, the column/diagonal round pattern, the feed-forward
    //      add) is checked against the RFC's layout;
    //   3. the stream's contract (counter advance, keystream never repeats,
    //      streams separate) is checked as properties.
    //
    // ChaCha8 differs from RFC 8439's ChaCha20 in ONE parameter: 4
    // double-rounds instead of 10. Everything the RFC's vectors can pin —
    // the quarter round and the block layout — is therefore fully pinned
    // here; the round count is verified by inspection against the loop bound
    // and asserted below so a future edit cannot silently change it.
    // ---------------------------------------------------------------------

    /// RFC 8439 §2.1.1: the authoritative quarter-round test vector.
    #[test]
    fn quarter_round_matches_rfc8439() {
        // The RFC states the quarter round on four numbers directly. Our
        // `qround` operates on state indices, so place them at 0,1,2,3.
        let mut x = [0u32; 16];
        x[0] = 0x1111_1111;
        x[1] = 0x0102_0304;
        x[2] = 0x9b8d_6f43;
        x[3] = 0x0123_4567;
        qround(&mut x, 0, 1, 2, 3);
        assert_eq!(x[0], 0xea2a_92f4, "quarter round: a");
        assert_eq!(x[1], 0xcb1c_f8ce, "quarter round: b");
        assert_eq!(x[2], 0x4581_472e, "quarter round: c");
        assert_eq!(x[3], 0x5881_c4bb, "quarter round: d");
    }

    /// The block layout the RFC specifies: the four "expand 32-byte k"
    /// constants, the key in words 4..12, the counter in 12..14 and the
    /// stream/nonce in 14..16.
    #[test]
    fn state_layout_matches_rfc8439() {
        let mut r = Random::new();
        let key = [
            0x0302_0100,
            0x0706_0504,
            0x0b0a_0908,
            0x0f0e_0d0c,
            0x1312_1110,
            0x1716_1514,
            0x1b1a_1918,
            0x1f1e_1d1c,
        ];
        r.seed_from(key, 0xdead_beef_cafe_f00d);
        assert_eq!(
            [r.state[0], r.state[1], r.state[2], r.state[3]],
            [0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574],
            "the four ChaCha constants are wrong"
        );
        assert_eq!(&r.state[4..12], &key, "key is not in words 4..12");
        assert_eq!(
            [r.state[12], r.state[13]],
            [0, 0],
            "counter must start at 0"
        );
        assert_eq!(r.state[14], 0xcafe_f00d, "stream low word");
        assert_eq!(r.state[15], 0xdead_beef, "stream high word");
    }

    /// The 64-bit block counter advances by one per refill and carries
    /// correctly across the 32-bit boundary — a counter that fails to
    /// advance would repeat the keystream, which is the catastrophic failure
    /// mode for a stream cipher.
    #[test]
    fn block_counter_advances_and_carries() {
        let mut r = Random::new();
        r.seed_from([0; 8], 0);
        for _ in 0..16 {
            r.next_u32(); // forces exactly one refill
        }
        assert_eq!(
            [r.state[12], r.state[13]],
            [1, 0],
            "counter did not advance"
        );

        // Force the low word to wrap and confirm the carry reaches word 13.
        r.state[12] = u32::MAX;
        r.used = 16;
        r.next_u32();
        assert_eq!(
            [r.state[12], r.state[13]],
            [0, 1],
            "64-bit counter carry is broken: the keystream would repeat"
        );
    }

    /// ChaCha8 is EIGHT rounds. If a future edit changes the loop bound the
    /// primitive silently becomes something else — pin the identity by
    /// checking a full keystream block against this implementation's own
    /// frozen output for an all-zero key, which changes if the round count,
    /// the round pattern, or the feed-forward add changes.
    #[test]
    fn chacha8_block_is_frozen() {
        let mut r = Random::new();
        r.seed_from([0; 8], 0);
        let block: Vec<u32> = (0..16).map(|_| r.next_u32()).collect();

        // Independently recompute the SAME block from the RFC's algorithm,
        // written here in a deliberately different shape (explicit round
        // list, no shared helper) so a bug in `refill`'s structure does not
        // reproduce itself in the check.
        let mut s = [0u32; 16];
        s[0] = 0x6170_7865;
        s[1] = 0x3320_646e;
        s[2] = 0x7962_2d32;
        s[3] = 0x6b20_6574;
        let orig = s;
        let mut x = s;
        for _ in 0..4 {
            for &(a, b, c, d) in &[
                (0usize, 4usize, 8usize, 12usize),
                (1, 5, 9, 13),
                (2, 6, 10, 14),
                (3, 7, 11, 15),
                (0, 5, 10, 15),
                (1, 6, 11, 12),
                (2, 7, 8, 13),
                (3, 4, 9, 14),
            ] {
                x[a] = x[a].wrapping_add(x[b]);
                x[d] = (x[d] ^ x[a]).rotate_left(16);
                x[c] = x[c].wrapping_add(x[d]);
                x[b] = (x[b] ^ x[c]).rotate_left(12);
                x[a] = x[a].wrapping_add(x[b]);
                x[d] = (x[d] ^ x[a]).rotate_left(8);
                x[c] = x[c].wrapping_add(x[d]);
                x[b] = (x[b] ^ x[c]).rotate_left(7);
            }
        }
        let expect: Vec<u32> = x
            .iter()
            .zip(orig.iter())
            .map(|(a, b)| a.wrapping_add(*b))
            .collect();
        assert_eq!(
            block, expect,
            "ChaCha8 block function diverged from the RFC construction"
        );
        s = orig; // silence the unused-assignment lint on `s`
        let _ = s;
    }

    /// A keystream that is not obviously degenerate: bit balance within a
    /// few sigma of 50%, and no repeated 32-bit word run. This would not
    /// catch a subtle cryptographic weakness — nothing cheap would — but it
    /// catches the failures that matter here (a stuck counter, an all-zero
    /// or short-period stream) loudly.
    #[test]
    fn keystream_is_not_degenerate() {
        let mut r = Random::new();
        r.seed_from([0xdead_beef; 8], 7);
        let n: u32 = 4096;
        let words: Vec<u32> = (0..n).map(|_| r.next_u32()).collect();
        let ones: u32 = words.iter().map(|w| w.count_ones()).sum();
        let frac = f64::from(ones) / f64::from(n * 32);
        assert!(
            (0.45..0.55).contains(&frac),
            "keystream bit balance {frac} is not plausibly random"
        );
        let mut sorted = words.clone();
        sorted.sort_unstable();
        sorted.dedup();
        // 4096 draws from 2^32 should essentially never collide.
        assert!(
            sorted.len() >= words.len() - 1,
            "keystream repeated words: period far too short"
        );
    }
}
