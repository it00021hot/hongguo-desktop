`audio.m4a` contains the unchanged synthetic AAC samples from
`hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4`.
The optional window test reads this checked-in fixture and requires no external media tool.

`clocked.rs` attaches the production platform audio factory to the synthetic frame sources,
so pause/seek assertions follow an actual AVPlayer clock. The probe uses NSH's background
window mode: visible without taking keyboard focus, so drawable presentation is observable.

On an unlocked Mac, build both probes and run the native regressions:

```sh
cargo build -p hongguo-nsh-video-macos --example pixel_surface --example display_clock
HONGGUO_NSH_MACOS_PIXEL_TEST=1 node --test scripts/nsh-macos-pixel.test.mjs
```

The display-clock probe switches 60 synthetic native frame sources at 24/30/60 fps,
clears the source, and checks pause and teardown across 10 clock/controller instances.
It exercises real CADisplayLink callbacks; headless component tests cannot detect an
Objective-C selector that is unavailable on macOS.
