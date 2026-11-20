# guava · 番果

guava is an unofficial Hongguo short drama client for **Windows and macOS**. Browse recommendations, find a series, pick up where you left off, or download selected episodes.

**[Download latest](https://github.com/it00021hot/hongguo-desktop/releases/latest)** · [Install](#download-and-install) · [Quick start](#quick-start) · [Changelog](CHANGELOG.md) · [中文](README.md)

## Download and install

Open the [latest release](https://github.com/it00021hot/hongguo-desktop/releases/latest) and match the file name suffix under **Assets**.

- **Windows x64** (Windows 10 1809+ or Windows 11, excluding Windows N). Run the Simplified Chinese installer ending in `windows-x86_64-setup.exe`, or extract the entire `windows-x86_64-portable.zip` before opening `guava.exe`.
- **Mac · Apple silicon (M series)** (macOS 14+). Choose `macos-aarch64.dmg`.
- **Mac · Intel** (macOS 14+). Choose `macos-x86_64.dmg`.

Find your chip type in About This Mac. Open the matching DMG and drag guava into Applications.

The Windows executable is unsigned. macOS builds are ad hoc signed and have no Apple notarization. Android and iOS hosts can be built from source. GitHub Releases provides desktop packages. [Desktop release guide](docs/desktop-release.md)

## Quick start

### Browse or find a series

Open **Home** to browse recommendations, animated dramas, and live-action dramas. Select the title on the video to open its details, or enter a title or keyword in the top search field. Open **Find** for filters such as genre, duration, and completion status, plus rankings and new releases.

### Resume and adjust playback

Open a series from **My → History** to resume. Choose an episode, playback speed, or quality in the player. You can also read comments and send danmaku with emoji.

| Default shortcut | Action |
| --- | --- |
| <kbd>Space</kbd> | Play or pause |
| <kbd>←</kbd> / <kbd>→</kbd> | Skip back or forward 5 seconds |
| <kbd>↑</kbd> / <kbd>↓</kbd> | Switch dramas or episodes |

Change the playback shortcuts in **Settings**.

**Window modes** include fullscreen, mini player, portrait, always on top, and borderless. The main, mini, and portrait windows each remember their own size and position.

**Video enhancement** offers sharpening and Anime4K 2×. Anime4K works with animated sources up to 1080p on supported devices. Enhancements apply to playback only.

### Download, merge, and clean up episodes

- Choose **Download** in the player and select an episode or enter a range. Follow progress under **Downloads** in the sidebar.
- Open **Merge** to join downloaded episodes. Use **Quick merge** for lossless joining when files match, or **HEVC merge** to re-encode mixed formats before joining.
- Use **Storage** to remove local files by series while keeping the series record available for future downloads.

### Manage your library and desktop preferences

**My** holds your history, collections, likes, follows, and reservations. Sign in to sync collections and follows. Set your download folder and shortcuts in **Settings**. The desktop app includes Chinese and English, light and dark themes, a system tray, quit confirmation, and a boss key to hide the window quickly.

## Updates and data

**Since 0.8.0**, use **Check for updates** and then **Download update** in Settings. Once the download is verified, select **Update now** to install and restart with your settings and login intact. The installation directory must be writable. If you are using **0.7.0 or earlier**, install the latest release manually once.

| Platform | Default data directory |
| --- | --- |
| Windows | `%APPDATA%\guava` |
| macOS | `~/Library/Application Support/guava` |

The database is `guava.db`. Set `GUAVA_DATA_DIR` to use a separate data directory. Downloaded videos are stored under `guava/<series>` in your chosen download folder.

### Previous-version data

Migrating an old database is a manual step. The copy replaces the new database, so back up both versions first.

<details>
<summary>Show migration paths and steps</summary>

| Platform | Default old database | Default new database |
| --- | --- | --- |
| Windows | `%APPDATA%\hongguo-downloader\hongguo.db` | `%APPDATA%\guava\guava.db` |
| macOS | `~/Library/Application Support/hongguo-downloader/hongguo.db` | `~/Library/Application Support/guava/guava.db` |

1. Fully quit both versions, including tray processes, and back up both data directories.
2. Create the new directory if needed. Move its existing `guava.db` and journal files to the backup location.
3. **Copy** the old `hongguo.db` into the new directory as `guava.db`. If `hongguo.db-wal`, `hongguo.db-shm`, or `hongguo.db-journal` remain, copy them with the database and rename them to the corresponding `guava.db-wal`, `guava.db-shm`, or `guava.db-journal`.
4. Start the new version, check your records and settings, and keep the original database and backups.

Use your actual directories if customized. Migration copies the database and its journal files. Downloaded videos and caches stay at their original paths. These steps apply to older desktop versions that use `hongguo.db`.

</details>

## Development and packaging

The desktop UI uses NSH 0.6.0 and the app core uses Rust. Install Rust 1.99.1 and the SDK for your target, then build the pinned NSH CLI from the repository root.

```sh
cargo build --locked --manifest-path vendor/nsh/Cargo.toml -p nsh-cli --target-dir target/nsh-cli
```

Add `target/nsh-cli/debug` to your terminal's PATH, then run these commands from the repository root.

```sh
nsh run crates/hongguo-nsh
nsh check crates/hongguo-nsh
```

On macOS, `make dev` also configures the VVdeC wait policy. To package, run `nsh package crates/hongguo-nsh --target <target>` with `macos`, `windows`, `android`, `ios`, or `ios-simulator`. Building a Windows installer requires Inno Setup.

The build verifies and extracts the codec SDKs bundled in the repository. Building, packaging, and running `make lint` do not require Node.js or npm. See [nsh.json](crates/hongguo-nsh/nsh.json) for app metadata and platform options.

### Playback implementation

Standard video uses NSH Video, with bundled decoders covering additional HEVC and ByteVC2 playback cases. Windows prefers D3D11 hardware decoding for extended HEVC and uses software decoding when hardware decoding is unavailable. Online playback uses progressive caching. Prefetch follows server limits and gives current playback priority. [Decoder validation](docs/nsh-decoder-validation.md)

## Documentation and feedback

| Guide | Covers |
| --- | --- |
| [NSH development guide](.agents/skills/hongguo-dev/references/nsh.md) | Toolchains, packaging, and local verification |
| [Project structure](docs/project-structure.md) | UI, services, playback, and decoder boundaries |
| [Desktop release guide](docs/desktop-release.md) | Windows and macOS builds, signing, and update packages |
| [Documentation index](docs/README.md) | Topic guides and historical plans |

Include your app version, operating system, and steps to reproduce. For playback issues, include the video quality and whether enhancement was enabled. [Open an issue](https://github.com/it00021hot/hongguo-desktop/issues)

## Project notice

guava is for personal, noncommercial learning and research. It is unaffiliated with the official Hongguo service. The project repository does not host or distribute media. [Notice](NOTICE) · [License](LICENSE) · [Third-party notices](THIRD-PARTY-NOTICES.md)

The early Tauri and GPUI implementations are available in the [legacy backup](https://github.com/it00021hot/hongguo-desktop/tree/codex/backup-tauri-final).
