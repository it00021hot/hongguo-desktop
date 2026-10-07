# 🎬 Hongguo Desktop

<div align="center">

![Platform](https://img.shields.io/badge/Platform-Windows%20%2F%20macOS-blue?style=for-the-badge)
![Stack](https://img.shields.io/badge/Stack-Tauri%202%20%2B%20Rust%20%2B%20React%2019-47848F?style=for-the-badge)
![License](https://img.shields.io/badge/License-GPL--3.0-orange?style=for-the-badge)

**Browse · Search · Batch Download · Stream · Merge · Clean Up**

Pure-Rust decryption core · Installer bundles no external binaries · Watermark-free · Stream without touching disk

[中文](./README.md) · [License](./LICENSE) · [NOTICE](./NOTICE)

</div>

---

## 📖 Overview

A Windows and macOS desktop tool for **Hongguo short dramas** (the short-drama channel of
Fanqie Novel / novelread).

This version is a **full rewrite** of the Electron + Node.js original into **Tauri 2 + Rust**.
It embeds the ByteDance short-drama API protocol and a **native CENC-AES-CTR streaming
decryption engine**. The installer **no longer ships FFmpeg or any other external binary**,
cutting it from ~158 MB down to the ~70 MB range.

**Decryption, demuxing and merging (remux) are done entirely by pure-Rust crates, with zero
external dependencies.** The one exception is **HEVC → H.264 transcoding**, which has an
optional accelerator: if ffmpeg happens to be installed on the system, the app switches to it
automatically (NVENC / QSV / AMF / MF hardware encoders, close to real-time); otherwise it
falls back to the pure-Rust software path. ffmpeg is always optional — never a prerequisite.

> **Attribution**: This is a **modified version** of
> [327044572/hongguo-downloader](https://github.com/327044572/hongguo-downloader)
> (rewritten from October 2026), declared under GPL-3.0 §5(a).
> See [NOTICE](./NOTICE). This version remains licensed under **GPL-3.0**.

---

## 🛠️ Tech Stack

### Desktop

| Layer  | Choice                                                             |
| ------ | ------------------------------------------------------------------ |
| Shell  | [Tauri 2](https://tauri.app) 2.12+ (Rust / system webview)         |
| Core   | Rust 2021 · tokio · reqwest (rustls)                               |
| Codecs | `rusty_h265` `rusty_h264` `rusty_aac` `muxide` — **all pure Rust** |

### Frontend

| Layer        | Choice                                 |
| ------------ | -------------------------------------- |
| Router       | TanStack Router 1.168 (file-based)     |
| Server state | TanStack Query 5.99                    |
| Tables       | TanStack Table 8.21                    |
| Client state | Zustand 5.0                            |
| UI           | shadcn/ui (Radix UI) + Tailwind CSS v4 |
| Language     | TypeScript 6 · React 19 · Vite 8       |

---

## ✨ Features

### 1. 🔍 Discover

- **Browse** by category (live-action / comic / AI / manga) and genre, with pagination
- **Search** by name via a headless browser sniffing the site
- **Paste a link or ID** — app share links, long text with boilerplate, or a bare numeric ID
- Sniffing keys only off `series_id` in the href, not CSS class names, so it survives redesigns

### 2. 🎯 Select & Download

- **Quick select**: all / invert / clear / first 10 / first 30 / last 30
- **Range syntax**: `1-50`, `1-10, 25, 30-45`
- **Concurrency** 1–10 (default 3) with automatic queueing
- **Native decryption**: `spade_a` key derivation + CENC-AES-CTR streaming, straight to MP4
- **Crash-safe**: writes `.enc.tmp` first and only renames on success — no truncated files

### 3. ▶️ Play

- Built-in player; watch an episode the moment it finishes downloading
- Auto-advance to the next episode
- **Resume** remembers the episode and the second
- Shortcuts: `Space` play/pause · `←` `→` seek 5s · `↑` `↓` prev/next
- **Stream** any episode without downloading it (in-memory, never touches disk). The whole
  episode is fetched and decrypted before playback starts — CENC needs the sample table from
  `moov` to locate every sample, so it can't be decrypted chunk-by-chunk on the fly
- **Compatibility mode**: when HEVC can't be decoded, auto-transcode to H.264. Uses ffmpeg when
  it's installed (NVENC/QSV/AMF hardware encoders), otherwise falls back to the pure-Rust path

### 4. 📋 Task Manager

- States: pending / running / completed / failed / stopped
- Live progress, concurrency indicator, one-click start / pause / retry-all-failed
- **Rescan folder** re-registers files that exist on disk but were lost from the task list

### 5. 🔗 Merge

- **Quick merge**: stream copy, no re-encoding, lossless and near-instant
- **Compatible merge**: transcodes to H.264/AAC for any player
- Preflight checks disk space and codec consistency; verifies the output afterwards

### 6. 🧹 Cleanup

- Delete by series, after watching, per episode, or everything
- Live usage stats; **the series record is kept after deleting files**, so you can still
  stream or re-download later

### 7. ⚙️ Settings

- Download folder, naming template, max concurrency (applied immediately, no restart)
- Proxy: follow system / manual (with common port presets) / force direct, with a connection test
- **Transcode backend**: a live badge shows whether you are on pure-Rust software, ffmpeg
  hardware, or ffmpeg software encoding
- Bilingual UI (Chinese / English)

---

## 💻 Development

### Requirements

- Rust 1.85+ (tested on 1.98)
- Node.js 20+ / pnpm 10+ (tested on Node 24 / pnpm 11)
- Windows 10/11 or macOS 10.15+

### Develop

```bash
pnpm install
pnpm tauri:dev        # Vite + Tauri dev mode
```

### Build

```bash
pnpm tauri:build      # bundles NSIS / DMG
```

> **No FFmpeg setup required** — decryption, demuxing and merging are pure Rust with zero
> external dependencies. If ffmpeg happens to be installed, only the **HEVC → H.264 transcode**
> step switches to it for speed (see "Compatibility mode" above); nothing breaks without it.

### Quality checks

```bash
make test-rust        # cargo test
make test             # vitest
make lint             # ESLint + Prettier + cargo clippy (any failure fails the target)
make typecheck        # tsc --noEmit
```

---

## 📂 Project Structure

```text
hongguo-desktop/
├── src-tauri/src/
│   ├── signer/          # ByteDance signatures (1:1 port; never edit the constants)
│   ├── domain/          # Protocol, crypto, MP4 parsing
│   ├── service/         # Application services (mirrors commands/)
│   ├── commands/        # Thin Tauri command layer
│   ├── protocol/        # Custom URI schemes (Range streaming)
│   ├── media/           # Codecs (pure-Rust software + optional ffmpeg acceleration)
│   ├── sniff/           # Headless browser sniffing
│   ├── bootstrap/       # Startup wiring
│   └── store/           # On-disk task archive
└── src/
    ├── routes/          # Seven route pages
    ├── features/        # Split by business domain
    ├── lib/             # IPC wrappers, schemas, stores
    ├── hooks/           # Shared hooks
    ├── i18n/            # Chinese / English message resources
    ├── styles/          # Global styles
    └── components/      # shadcn/ui + layout
```

---

## ❓ FAQ

### Can't fetch a series / empty API response

The official app API requires five signature headers per request: `x-gorgon`, `x-argus`,
`x-ladon`, `x-helios`, `x-medusa`. **When a signature is missing or wrong the server doesn't
error — it returns HTTP 200 with a 0-byte body.** Checking only the status code will mislead you.

Check the **response body size**, not the status code; for per-endpoint detail, launch the app
with `RUST_LOG=debug`.

### Black screen with audio

The video is HEVC; decoding in the system webview needs hardware support. The built-in
compatibility mode handles this: it transcodes to H.264, using ffmpeg when available
(hardware encoders, so it's close to real-time) and the pure-Rust path otherwise.

### Can I edit the signature constants?

**No.** The values in `signer/constants.rs` are black-box measurements that match the server
one-for-one. Change any of them and signatures will be silently dropped.

---

## 📄 License

**GPL-3.0** — see [LICENSE](./LICENSE).

This project is a modified version of the upstream project; see
[NOTICE](./NOTICE) and [THIRD-PARTY-NOTICES.md](./THIRD-PARTY-NOTICES.md).
