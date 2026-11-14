# Windows video adapter

Ordinary video uses NSH 0.5.0 Video: MediaEngine schedules native decoding and presents
through NSH's D3D11 canvas. The application does not wrap D3D11VA/D3D12VA with FFmpeg.

This crate presents frames only when native decoding is unavailable, ByteVC2 is selected,
original precision needs the extended color path, or the user requests frame enhancement.
HEVC first tries direct D3D11 VLD on NSH's actual drawing device. Main uses NV12;
Main10 uses P010. This path parses HEVC itself and does not load a system HEVC MFT,
so a missing Store extension does not prevent hardware decoding when the driver supports it.
libde265 is used after unsupported profiles/formats or native initialization, submission,
format-change and import failures. ByteVC2 frames come from VVdeC. Native MediaEngine plays
the audio-only MP4 view and supplies the clock. Source bytes, audio edits and sample offsets
are retained; no playback transcoding or stream substitution occurs.

Original-precision planes enter the shared color pipeline. Enhancement receives linear FP16;
final display mapping follows enhancement. NSH Auto selects supported FP16/scRGB output or
an explicitly reported SDR mapping. Ordinary Windows 10 1809+ / Windows 11 are the targets.

Codec binaries are included in the repository and unpacked by `pnpm nsh`.
The bundle contains one private libde265 DLL with static CRT; customers need no codec packs
or HEVC Store extension. See [native codec builds](../../docs/native-media-build.md).

The former FFmpeg decoder audit was removed with its dependency.
Compilation and PE inspection do not establish physical Windows playback, HDR accuracy,
frame drops, A/V synchronization, decoder throughput or energy consumption.

On a Windows GPU supporting Main and Main10, run the independent synthetic pixel audit:

```sh
cargo run -p hongguo-nsh-video-windows --features test-support --example hevc_d3d11_probe --release
```

The probe requires actual D3D11 decoding and exits unsuccessfully if unavailable;
it never substitutes software for the hardware result. It compares every YUV sample,
10-bit low bits, signed PTS, reordered/drained frame counts, and retained surface contents
against libde265. Its elapsed time includes CPU readback and is not a playback benchmark.
Run it on Windows 10 1809+ / Windows 11 with and without the HEVC extension, followed by
actual application seek/switch/rate/HDR and audio-clock tests. The Mac host only cross-builds it.

中文：Windows 扩展 HEVC 优先直连 D3D11，保留 NV12/P010 与原始颜色元数据，失败才软解。
缺少系统 HEVC 插件仍可尝试硬解；上述探针和实播需要 Windows 真机，不能把交叉构建当作验收。
