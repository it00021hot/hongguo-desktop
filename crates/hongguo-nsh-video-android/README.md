# Android native playback

The widget controls the shared `hongguo-playback` session. AAC/tempo decoding and
the AAudio PCM clock belong to that session; Java owns audio focus, noisy-route
interruption and lifecycle. There is no MediaPlayer or HTTP audio-track view.
Real UI control claims the frame consumer; delayed controls, seek, presentation,
decoder admission and faults use the consumer-owned operations.

Software frames upload their unsigned native YUV planes directly to the shared
converter. An 8-bit hardware candidate requires Android API29 hardware codec
classification and `GL_EXT_YUV_target`. The external YUV sampler must succeed;
the extension specifies a draw-time error for a non-YUV image. It is not sampled
through an opaque YUV-to-RGB conversion.

An opaque SurfaceTexture never proves 10-bit storage. A Main10 frame requests an
API33+ P010 ImageReader device generation instead. Each acquired image must have
an actual P010 HardwareBuffer, valid matching crop, and usable GPU dimensions.
The borrowed AHardwareBuffer becomes an EGLImage, sampled directly as raw YUV
into R16UI planes. Image, HardwareBuffer, EGLImage, native frame and decoder device
leases survive GPU completion; error-path retirement also completes pending GL
reads before returning an ImageReader buffer. There is no CPU pixel readback.
The 8-bit SurfaceTexture is likewise retained until a context-local finish before
final release, including failures before a WGPU retirement fence was registered.
Missing classification/format/import capabilities reject the hardware candidate
and recover the same stream through the built-in software decoder.

The common converter preserves source precision through linear FP16 working
textures, optional enhancement, and final output mapping. Without enhancement,
no enhancement pipeline or extra normalization copy is created. SDR uses final
UNORM dithering. A capable HDR display can instead use a below-window SurfaceView:
the GLES instance verifies actual RGBA16F component sizes/type and scRGB extensions,
the configured EGL window colorspace, native RGBA16F format and linear scRGB
dataspace. The final shader retains negative and above-one scRGB values. Display
capabilities supply the luminance target; an SDR source leaves the HDR layer.
Missing/invalid capability selects SDR without first opening a decoder.

Separate HDR surfaces are admitted only under a stable rectangular NSH hierarchy.
Canvas shaders/transitions, masks, rounded ancestor clipping, ancestor transforms
or partial opacity select TextureView composition. Intersecting sibling overlays
and their descendants also reject post-layout transforms, opacity transitions or
shader/mask composition; ordinary rounded overlay buttons remain allowed. The
check excludes the video's own output child. This fallback is temporary: an HDR
source returns to SurfaceView when the hierarchy stabilizes. A float-capable
display can first initialize a FULL-range F16 SDR window during UI transitions,
so restoring HDR retains that same device. Only a real EGL/output capability
failure disables further HDR attempts for the current source. The HDR layer never uses `setZOrderOnTop`;
controls/dialogs stay above it. Changing only the output window retains the same
GLES device and MediaCodec input surface, without spending decoder recovery budget.
An already-float instance uses a separate SDR window with
`EGL_GL_COLORSPACE_LINEAR_KHR` and `ADATASPACE_SRGB_LINEAR` (**FULL** range).
HDR retains `EGL_GL_COLORSPACE_SCRGB_LINEAR_EXT` and
`ADATASPACE_SCRGB_LINEAR` (**EXTENDED**). The actual colorspace and dataspace are
checked after each configure. The Android window compositor supplies final SDR
encoding; the SDR window is not tagged as extended HDR.
Khronos deliberately leaves external-YUV chroma upsampling to
the driver; device color/chroma golden comparisons remain required.

Android uses **80 as a normalization reference** for both UNORM sRGB and floating
linear SDR output. This makes SDR white 1.0 in either path; it is not a measurement
of the panel's white luminance, which is unknown. `convert` and `map_output` share
that same target. The common `DisplayTarget::sdr()` default uses a 100 reference;
SDR input retains the same relative black/gray/white, while Android's HDR-to-SDR
curve uses the shared 80 reference for both outputs. Physical HDR-to-SDR goldens
still require a device. HDR uses the scRGB 80-nit reference and the display's
advertised HDR peak; neither claims the current physical brightness was measured.
See Android's [DataSpace definitions](https://developer.android.com/reference/android/hardware/DataSpace#DATASPACE_SRGB_LINEAR),
[KHR linear EGL colorspace](https://registry.khronos.org/EGL/extensions/KHR/EGL_KHR_gl_colorspace.txt),
and AOSP's [EGL wrapper](https://android.googlesource.com/platform/frameworks/native/+/refs/tags/android-14.0.0_r1/opengl/libs/EGL/egl_platform_entries.cpp).
The wrapper initially maps linear EGL to UNKNOWN, so the explicit native-window
FULL-range tag and its readback are necessary. Native-window tags persist across
swaps; the wrapper sets them at window creation, not each swap.

The controlled same-version WGPU HAL patch associates an explicit unique token
with the actual EGL swap. TextureView callbacks and real display pulses check that
exact token, source epoch and output-surface version before acknowledging the frame.
Surface changes invalidate both native pending presentation and already-queued Java
callbacks without retiring GPU image leases early. SurfaceView instead
requires the matching token recorded after successful final EGL window swap, with
an active valid surface. GPU completion alone cannot release audio. These are buffer
consumption/window submission contracts, not physical scanout measurements. Optional
physical-present/power measurements are unavailable. Android API28's CDD requires the presentation-time
extension; missing/rejected tokens are explicit failures. See
[`UPSTREAM.md`](../../vendor/wgpu-hal/UPSTREAM.md) for the patch and removal plan.

Choreographer supplies real display pulses while playing or awaiting a first
frame/seek/recovery image. Stable paused widgets block on the mailbox. GPU fence
retirement alone uses a temporary bounded poll; decoder image callbacks also wake
the worker. A retained output window survives resize and inactive props, avoiding
unnecessary decoder generations. A real GPU loss pauses the clock, rebuilds the
renderer, and consumes the shared recovery budget through the appropriate device
or presentation-recovery operation.

An explicit empty video edit hides the retained media image behind black and does
not wait for a nonexistent first frame. The UI's play intent reaches the core while
the core's holding state gates the actual PCM clock. Natural video tails still hold
the last image.

Verified here: Android API28 Rust cross-checks, strict Android renderer Clippy,
API37 Java SDK compilation, and pure Java stale-epoch/exact-token gates. Physical
Android MediaCodec/P010/AAudio playback, HDR display, visual goldens, refresh-rate
behavior, device loss and energy measurements are separate device QA gates.
