# 🎬 Hongguo Desktop · 红果播放器

<div align="center">

![Platform](https://img.shields.io/badge/Platform-Windows%20%2F%20macOS-blue?style=for-the-badge)
![Stack](https://img.shields.io/badge/Stack-Tauri%202%20%2B%20Rust%20%2B%20React%2019-47848F?style=for-the-badge)
![License](https://img.shields.io/badge/License-PolyForm%20Noncommercial%201.0.0-blueviolet?style=for-the-badge)

**Unofficial Hongguo short-drama desktop client — immersive feed · charts · danmaku playback · batch download · one-click merge**

Pure-Rust decryption & codec core · Installer bundles no external binaries · Stream while downloading · Mini-window / stealth playback

[中文](./README.md) · [NOTICE](./NOTICE) · [Third-party notices](./THIRD-PARTY-NOTICES.md)

</div>

---

## ⚠️ Disclaimer (read first)

> **This project is for personal learning, study and communication only. Commercial use is strictly prohibited.**

- This is an **unofficial** third-party client. It has **no affiliation with** and is **not endorsed by** ByteDance, Fanqie Novel, or Hongguo Short Drama.
- This project **does not store or distribute any video content** — all content is fetched on the user's device from the official APIs at the user's direction and remains the property of its rightful owners.
- You assume all consequences of using this project. The authors make no warranty of any kind regarding functionality, stability, safety, or legality.
- If this project infringes your rights, please open an issue and we will address it promptly.

See [NOTICE](./NOTICE) for the full statement and [LICENSE](./LICENSE) for license terms.

---

## 📖 Overview

Hongguo is a Windows / macOS desktop client for **Hongguo Short Drama** (the short-drama
channel of Fanqie Novel): a **Rust** core, a [Tauri 2](https://tauri.app) shell, and a
React 19 UI.

It implements the complete ByteDance short-drama app protocol stack — device registration,
five-header request signing, stream fetching, danmaku, interactions, watch history, SMS
login — together with a **native CENC-AES-CTR streaming decryption engine**. Decryption,
demuxing, transcoding and muxing are done entirely in pure Rust: **the installer ships no
FFmpeg and no external binaries at all**.

- **Streaming never touches disk** — a custom protocol feeds the player progressively,
  decrypting on the fly; the first frame doesn't wait for the whole episode
- **Downloads are standard MP4** — streaming decryption + atomic rename, no truncated
  files on disconnect
- **Fully automatic compatibility mode** — when HEVC can't be decoded, it transcodes to
  H.264 with a four-level fallback: platform hardware → ffmpeg hardware → ffmpeg software
  → pure-Rust software
- **Data lives in an embedded Turso database** — pure Rust, no C toolchain, with a schema
  migration chain and automatic recovery

---

## ✨ Features

### 🏠 Home · Discover

- **Recommendation feed**: a fullscreen, immersive vertically-swiping stream with three
  channels (Recommended / Comic / Live-action); tapping the active tab again shuffles.
  Mouse wheel or ↑↓ switches series; tapping the video locks into episode mode. The next
  series' episodes and streams are prefetched while you watch
- **Find**: search with typeahead suggestions (300 ms debounce; series hits jump straight
  into the player) plus the official 8-dimension filter panel (genre / theme / characters
  / era / sort / audience / release window / length), infinite scroll
- **Charts**: content tabs (all / live-action / comic / AI / series) × sub-charts × filter
  panel; gold medals for the top 3, hot keywords and heat metrics. Unreleased titles can
  be reserved in-row; released ones play immediately
- **New**: cover grid sorted by release time (newest first) and a **release calendar**
  (±1 week, including unreleased), with male / female channel filters

### 🎬 Series Detail

- Rating, Hongguo heat, followers, play count, genre tags, filing number, expandable synopsis
- Three tabs: **Episodes** (grid with duration badges), **Reviews** (post reviews after
  login; like and reply counts shown), **Related** (series works + recommendations for you)
- "Continue at episode N" (jump to the next episode once ≥95% watched), favorite, like
- Unreleased series degrade to a "Coming soon + reserve" view
- `seriesId` is a URL parameter, so detail pages are bookmarkable and shareable

### ▶️ Player

- **Online streaming**: in-memory progressive stream, decrypted while downloading, with an
  on-screen "caching xx%" indicator; a dropped stream retries once automatically without
  losing your position
- **Quality switching**: starts at the highest available tier; switching keeps the current
  playback position
- **Danmaku**: toggle + style panel (opacity / size / density / display area) and
  **sending** (emoji picker); freezes on pause, syncs with speed, rebuilds on seek
- **Speed** 0.75×–3×; volume and mute persist across episodes
- **Mini-window**: the same window shrinks in place to 480×270 at the bottom-right of the
  work area with zero playback interruption; plus window **always-on-top**, native PiP,
  and fullscreen
- **Stealth mode**: mouse leaves the window → the whole window hides and playback pauses;
  mouse returns → it reappears in place and resumes
- **Binge & relay**: auto-next episode; when a season ends it chains into season 2 episode
  1, or follows the feed to the next recommendation
- **Resume**: local progress persisted (5 s throttle) and restored across restarts, while
  also syncing to the cloud
- **Compatibility mode**: undecodable codecs are auto-transcoded to H.264 with a
  transcoding-progress overlay
- **Shortcuts**: `Space` play/pause · `←` `→` seek 5 s · `↑` `↓` prev/next episode/series

### 👤 Account & Interactions

- **Phone number + SMS code login** (including the "reply to SMS" MFA flow with automatic
  background polling)
- Likes, favorites (bookshelf), **release reservations** (released / upcoming), watch
  history — all cloud-sourced and shared with the official app and other clients
- Per-episode comments (in the player) and series-level reviews (on the detail page)

> No login required: browsing, charts, new releases, search, streaming, downloading, merging.
> Login required: likes, favorites, reservations, danmaku, comments.

### ⬇️ Download · Merge · Cleanup

- **Task manager**: concurrency 1–10 (default 3), five task states (pending / running /
  completed / failed / stopped), batch retry & delete, one-click pause/start; **disk
  rescan** re-registers files you copied back manually
- **Crash-safe writes**: each episode is written as `.enc.tmp` and renamed to `.mp4` only
  after decryption succeeds — no truncated files on disconnect
- **One-click merge**: quick merge (stream-copy concatenation, lossless and near-instant) /
  compatible merge (transcode to H.264/AAC so any player works); preflight checks codec
  consistency and disk space; background jobs with progress
- **Cleanup**: total usage stats, delete by series / episode, **auto-delete after
  watching**, clear everything; streaming cache and compatibility cache (4 GB cap)
  managed separately; deleting files keeps the series record so you can stream or
  re-download later
- **Naming templates**: `Title E01` / `Title E01 第N集` / title only

### ⚙️ Settings

- Account card (login / MFA / sign out), download folder (typed or picked), naming
  template, concurrency
- **Proxy**: follow system / manual (common port presets) / force direct, with a one-click
  latency test
- **Transcode backend badge**: live indication of hardware / software / standard
  (pure-Rust) mode, with re-detection
- Auto-next and auto-delete-after-watching toggles
- **Theme**: auto / light / dark (dark default); **bilingual UI** (Chinese / English),
  follows system language
- **In-app updates**: check → download (with progress) → install (minisign-verified;
  silent on Windows)

---

## 🛠️ Tech Stack

### Core (Rust)

| Layer    | Choice                                                                                           |
| -------- | ------------------------------------------------------------------------------------------------ |
| Shell    | [Tauri 2](https://tauri.app) (system webview, no Electron)                                       |
| Language | Rust 2024 edition · tokio · reqwest (rustls)                                                     |
| Storage  | [Turso](https://turso.tech) embedded database (pure Rust, `hongguo.db`, schema v5)               |
| Crypto   | aes / ctr / sm3 / md-5 and other pure-Rust crates (CENC-AES-CTR)                                 |
| Codecs   | rusty_h265 / rusty_h264 / muxide (pure Rust) + VideoToolbox / Media Foundation + optional ffmpeg |

### Frontend

| Layer          | Choice                                                |
| -------------- | ----------------------------------------------------- |
| Language       | TypeScript 6 · React 19                               |
| Build          | Vite 8 · Vitest                                       |
| Routing / data | TanStack Router (file-based) · TanStack Query 5       |
| State          | Zustand 5 · zod 4                                     |
| UI             | shadcn/ui (Radix UI) · Tailwind CSS v4 · lucide-react |

---

## 🏗️ Architecture Highlights

- **Custom URI schemes**: `hongguo-stream://` progressive streaming (256 KB head probe →
  box-walking to locate `moov` → 1 MB sequential chunk fill; the first frame only needs
  head + tail + sample table); `hongguo-local://` local file playback with Range support;
  `hongguo-cover://` cover proxy (HEIC → JPEG with an on-disk hash cache)
- **Signing stack**: five signature headers (`x-gorgon` / `x-argus` / `x-ladon` /
  `x-helios` / `x-medusa`) locked by golden-vector unit tests
- **Four-level transcode fallback**: platform hardware (VideoToolbox on macOS / Media
  Foundation on Windows) → ffmpeg hardware (NVENC / QSV / AMF) → ffmpeg software →
  rusty_h265 + rusty_h264 pure-Rust software; probed at startup and shown as a badge
- **Reliability**: single-instance lock, three-layer panic guard on protocol callbacks,
  crash.log forensics, database corruption recovery, interrupted-job resume, and a
  one-shot idempotent migration from the legacy JSON archive

---

## 💻 Development

### Requirements

- Rust 1.99+ (edition 2024)
- Node.js 20+ / pnpm 10+
- Windows 10/11 or macOS 10.15+

### Commands

```bash
make dev            # Vite + Tauri dev mode (= pnpm tauri:dev)
make release        # bundle NSIS / DMG (= pnpm tauri:build)
make test           # vitest unit tests
make test-rust      # cargo test
make lint           # ESLint + Prettier + cargo clippy (any failure fails the target)
make typecheck      # tsc --noEmit
make assets         # regenerate icons / macOS name localization (offline)
```

> **No FFmpeg setup required** — decryption, demuxing and merging are pure Rust with zero
> external dependencies. If ffmpeg happens to be installed, only the **HEVC → H.264
> transcode** step switches to it for speed; nothing breaks without it.

---

## 📂 Project Structure

```text
hongguo-desktop/
├── src-tauri/src/
│   ├── signer/          # ByteDance request signing (constants are black-box locked, don't edit)
│   ├── domain/          # Official API clients (api/ one directory per endpoint domain), CENC crypto, MP4 parsing
│   ├── service/         # App services: download scheduler, playback, merge, transcode, storage…
│   ├── commands/        # Thin Tauri command layer (~80 commands, mirrors service/)
│   ├── protocol/        # Custom URI schemes (streaming / local files / cover proxy)
│   ├── media/           # Codecs & transcode pipeline (platform HW → ffmpeg → pure Rust)
│   ├── store/           # Turso embedded database (entity/ split by aggregate / migrations / recovery)
│   ├── utils/           # Pure helpers without business semantics (json / time / hex / url)
│   └── bootstrap/       # Startup wiring (store → device → rescan → queue → transcode probe)
└── src/
    ├── pages/           # TanStack file routes (thin shells) + page implementations nearby
    ├── features/        # Complex capability domains (player engine / auto update)
    ├── service/         # IPC wrappers (tauri/) + commands + queries + zod contracts (schema/)
    ├── stores/          # zustand client state
    ├── components/      # shadcn/ui + layout + shared display components
    ├── hooks/           # Shared hooks
    ├── locales/         # Chinese / English message resources
    ├── utils/           # Pure helpers (format / range / cover / playback-prefs…)
    └── styles/          # Global styles
```

---

## ❓ FAQ

### Series won't load / empty API response

The official app API requires five signature headers per request: `x-gorgon`, `x-argus`,
`x-ladon`, `x-helios`, `x-medusa`. **When a signature is missing or wrong the server
doesn't error — it returns HTTP 200 with a 0-byte body.** Checking only the status code
will mislead you.

Check the **response body size**, not the status code; for per-endpoint detail, launch the
app with `RUST_LOG=debug`.

### Black screen with audio

The video is HEVC and the system webview needs hardware support to decode it. The built-in
compatibility mode handles this: it transcodes to H.264 automatically — platform hardware
(VideoToolbox on Apple silicon / Media Foundation on Windows) → ffmpeg (NVENC/QSV/AMF) →
pure-Rust software decoding, with automatic fallback and no manual configuration.

### Can I edit the signature constants?

**No.** The values in `signer/constants.rs` are black-box measurements that match the
server one-for-one. Change any of them and signatures will be silently dropped.

### Which features need login?

Browsing, charts, new releases, search, streaming, downloading and merging all work
**without login**. Likes, favorites, reservations, danmaku and comments require phone +
SMS login (account card in Settings).

---

## 📄 License

This project is licensed under the **PolyForm Noncommercial 1.0.0** — see
[LICENSE](./LICENSE).

- ✅ **Permitted**: use, modification and redistribution for any noncommercial purpose —
  personal learning, research, private entertainment, hobby projects
- ❌ **Prohibited**: all commercial use (selling, paid distribution, bundling into
  commercial products, monetization)
- 📋 Redistributions must include the license (or its URL) and the `Required Notice`
  copyright line

Because it prohibits commercial use, this is not an OSI-approved open-source license; it
is a professionally drafted license text from the
[Polyform Project](https://polyformproject.org) that expresses "personal learning only,
no commercial use" precisely.

Third-party components remain under their own licenses — see
[THIRD-PARTY-NOTICES.md](./THIRD-PARTY-NOTICES.md). Project statement and disclaimer:
[NOTICE](./NOTICE).
