# CLI reference

`nsh <command> [ENTRY]`. Every command that builds an app takes the app's **directory** (the default: the current
one), which runs a Rust app's root component (`nsh.json`'s `"root"`) or a TSX app's `App.tsx`; or a TSX entry module.
It reads `nsh.json` in the app's root and refreshes `.nsh/types`.

`nsh <command> --help` prints a command's flags. `nsh -V` prints the version and the Git revision.

## Output

One line per step; details only on failure. `✓` a finished step, `→` where to connect, `•` what a device did, `warn`
and `error` a problem. Plain lines are the app's own output (`console.log`). `-v` / `--verbose` (or `NSH_VERBOSE=1`)
prints everything as it comes: log lines, cargo's output, the hosts' own.

## nsh init

```sh
nsh init [DIR] [--name <NAME>] [--template <ts|rust|mixed>] [--nsh-source <DIR>]
```

| Template | What it creates |
|---|---|
| `ts` | `App.tsx`, `App.test.tsx`, `nsh.json`, `tsconfig.json`, `.gitignore`, `.nsh/types`. Files that exist are kept. |
| `rust` | A Cargo project (`Cargo.toml`, `src/lib.rs` with the root component, `src/main.rs` for `cargo run`), `nsh.json` with `"rust": "."` and `"root": "App"`, `App.test.tsx`, `tsconfig.json`, `.gitignore`, a README |
| `mixed` | A TSX app with a Rust crate at its root: `App.tsx`, `src/lib.rs`, `Cargo.toml`, `nsh.json` with `"rust": "."` and **no** `"root"` |

`--name` sets the display name; the id becomes `com.example.<name>` (lower case, letters and digits only) and the
crate's import specifier `<name>/native`. `--nsh-source <DIR>` makes the app depend on this checkout's crates by path
instead of crates.io's.

Without `--template` the command asks on a terminal (Enter picks `ts`) and takes `ts` elsewhere.

## nsh run

```sh
nsh run [ENTRY] [--target catalyst|ios|android|macos|linux|windows|gpui|web] [--device <DEVICE>]
          [--port <PORT>] [--release] [--limits [<LIMITS>]] [--trace] [--fps] [--name <NAME>] [--window-size <WxH>]
```

| Target | What it runs |
|---|---|
| *(default)* | This machine's backend: AppKit on macOS, Win32 on Windows, GTK on Linux |
| `catalyst` | The Mac Catalyst host, an iPhone-sized window (390×844) |
| `ios` | The host in an iOS simulator (a booted one, else the newest iPhone, booted) |
| `android` | The host on the first device `adb devices` lists (`adb reverse` forwards the port) |
| `macos` | The AppKit host, a 1200×800 window |
| `linux` | The GTK 4 host, a 390×844 window |
| `windows` | The Win32 host, a 1200×800 window |
| `gpui` | The GPUI host, on macOS, Windows and Linux |
| `web` | The dev server's page, at the address it prints |

| Flag | Default | Description |
|---|---|---|
| `--device <DEVICE>` | | iOS simulator, by name or UDID |
| `--port <PORT>` | `8790` | The dev server's port; the next free one is used if it is taken |
| `--release` | | Minify and strip debug info. No hot reload: every save reloads the app. |
| `--limits [<LIMITS>]` | | Run as an embedded app: `memory=128MB,stack=4MB,load=10s,call=2s,nodes=200000`. TSX entries only. |
| `--trace` | | Log the timing of every commit |
| `--fps` | | The performance monitor over the app (macOS only for now) |
| `--name <NAME>` | `nsh.json`'s name | The name in the menu bar and the window title (macOS) |
| `--window-size <WxH>` | The host's | The initial window size, in points |

For a TSX entry, `nsh run` starts the dev server and a development host, and hot-updates the running app on every
save. For a Rust app it builds the app's own runner (no QuickJS) and restarts it on every change.

## nsh dev

```sh
nsh dev [ENTRY] [--target …] [--device …] [--server-only] [--port …] [--release] [--limits …]
```

The dev server plus this system's host. `--server-only` serves hosts you start yourself and prints the URL for this
machine and one per LAN address.

## nsh test

```sh
nsh test [PATH] [--filter <NAME>] [--trace] [--junit <FILE>] [--target <TARGET>] [--browser <PATH>]
```

| Flag | Description |
|---|---|
| `--filter <NAME>` | Only the tests whose name contains it |
| `--trace` | Every mount batch and every commit's time |
| `--junit <FILE>` | Also write a JUnit XML report (the format CI services read) |
| `--target <TARGET>` | Run the apps in a real window: `macos`, `linux`, `windows`, `gpui`, `web`, `ios`, `android` |
| `--browser <PATH>` | The Chrome / Edge / Chromium `--target web` runs the apps in |

## nsh check

```sh
nsh check [ENTRY] [--tsc <TSC>] [-- <TSC ARGS>...]
```

Refreshes `.nsh/types` and `.nsh/tsconfig.json`, then runs `tsc -p <app directory> --noEmit` with the app's
`tsconfig.json`. It uses TypeScript 7's native compiler (downloaded once, about 9 MB; no Node needed). Type errors
exit non-zero. It warns when `tsconfig.json` does not extend `./.nsh/tsconfig.json`.

**The only command that checks types.** The others strip types without checking them.

## nsh build

```sh
nsh build [ENTRY] [-o <OUT>] [--release] [--split]
```

| Flag | Default | Description |
|---|---|---|
| `-o, --out <OUT>` | `.nsh/out` | The output directory |
| `--release` | | Minify and strip debug info |
| `--split` | | Put what `import()` loads into chunk files, downloaded when first imported (web apps) |

It writes `bundle.js`, `bundle.js.map`, `bundle.nshc` (the bytecode), `capabilities.json` (if declared) and
`assets/`. A Rust app writes `app.json` and its assets instead.

## nsh package

```sh
nsh package [ENTRY] --target ios|ios-simulator|android|macos|linux|windows|web [-o <OUT>] [flags]
```

| Flag | Description |
|---|---|
| `--target <TARGET>` | Required |
| `-o, --out <OUT>` | The output directory (default `.nsh/out`) |
| `--name`, `--id`, `--app-version`, `--build` | Override `nsh.json` |
| `--icon <ICON>` | A square PNG, ideally 1024×1024 |
| `--sign <IDENTITY>`, `--profile <FILE>` | iOS devices: the signing identity and the provisioning profile |
| `--keystore <FILE>`, `--key-alias <ALIAS>`, `--keystore-password <PASSWORD>` | Android: the release keystore (the password also comes from `NSH_KEYSTORE_PASSWORD`) |
| `--run` | Install and launch afterwards (or serve it, for the web) |

The output is a signed `.ipa` / `.app` (iOS), a signed `.apk` (Android), an ad hoc signed `.app` (macOS), a
directory and a `.tar.gz` (Linux), a directory and a `.zip` (Windows), or a static site (the web). The web site is
content-addressed (`<digest>` from the content's SHA-256), so serve `index.html` and `manifest.webmanifest` as
`no-cache`, everything else as `public, max-age=31536000, immutable`, and `.wasm` as `application/wasm` — a release
then touches only `index.html` and the files that really changed.

## The signing commands

```sh
nsh keygen keys/root                     # keys/root.secret + keys/root.pub
nsh certify --root keys/root.secret --key keys/release.pub --apps 'com.example.*' -o keys/release.cert
nsh publish my-app --key keys/release.secret --cert keys/release.cert --build 2
nsh verify out/com.example.my-app-2.nshpkg --root keys/root.pub
nsh revoke --root keys/root.secret --package <sha256> -o revocations.json
nsh symbolicate bundle.js.map            # rewrites bundle.js:LINE:COL to the app's sources
```

| Command | What it does |
|---|---|
| `keygen <NAME>` | An Ed25519 key pair. Keep a root's `.secret` offline; apps trust its `.pub`. |
| `certify` | Certifies a release key with a root key for some app ids and a time window (`--days`, default 365). |
| `publish` | Builds in release mode and signs into a package. `--build` must grow with every release. `--source-map package\|file\|none`, `--strip-debug`, `--min-runtime`. |
| `verify` | Verifies a package as a trusting app would. Prints the package's SHA-256. |
| `revoke` | Signs a revocation list: `--key <ID>` and `--package <SHA256>` to refuse, `--extend <LIST>` and `--sequence <N>` to chain. |
| `symbolicate` | Maps a reported error stack to the app's sources. |

## Environment variables

| Variable | Used by | Description |
|---|---|---|
| `ANDROID_HOME`, `ANDROID_SDK_ROOT` | Android | The Android SDK (default `~/Library/Android/sdk`) |
| `CARGO_TARGET_DIR` | Rust apps | Where the app's runners build (default the app's `target/`) |
| `NSH_KEYSTORE_PASSWORD` | `package --target android` | The keystore password |
| `NSH_RUNTIME` | `run`, `package` | A runtime directory instead of the one next to the CLI |
| `NSH_TSC` | `check` | The `tsc` to run |
| `NSH_VERBOSE` | every command | `1`: as `--verbose` |
| `NSH_WASM_BINDGEN` | `run`, `package --target web` | The `wasm-bindgen` CLI |
| `NO_COLOR` | every command | No colors on terminals |

## Environment gotchas

- **The web target needs the web runtime.** `nsh package --target web` and `nsh run --target web` use the runtime next
  to the CLI; in a checkout that runtime has to be built first (`cargo xtask web`). `NSH_RUNTIME` points the commands
  at a runtime directory instead.
- **The web target needs a WebAssembly toolchain.** Building the runtime (`cargo xtask web`) and packaging or running
  a Rust app for the web need the wasm target and a clang that compiles to it: `rustup target add
  wasm32-unknown-unknown`, plus `clang` and `llvm-ar` (on macOS an LLVM toolchain such as Homebrew's, named through
  `CC_wasm32_unknown_unknown` and `AR_wasm32_unknown_unknown`). When the app's crates call browser APIs through
  wasm-bindgen, `nsh` runs the `wasm-bindgen` CLI of the version the runtime links and prints it:
  `cargo install wasm-bindgen-cli --version <version> --locked` installs it, `NSH_WASM_BINDGEN` names another. Pin
  `wasm-bindgen` (`=0.2.x`) in the crate so the CLI's version holds from build to build.
- **An app with Rust crates builds for minutes the first time** (the crate, the headless runner, the development
  host). That is normal.
- **A Linux Rust app does not link WebKitGTK unless it uses web views.** The generated runners no longer enable
  `nsh-linux-app/webview` unconditionally; the app's own crate opts in by enabling `webview` on its
  `nsh-platform-linux` dependency, and Cargo feature unification carries that into the runner. Enabling it links
  WebKitGTK 6.0, which must be installed to build and run. A Rust app that renders `<WebView>` on Linux needs this.
- **`nsh.json`'s `"name"` names the output files**: `tsx-app` → `tsx-app.app`, `tsx-app.ipa`, `tsx-app.apk`,
  `tsx-app/`. Set it, or pass `--name`.
- **`nsh publish` refuses to sign what a device would refuse**, and warns about a placeholder id (`com.example.*`).
  Give the app a real reverse-DNS id before releasing it: apps sharing an id share their storage.
- **`nsh keygen` never overwrites an existing key.** A second `nsh keygen keys/root` leaves the first pair alone —
  move the files out of the way first.
- **The CLI version and the checkout must agree.** In a checkout, build and use `./target/debug/nsh`; an `nsh` on
  `PATH` may be an older release whose flags differ. This skill documents **0.2.0** (checkout `v0.2.0-33-g4dea976d`,
  2026-10-23).

## LayerFS (checkout only)

The NSH repository carries a Linux LayerFS trial for reusable Cargo build workspaces: `scripts/layerfs prepare`,
`smoke`, `status`. It needs Docker Engine with a local Linux FUSE device and writes receipts under `.nsh/layerfs/`.
This is a workflow for working **on NSH itself**, not something an app uses; skip it unless the task is about NSH's own
build infrastructure.

## Where things are written

- `.nsh/` — everything NSH generates: `.nsh/types` (the declarations), `.nsh/tsconfig.json`, `.nsh/rust` (the
  sources of a Rust app's runners), `.nsh/test`, and `.nsh/out` (the default build and package output). Safe to
  delete; it is recreated.
- `target/` — Cargo's build directory for an app's crates and runners (or `CARGO_TARGET_DIR`).

NSH does not add Cargo configuration to relocate `target/`.
