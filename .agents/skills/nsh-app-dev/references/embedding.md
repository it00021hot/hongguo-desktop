# Embedding NSH in a host app

A host app shows NSH in a view of its own: `NshAppView` (a `UIView`) on iOS, `NshView` (an Android `View`) on
Android, `AppView` on macOS, Windows and Linux, a GPUI view in GPUI apps, and on the web a page element driven by
`createPlayer`. Several instances may coexist — there
are no global singletons.

## The four things a host can load

| Source | What it is | iOS | Android | macOS / Windows / Linux | GPUI |
|---|---|---|---|---|---|
| **A Rust root** | A component compiled into the host; no VM at all | Rust-side `AppView::run(Root)` only (the Swift API is JS-only) | `NshView.runRustApp("nsh")` | `view.load(Root::of::<App>())` | `view.load(App::Rust(Root::of::<App>()))` |
| **A built bundle** | `bundle.nshc` (bytecode) or `bundle.js`, with `assets/` next to it | `try app.loadBundle(at: url)` | `nsh.loadAssetBundle("nsh")` | `view.load(Bundle::source(code))` | `view.load(App::Js(Bundle::source(code)))` |
| **The dev server** | Hot updates from `nsh dev` | `try app.connectDevServer(url)` (the **Dev** framework only) | `nsh.connectDevServer(url)` (the **dev** AAR only) | `AppSource::DevServer(url)` in `run_app` | `view.connect_dev_server(url, cx)` (the `dev` feature) |
| **A signed package** | A `.nshpkg` downloaded at runtime, verified against a trust store | `try await app.loadPackage(at:trust:)` | `trust.openPackage(file)` then `nsh.runPackage(pkg)` | not exposed | `view.run_package(&package)` |

The dev-server protocol is `ws://host:port/ws`; assets are then served from `http://host:port/assets/`.

On the web the sources collapse: the page loads the runtime `nsh package --target web` ships and runs a bundle with
`createPlayer`; the dev-server flow is opening the page `nsh run --target web` prints. See [Web](#web-javascript).

## The distribution artifacts

| Artifact | What it is | Built by |
|---|---|---|
| `NativeShell.xcframework` | The iOS framework (release) | `cargo xtask ios-embed` |
| `NativeShellDev.xcframework` | The iOS framework with dev-server support (debug) | `cargo xtask ios-embed` |
| `nsh.aar` | The Android library (release) | `cargo xtask android` |
| `nsh-dev.aar` | The Android library with dev-server support (debug) | `cargo xtask android` |

They land in `dist/nsh/embed/ios/` and `dist/nsh/embed/android/`.

## iOS (Swift / UIKit)

Link `NativeShellDev.xcframework` in Debug and `NativeShell.xcframework` in Release. The Swift module is `NativeShell`;
apps never see the C ABI behind it.

```swift
import NativeShell

let app = NshAppView()
app.onError = { message in print("nsh error: \(message)") }
view.addSubview(app)          // constrain it like any other view

#if DEBUG
try app.connectDevServer(URL(string: "ws://127.0.0.1:8790/ws")!)
#else
try app.loadBundle(at: Bundle.main.url(forResource: "bundle", withExtension: "nshc", subdirectory: "nsh")!)
#endif
```

`NshAppView` (a `UIView`, `@MainActor`):

| Member | What it does |
|---|---|
| `init(frame:trace:)` | Creates the view |
| `onError: ((String) -> Void)?` | Fatal errors, on the main thread |
| `limits: NshLimits` | The VM's limits (memory, stack, load and call budget, node count) |
| `storageDirectory: URL?` | Where the app's storage lives |
| `locale: Locale?`, `fontScale: CGFloat?` | The app's environment |
| `grantCapabilities(_ json: String?)` | The capabilities the host grants the module |
| `loadBundle(at: URL)` | Loads `bundle.nshc` (or `bundle.js`) and its assets |
| `connectDevServer(_ url: URL)` | Connects to `nsh dev` (Dev framework only) |
| `loadPackage(at: URL, trust: NshTrust) async throws -> NshPackage.Info` | Verifies and runs a signed package |
| `run(_ package: NshPackage)` | Runs an already-verified package |
| `register(component name: String, make: @escaping (NshComponentContext) -> NshComponent)` | A native component |
| `static var preferredStatusBarStyle` / `prefersStatusBarHidden` | The status bar |

`NshLimits(memoryMB:stackKB:loadBudget:callBudget:maxNodes:)` — the defaults are 256 MB, 4 MB, 10 s, 2 s and
200 000 nodes.

The C ABI is in `crates/nsh-platform-ios/include/nsh.h` (`nsh_app_create`, `nsh_app_load_bundle`,
`nsh_app_connect_dev_server`, `nsh_app_set_limits`, `nsh_trust_*`, `nsh_app_register_component`, …). It is for
hosts that cannot use Swift.

A Rust host (linking `nsh-platform-ios` directly) mounts a root itself:

```rust
use nsh_platform_ios::{set_ui_sets, AppView, Root, SurfaceOptions};

set_ui_sets(vec![nsh_base::registry(), my_app::registry()]);
let app = AppView::new(mtm, container.bounds(), SurfaceOptions::default());
container.addSubview(app.view());
app.run(Root::of::<my_app::App>());       // a Rust root
// app.load(bundle);                     // or a JS bundle
```

## Android (Java / Kotlin)

```gradle
dependencies {
    debugImplementation(files("libs/nsh-dev.aar"))
    releaseImplementation(files("libs/nsh.aar"))
}
```

```java
public final class MainActivity extends Activity {
    private NshView nsh;

    @Override protected void onCreate(Bundle state) {
        super.onCreate(state);
        nsh = new NshView(this);
        nsh.setErrorListener(message -> Log.e("nsh", message));
        setContentView(nsh);
        nsh.loadAssetBundle("nsh");       // assets/nsh/bundle.nshc
    }

    @Override protected void onDestroy() { nsh.destroy(); super.onDestroy(); }
}
```

`com.longbridge.nsh.NshView`:

| Member | What it does |
|---|---|
| `NshView(Context)` / `NshView(Context, boolean trace)` | Creates the view (the data directory is `filesDir/nsh`) |
| `setErrorListener(ErrorListener)` | Errors, on the UI thread |
| `setGrantedCapabilities(String)` | The capabilities the host grants |
| `setLimits(int memoryMb, int stackKb, int loadMs, int callMs, int maxNodes)` | 0 keeps the default |
| `setStorageDirectory(File)`, `setLocale(Locale)`, `setFontScale(float)` | The app's environment |
| `loadAssetBundle(String dir)` | Runs `assets/<dir>/bundle.nshc` |
| `loadBundleFile(File)` | Loads `bundle.js` (refuses `.nshc`) |
| `runRustApp(String dir)` | Runs the Rust root compiled into `libnsh.so` |
| `runPackage(NshPackage)` | Runs a verified package |
| `connectDevServer(String url)` | Connects to `nsh dev` (dev AAR only) |
| `registerComponent(String name, NshComponent.Factory factory)` | A native component |
| `handleBack()` | Call from `onBackPressed` (Android ≤ 12) |
| `destroy()` | Stops the app |

The Rust side is JNI glue: `nsh_platform_android::set_ui_sets`, `nsh_platform_android::run_root`, and the
`ui_sets!`, `root!`, `main!` macros.

## macOS, Windows and Linux (Rust)

The three desktops share one shape: `AppView` + `AppOptions` + `AppSource`.

```rust
use nsh_platform_macos::{objc2::MainThreadMarker, objc2_app_kit::NSWindow, set_ui_sets, AppOptions, AppView, Root};

fn show(window: &NSWindow) -> Rc<AppView> {
    set_ui_sets(vec![nsh_base::registry(), my_app::registry()]);
    let mtm = MainThreadMarker::new().expect("main thread");
    let view = AppView::new(mtm, AppOptions::default());
    view.load(Root::of::<my_app::App>().into());     // or Bundle::source(code).into()
    window.setContentView(Some(&view.view()));
    view
}
```

| Crate | The view | The widget | The standalone entry |
|---|---|---|---|
| `nsh-platform-macos` | `AppView::new(mtm, AppOptions)` → `view()` gives an `NSView` | — | `run_app(AppSource, AppOptions)` |
| `nsh-platform-windows` | `AppView::new(Option<HWND>, AppOptions)` → `hwnd()`; call `MoveWindow` on `WM_SIZE` | — | `run_app(AppSource, AppOptions)` |
| `nsh-platform-linux` | `AppView::new(AppOptions)` → `widget()` gives a GTK 4 widget | GTK 4 | `run_app(AppSource, AppOptions)` |

`AppOptions` carries `trace`, `asset_base`, `app_id`, `title`, `size`, `components`, `appearance`, `limits`,
`data_dir` and `storage`.

`AppSource` is `Rust(Root)` | `Bundle(Bundle)` | `DevServer(String)` (the `dev` feature).

## GPUI

The GPUI backend is its own workspace (`crates/gpui/`), built with
`cargo build --manifest-path crates/gpui/Cargo.toml --target-dir target -p nsh-gpui-host`.

```rust
use gpui::{AppContext, WindowOptions};
use nsh_platform_gpui::{set_ui_sets, App, AppOptions, AppView, Root};

set_ui_sets(vec![nsh_base::registry(), my_app::registry()]);
gpui_platform::application().run(|cx| {
    cx.open_window(WindowOptions::default(), |window, cx| {
        cx.new(|cx| {
            let view = AppView::new(AppOptions::default(), window, cx);
            view.load(App::Rust(Root::of::<my_app::App>()));
            view
        })
    }).unwrap();
});
```

Put the `Entity<AppView>` anywhere in the element tree. `AppView` is `Focusable` and `Render`; its `focus_handle`
moves focus into the embedded surface. `open_app_window(source, options, cx)` opens a window with the app in it.

## Web (JavaScript)

There is no view class on the web: `nsh package --target web` writes a static site to `.nsh/out/<Name>/`, and a page
embeds it with an `<iframe>` (its size is the app's window) or runs a bundle in a container of its own with
`createPlayer` from `nsh.js`:

```js
import { createPlayer } from "./nsh.js";

const player = await createPlayer(document.querySelector("#app"), {
  wasm: "./nsh.wasm",       // the runtime `nsh package --target web` ships
  assetBase: "./assets/",   // where `asset:/…` URIs are served from
  // dark: true,            // force an appearance; otherwise it follows the system
  // idiom: "phone",        // force "phone", "pad" or "desktop"; otherwise the pointer and the size decide
});
player.run(await (await fetch("./bundle.nshc")).arrayBuffer());
```

- **Limits replace `NshLimits` / `setLimits`:** `run(code, { limits: "memory=128MB,call=500ms" })` sets the limits
  the page names and keeps the embedding defaults for the rest; `limits: ""` removes them all. The engine runs on
  the page's main thread, so the call budget also keeps the page responsive.
- `run(code, { release: true, capabilities })` runs a bundle as a release (a release also takes `harden`, the
  hardening bytecode); `chunks` / `chunkBase` locate a split bundle's chunks; `glue` is the wasm-bindgen glue of a
  runtime whose crates call browser APIs; `readAsset(uri)` returns an asset's bytes at once (a font);
  `player.setDark(true)` switches the appearance at run time. Resizing the container resizes the app.
- **A Rust app embeds as its published site** — one `nsh.wasm` with the crate compiled in, no `bundle.nshc`. With
  the playground runtime (`nsh-playground.wasm`), `player.runSource(tsx, name, options)` compiles TSX first and runs
  it; `player.compile(tsx)` only compiles.
- **There is no trust-store flow on the web.** A published app is already a release — hardened, capability-checked,
  with a CSP `connect-src` the browser enforces. Gate who runs what at the URL and host level.

The site is content-addressed, so serve `index.html` and `manifest.webmanifest` with `no-cache`, everything else
with `public, max-age=31536000, immutable`, and `.wasm` as `application/wasm`.

## Signed packages and the trust store

A signed package (`.nshpkg`) is a module a host app downloads after release. The host verifies the signature, the
certificate and the revocation list before running anything.

```swift
// iOS
let trust = try NshTrust(stateDirectory: support.appendingPathComponent("nsh"))
try trust.addRoot(rootPublicKey)                 // the contents of root.pub
try trust.updateRevocations(latestJson)          // optional
let info = try await app.loadPackage(at: downloaded, trust: trust)
```

```java
// Android
NshTrust trust = new NshTrust(new File(getFilesDir(), "nsh"));
trust.addRoot(rootPublicKey);
executor.execute(() -> {
    try {
        NshPackage pkg = trust.openPackage(downloadedFile);   // off the UI thread
        runOnUiThread(() -> nsh.runPackage(pkg));
    } catch (NshPackage.RefusedException e) { /* the running app stays */ }
});
```

```rust
// GPUI, or any Rust host
use nsh_platform_gpui::{accept_package, package::{PublicKey, TrustStore}, verify_package};

let mut trust = TrustStore::open(state_dir)?;
trust.add_root(PublicKey::from_base64(ROOT_PUB)?);
let package = verify_package(&trust, &bytes, &packages_dir)?;   // off the main thread
view.read(cx).run_package(&package);
// once the app is healthy:
accept_package(&mut trust, &package)?;
```

`verify_package` checks without recording the build; `accept_package` records it, which is what stops a rollback.
`load_package` does both. The Rust types are `nsh_package::TrustStore` and `nsh_engine::{load_package,
verify_package, accept_package}`.

## Native components

A host can give the module its own native views — a map, a chart, a design-system control.

```swift
// iOS
app.register(component: "embed.badge") { context in
    let view = MyBadgeView()
    return MyBadgeComponent(view: view, context: context)
}
```

```java
// Android
nsh.registerComponent("embed.badge", context -> new MyBadgeComponent(context));
```

```rust
// Any Rust host
view.register_component("embed.badge", Rc::new(|context| Box::new(MyBadge::new(context))));
```

The protocol is the same everywhere: the component has a `view`, receives props with `setProp`, and receives
commands with `command`; it emits events back with `context.emit(name, data)` and reports its size with
`context.setSize(size)`.

From TSX, the module imports the host's component with `nativeComponent` from `nsh/native`.

## Capabilities and limits

The host decides, before loading, what a module may do:

- **Capabilities** (`grantCapabilities` on iOS, `setGrantedCapabilities` on Android) cap the network hosts, the web
  view hosts and the status bar. What is not granted is denied.
- **Limits** (`NshLimits` / `setLimits`) cap the VM's memory, stack, load and call budget and node count.

The module's own `nsh.json` may declare capabilities; the host may grant less. An app that embeds NSH and loads
unsigned bundles it built itself still runs within the host's limits.

## Development with an embedded host

`nsh dev <app> --server-only` serves the app to hosts you start yourself and prints the URL for this machine and one
per LAN address:

```text
→ dev server ws://127.0.0.1:8790/ws
  devices on this network: ws://192.168.1.20:8790/ws
```

The host connects to `ws://127.0.0.1:8790/ws` (iOS, on the same machine) or to the LAN address (a device). On
Android, forward the port with `adb reverse tcp:8790 tcp:8790`.

For a release build, `nsh build <app> --release -o MyApp/nsh` writes the bundle into a folder you add to the app's
resources (`bundle.nshc` plus `assets/`). On the web, `nsh run --target web` (or `nsh dev`) prints the page's
address for this machine and one per LAN network — open it in any browser, a phone on the same network included.
