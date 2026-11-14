# Windows native video

Production uses the shared playback session and its actual NSH D3D11 device.
NV12/P010 hardware frames and original-precision CPU planes enter the same color
pipeline. The optional enhancement path receives linear FP16; final display
mapping follows enhancement. NSH `Auto` selects verified FP16/scRGB presentation
or a reported SDR fallback. No customer decoder selection control is added.

## Developer D3D11VA / D3D12VA audit

The `decoder-audit` feature is development-only. It opens an explicit local,
unencrypted H.264/HEVC MP4 fixture; it does not fetch media or access account data.
Run it on the physical Windows machine whose GPU/display is being evaluated.
Use the pinned native libraries built by `scripts/native-media.mjs`; the selected
prefix must include both the D3D11VA2 and D3D12VA hardware accelerators.

From a Windows developer terminal at the repository root:

```powershell
node scripts/native-media.mjs build x86_64-pc-windows-msvc
$ffmpegPrefix = (Resolve-Path target/native-media/x86_64-pc-windows-msvc/ffmpeg-9.0.2).Path
$env:PATH = "$ffmpegPrefix/bin;$env:PATH"
cargo run -p hongguo-nsh-video-windows --features decoder-audit --example decoder_audit -- C:\fixtures\hevc-main10.mp4 C:\reports\main10.json both 900 off
```

Arguments are `INPUT REPORT [d3d11|d3d12|both] [MAX_FRAMES]` and
`[off|sharpen|anime4k|both]`. `both` runs four fresh child processes in ABBA order
and retains each JSON report beside the aggregate. Test H.264, HEVC Main and
Main10 separately, plus the SDR/HDR and enhancement combinations relevant to the
release. Keep window visibility, size, monitor, refresh rate, power mode and
background workload consistent. Driver/OS cache state is not claimed to be cold.

Both candidates use the production video component, color conversion,
enhancement and NSH canvas. The D3D12 trial creates a device on the same adapter
LUID, waits on the decoder frame's fence on its COPY queue, copies both video
planes into a shared NV12/P010 texture, and queues a shared-fence wait on NSH's
D3D11 context. The retained AVFrame protects the decoder pool slice. Native
leases, shared textures and command allocators remain retained until GPU copy
completion even if the window closes before a queued frame is displayed. No
media pixel resource is downloaded to the CPU.

Reports distinguish:

- Decoder surface return, GPU draw completion, and whole-canvas DXGI
  `Present`/`Present1` submission, with per-frame identities and deadlines.
- The first 30 warm-up frames and steady-state pre-present drops/deadline misses.
- Whether each canvas submission actually used enhancement. A requested effect
  that fails or disappears during steady state invalidates the trial; raw
  fallback cannot silently be reported as an enhanced comparison.
- Extra D3D12 copy bytes, CPU submission time, GPU queue-wait count, and GPU copy
  timestamps when the device supports COPY queue timestamp queries. The only
  D3D12 CPU readback is the 16-byte timestamp pair for a copied frame.
- Raw DXGI frame statistics, if supported, and the actual output color mode,
  measured display metadata or explicit SDR fallback reason.

The clock is a fixed 1x video-only monotonic clock gated on the first GPU-drawn
frame. Fixtures must have one continuous presentation edit beginning at zero.
GPU completion is observed through a nonblocking event query on the next draw;
its CPU timestamp is an observation upper bound at the UI polling resolution.
Audio decoding/output and power consumption are not measured by this
tool. `SyncQPCTime` is a vblank reference and is not a per-frame photon timestamp;
the report leaves physical power, photon time and total OS display drops unknown.
Use external physical measurement and appropriate Windows trace tooling for
those conclusions. An unavailable hardware profile or interop configuration
fails that candidate; the audit never substitutes software or pixel download.

The source has been cross-checked for Windows types. GPU execution, actual HDR
output, device-removal recovery and comparative performance still require
physical Windows validation. The production default remains D3D11VA; a report
does not automatically change policy or declare either candidate optimal.
