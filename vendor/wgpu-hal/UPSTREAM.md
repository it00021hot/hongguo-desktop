# Controlled Android output and presentation patch

Upstream package: `wgpu-hal` **29.0.4**, crates.io release, MIT OR Apache-2.0.
Repository: <https://github.com/gfx-rs/wgpu/tree/v29.0.4/wgpu-hal>.
The unmodified package is the matching crates.io `.crate` artifact; all files
except `src/gles/egl.rs` and this note retain that release's contents.
Crate archive SHA-256: `97ace1c17727311c22a46e4e3faf56ea6de81af99dcc839bdfb54857b94d448d`.

The Android-only patch adds `Surface::set_next_presentation_time(token_ns)` and
consumes its atomic token immediately before the real `eglSwapBuffers`.
`submitted_presentation_time()` records that exact token only after a successful
swap. It describes final-window submission, not physical presentation.
It checks both the extension string and EGL function pointer. Other targets and
swaps without a token retain upstream behavior. No WGPU/core API or decoder is
changed. The application uses `wgpu::Surface::as_hal` for this narrow extension.

The FP16 API must be selected before enumerating adapters or creating surfaces.
It recreates only the new instance's EGL context with a verified RGBA16F float
window config, then tags opted-in HDR windows with `SCRGB_LINEAR_EXT` and SDR
windows with ordinary `LINEAR_KHR` through `set_android_linear_output(hdr)`. The
application verifies the actual configured surface, native window format and
appropriate extended HDR/full-range SDR dataspace before admitting output. It retains upstream configuration on all other
instances. See the Khronos
[float configuration](https://registry.khronos.org/EGL/extensions/EXT/EGL_EXT_pixel_format_float.txt)
and [linear scRGB](https://registry.khronos.org/EGL/extensions/EXT/EGL_EXT_gl_colorspace_scrgb_linear.txt)
specifications.

For SDR TextureView, this allows `onSurfaceTextureUpdated` and subsequent real
display pulses to return
the **exact** submitted token from `SurfaceTexture.getTimestamp()`, paired with
the application's source epoch and decoder generation. Some Android releases
dispatch `onSurfaceTextureUpdated` before RenderThread actually dequeues the
buffer, so the application rechecks on Choreographer pulses. For HDR SurfaceView,
the application's gate requires the matching successful final-window swap token
and a current active valid surface. GPU completion alone never releases the
shared audio clock. Neither path claims to measure physical display scanout;
optional presentation timestamps and power measurements remain unavailable.
The application also carries an output-surface version through each submission
and Java callback so a late success from a retired surface cannot acknowledge its
replacement.

Android 9 and later CDD §7.1.4 C-2-2 requires
[`EGL_ANDROID_presentation_time`](https://source.android.com/docs/compatibility/9/android-9-cdd).
A missing or rejected entry point is a visible capability failure, never an AUTO
timestamp heuristic. The
[Khronos specification](https://registry.khronos.org/EGL/extensions/ANDROID/EGL_ANDROID_presentation_time.txt)
defines the timestamp association for buffers consumed through SurfaceTexture.

Validation: the application cross-compiles for Android API28, including this HAL
implementation; native GPU/device callback ordering remains a physical-device QA
gate. Keep the patch when updating WGPU until upstream provides an equivalent
per-swap timestamp API. Then remove this directory and the root crates.io patch,
switch the application to the upstream API, and repeat seek/recovery stale-frame
and compositor-latch tests. Never modify the Cargo registry cache.
