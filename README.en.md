# Hongguo Desktop

An unofficial Hongguo client built with NSH 0.5.0 and Rust. Desktop targets are Windows 10 1809+, Windows 11 and macOS; Android and iOS have dedicated hosts.

[中文](README.md) · [Notice](NOTICE) · [Third-party notices](THIRD-PARTY-NOTICES.md)

For noncommercial learning and research only. This project is unaffiliated with the official service and does not store or distribute media. Read [NOTICE](NOTICE) and [LICENSE](LICENSE).

Install Rust, platform SDKs, Node.js and pnpm, then run from the repository root:

```sh
pnpm install --frozen-lockfile
pnpm nsh
pnpm nsh:check
pnpm test
make nsh-package TARGET=macos
```

Use `TARGET=windows` on Windows; installer builds also require NSIS 3. Mobile targets are `android`, `ios` and `ios-simulator`.
Normal development verifies and extracts the bundled codec SDKs without building FFmpeg, VVdeC or libde265.
Customer packages include their codec libraries and do not install WebView2 or HEVC Store extensions.

Playback uses NSH Video, supplemented by libde265/VVdeC and native audio clocks. Extended Windows HEVC prefers D3D11 VLD with P010 for Main10; software decoding is the fallback. No FFmpeg is used.
See the [validation record](docs/nsh-decoder-validation.md) for actual platform coverage.

Shared icons, emoji and translations live in `assets/`. Native preview TypeScript declarations require the matching NSH CLI.
The Tauri/GPUI shells, dedicated players and WebView frontend have been removed. Their complete backup is
[`codex/backup-tauri-final`](https://github.com/it00021hot/hongguo-desktop/tree/codex/backup-tauri-final).
