# AAC seeking

The audio worker keeps one decoder for its source. `reset(rate)` flushes the codec's
overlap/output queue and rebuilds resampling/tempo state while retaining the AAC
configuration and SBR/PS headers. A cold seek first decodes the initial configuration
access units. It then reads a bounded region before the requested media time.

The lower bound follows FFmpeg's
[AAC pre-roll implementation](https://github.com/FFmpeg/FFmpeg/commit/ad74ac4cee):
one core frame for LC, another for SBR, and two additional frames for ELD. Frame
length comes from AudioSpecificConfig; core and extension sample rates remain
distinct. Implicit SBR uses the conservative SBR bound. Unknown predictive/USAC
configurations retain the safe source origin. Negative `roll`/`prol` descriptions
can extend the preceding sample range; positive recovery points are also retained.

PS receives additional settling time from the pinned FFmpeg 9.0.2 implementation:

- [aacpsdsp_template.c](https://github.com/FFmpeg/FFmpeg/blob/n9.0.2/libavcodec/aacpsdsp_template.c):
  three all-pass links, coefficients at most `0.65143905753106`, delays of 3–5 QMF slots.
- [aacps.c](https://github.com/FFmpeg/FFmpeg/blob/n9.0.2/libavcodec/aacps.c):
  peak decay `0.76592833836465`, energy smoothing residual coefficient `0.75`.
- [aacps.h](https://github.com/FFmpeg/FFmpeg/blob/n9.0.2/libavcodec/aacps.h):
  maximum explicit PS delay 14 slots. The bound also includes 12 hybrid-filter
  history slots and 20 synthesis-buffer slots.

For a conservative cascade of `k` stages, pole magnitude `r`, maximum delay `d`,
and normalized residual tolerance `e`, the implemented settling allowance is
`k*d*ceil(log(e / (k*(1+2*r)^k)) / log(r))` slots. It deliberately sums separate
stage allowances and includes a product of all-pass absolute impulse-sum bounds.
The signal path uses `k=3, r=0.65143905753106, d=5, e=f32::EPSILON`.
The three energy-state paths use `k=3, r=0.76592833836465, d=1` and
`e=f32::EPSILON²`, because energy is squared amplitude. Adding the finite histories
gives a normalized old-state settling allowance, measured in 64-output-sample
QMF slots. At 44.1 kHz this is about 1.7 seconds, followed by the normative AAC
overlap. Missing an explicit PS disable in a mono/SBR configuration conservatively
uses this allowance too.

This bounds residual **old filter state**, relative to its initial magnitude. It
does not prove bit-identical synthesis noise, equal PS phase, or valid new codec
configuration when a stream violates its random-access declarations. Initial
headers and larger container recovery declarations are still required. SBR noise
phase and PS decorrelation can differ after discontinuities. Real listening remains
a device QA gate; waveform error alone is not a listening-quality verdict.

Tempo filtering adds the actual bounded input history of each stage:
[`af_atempo.c`](https://github.com/FFmpeg/FFmpeg/blob/n9.0.2/libavfilter/af_atempo.c)
uses a power-of-two window at least `sample_rate/24`, with a three-window input
ring. For the two-stage chain used above 2×, the second stage's history is mapped
back through the first stage's rate. The AAC and filter allowances are additive.

`audio_seek_probe` compares a one-second offline PCM window to continuous decoding,
reports missing samples, both raw and delay-aligned error, and mid/side error. It
accepts an explicit PCM rate to separate resampling phase from codec history. It
never starts an audio device or plays a user's application.
