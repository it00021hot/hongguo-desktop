---
name: nsh-app-dev
description: Build, run, test, package and ship Native Shell (NSH) apps with the `nsh` CLI, in TSX or Rust, and embed NSH in native iOS, Android, macOS, Windows, Linux or GPUI host apps or in a web page. Use this whenever the task touches NSH — `nsh init` / `nsh run` / `nsh test` / `nsh check` / `nsh build` / `nsh package` / `nsh publish`, an `nsh.json`, a TSX entry (`App.tsx`, `mount`, `signal`, `nsh`, `nsh/base`), a Rust NSH component (`#[component]`, `#[native_module]`, `nsh_core::ui::prelude`), `NshAppView` / `NshView` / `AppView` embedding, or a signed `.nshpkg` — even when the user only says "add a screen", "package it for the store", or "show our module inside our own app" without naming NSH.
---

# NSH app development

NSH renders a native UI from TypeScript (TSX) or Rust. The reactive graph, the renderer and every native module
are Rust; an app's own code is TSX on QuickJS (hot reload, ships as a signed package a host app downloads) or Rust
(compiled in, no VM at all). One surface — one `NshView` / `AppView` — is one engine.

Three layers, and only three: `nsh-core` is the primitives (`nsh` in TSX, `nsh_core::ui::prelude` in Rust),
`nsh-base` is the unstyled behavior components (`nsh/base`, `nsh_base`), and the app's own components carry the look.
NSH ships **no** styled component library and no default colors or sizes — a component takes the app's `class` /
`style` and children, and the app draws it.

## Orient yourself before writing code

1. **Find the app root.** The directory holding `nsh.json` is the app's root; `nsh.json` is read from there, asset
   paths are relative to it and `.nsh/` is written there. An entry's directory is the app's root too.
2. **Read `nsh.json`.** It says the app's name, id, version, build, icon, theme, capabilities, whether the app has
   Rust crates (`"rust"`) and which component is its root (`"root"`).
3. **Read the exact signatures, do not guess them.** Every NSH module's TypeScript declarations are generated into
   `.nsh/types/` (`nsh/types.d.ts`, `nsh/base.d.ts`, one file per module) and NSH's compiler settings into
   `.nsh/tsconfig.json`. Read them, and the nearest existing implementation, before writing code. Do not translate a
   React, React Native, CSS or GPUI example by analogy — NSH components run **once**, and bindings follow the signals
   they read.

   The `references/` files of this skill carry the API surface on their own. The checkout's own guides (below) are
   for depth.
4. **Check the route** (below) and pick the one the app already uses. Do not mix routes inside one app's UI layer.

## Pick the route

| Route | Choose it when | The entry | The root |
|---|---|---|---|
| **TSX** | The app ships updates to a host app, wants hot reload, or is written by a JS team. The default. | `App.tsx` ending in `mount(App)` | that module |
| **Rust** | The app must ship with no JavaScript VM, or its hot paths are native. | `src/lib.rs` with `#[component] pub fn App()` | `nsh.json`'s `"root"` names the component |
| **Mixed** | Rust owns the core (components, modules, classes) and TSX owns the screens. | `App.tsx` importing the crate | the TSX entry |

A mixed app's TSX imports the crate by the specifier its `#[native_module("…")]` declares (`my-app/native`).

## The command loop

Run from the app's directory. In an NSH checkout use `./target/debug/nsh`; an installed tool is `nsh`.

| Command | What it does |
|---|---|
| `nsh init my-app --template ts\|rust\|mixed` | Create an app (TSX, Rust, or TSX with a Rust crate). `--name` sets the display name. |
| `nsh run [ENTRY]` | Build, launch the native host and hot-reload. `--target macos\|windows\|linux\|ios\|android\|catalyst\|gpui\|web`. |
| `nsh dev [ENTRY] --server-only` | Serve the app to hosts you start yourself (the dev server URL and its LAN addresses). |
| `nsh check [ENTRY]` | Type check with TypeScript 7's native compiler. **The only command that checks types.** |
| `nsh test [PATH\|ENTRY]` | Run the app's `*.test.tsx` headless; `--trace` prints the mount batches and commit timings. |
| `nsh build [ENTRY] -o OUT` | Bundle and compile to bytecode: `bundle.js`, `bundle.js.map`, `bundle.nshc`, `assets/`. |
| `nsh package [ENTRY] --target T` | Build an installable app (`.app`, `.ipa`, `.apk`, `.tar.gz`, `.zip`, a static site). |
| `nsh publish [ENTRY] --key … --cert …` | Build in release mode and sign it into a `.nshpkg` a host app downloads. |
| `nsh keygen` / `certify` / `verify` / `revoke` / `symbolicate` | The signing keys, certificates, revocation lists and error stacks. |

Every command that builds the app refreshes `.nsh/types` and `.nsh/tsconfig.json`. Do not edit those and do not
commit them (`.gitignore` already excludes `.nsh/`).

**The first build of an app with Rust crates takes minutes** (the crate, then the headless runner, then the
development host) — that is normal, not a hang. Let it finish before judging the result.

## Writing TSX

### The shape of an app

```tsx
import { mount, signal, Text, View } from "nsh";
import { SafeAreaView } from "nsh/base";

function App() {
  const count = signal(0);
  return (
    <SafeAreaView class="flex-1 items-center justify-center gap-4 bg-background">
      <Text class="text-3xl font-semibold text-foreground">{count()}</Text>
      <View onPress={() => count.update((c) => c + 1)} role="button" class="rounded-(--radius) bg-primary px-5 py-3 active:bg-primary/90">
        <Text class="text-lg text-primary-foreground">Tap</Text>
      </View>
    </SafeAreaView>
  );
}

mount(App);
```

### Rules that bite

- **A component function runs once.** There is no re-render and no virtual DOM. Reading a signal in the component
  body gives you that moment's value; read it **in JSX** (`{count()}`) or in a **closure** (`style={{ opacity: busy() }}`)
  so the binding follows it. `props.label` read in the body is a snapshot — that is what `split()` is for.
- **Text lives in `<Text>`.** `<View>Hello</View>` is a build error; a string an expression drops into a `<View>` is
  reported in the log.
- **Tags are PascalCase, everywhere.** Every tag names a binding in scope. Only `<If>` and `<Each>` are globals and
  need no import. A lowercase tag is a compile error.
- **Imports come from `nsh`** (primitives: `View`, `Text`, `Scroll`, `Input`, `Overlay`, `Image`, `List`, `Canvas`,
  `WebView`, `Video`, and the reactive API) and from **`nsh/base`** (the unstyled components: `Button`, `Checkbox`,
  `Radio`, `RadioGroup`, `Switch`, `Tab`, `Tabs`, `Collapsible`, `Progress`, `ResizablePanel`, `ResizablePanelGroup`,
  `DockArea`, `NavStack`, `NavPage`, `createNavStack`, `Dialog`, `SafeAreaView`, `safeArea`, `safeAreaInsets`,
  `AlertDialog`, `Keyboard`, `keyboardFrame`, `KeyboardAvoidingView`, `Popover`, `Menu`, `ContextMenu`, `MenuItem`,
  `MenuSeparator`, `Submenu`, `Tooltip`, `Sheet`, `TextView`, `SelectionArea`).
  `nsh/image`, `nsh/list`, `nsh/canvas`, `nsh/webview`, `nsh/video` still export the same values, deprecated: import
  from `nsh`.
- **A control is a base component, not a styled pressable.** `Tabs` and `Switch` own the role, the selected state and
  the keyboard of their kind; a `Button` with `role="tab"` or `role="switch"` does not. Same for `Collapsible`
  (disclosure), `Progress` (the progress-bar role, range and value) and `ResizablePanelGroup` / `DockArea` (the drags,
  the handles' role and the layout data).
- **There is no React.** No `useState`, no `useEffect`, no `key`, no reconciliation. `signal` / `computed` /
  `effect` / `batch` / `untracked` / `cleanup` / `context` replace it, and `solid-js` is refused by the bundler.
- **Styles: `class` is Tailwind, `style` is an object.** `class="rounded-lg px-4"`; `style={{ borderRadius: 12, padding: 24 }}`
  with camelCase keys and **numbers in points**. `style` wins over classes; `activeStyle`, `hoverStyle`, `focusStyle`,
  `disabledStyle` carry the state styles. Tailwind's variants (`active:`, `hover:`, `focus:`, `disabled:`, `dark:`,
  `group-*`, `first:`/`last:`/`odd:`) work as in the browser.
- **Units are points**, not CSS pixels: `p-4` is 16pt, `1rem` is 16pt. `flex` is a **row** (as in CSS); a view with
  no `flex` is NSH's default **column**. Text utilities on a view are inherited by the texts inside it.
- **No DOM and no Node.** There is no `document`, no `window`, no `node_modules` requirement — but web APIs exist
  (`fetch`, `URL`, `streams`, `TextEncoder`, `Blob`, `FormData`). The `node:` prefix and Node's modules are refused.
  The eleven standard modules are `buffer`, `console`, `crypto`, `fs/promises`, `net`, `os`, `path`, `process`, `url`,
  `websocket`, `zlib`.

### The reactive API

`signal(v)` returns a callable: `s()` reads (and subscribes), `s.set(v)` writes, `s.update(f)` writes from the
current value. `computed(fn)` derives, `effect(fn)` runs for side effects, `batch(fn)` applies writes together,
`untracked(fn)` reads without subscribing, `cleanup(fn)` undoes work before the next run, `context(v)` provides a
value to everything below, `split(props, keys)` keeps props reactive when a component handles some itself.
`token("primary")` reads a theme token; the token classes (`bg-primary`, `rounded-(--radius)`) do the same.

A component is a plain function that returns JSX. Give it props as an object; read them where they are used, not in
the body.

Full surface and idioms: `references/tsx.md`.

## Writing Rust

```rust
use nsh_base::{button, theme};
use nsh_core::ui::prelude::*;

/// The application's root component.
#[component]
pub fn App() -> impl IntoElement {
    let count = signal(0_u32);
    view()
        .flex_1()
        .items_center()
        .justify_center()
        .gap_4()
        .bg(move || theme().background)
        .child(text(move || format!("Count: {}", count.get())).text_size(px(28.0)))
        .child(
            button()
                .px_5()
                .py_3()
                .rounded(move || theme().radius)
                .bg(move || theme().primary)
                .on_press(move || count.update(|value| *value += 1))
                .child(text("Tap").text_color(move || theme().primary_foreground)),
        )
}
```

### Rules that bite

- `use nsh_core::ui::prelude::*` gives the primitives, the signals and the typed style methods;
  `use nsh_base::{…}` gives the base components and `theme()`.
- **`#[component]` needs `-> impl IntoElement`** — the return type is mandatory. It generates the component type, its
  element type and a snake_case constructor (`App` → `app()`).
- **Props are a named-field struct** with `#[derive(Props)]`. A `Prop<T>` field is **held and reactive**; a plain
  field is read **once**. Use `Prop<T>` for anything that changes, `#[prop(default = …)]` for an optional one.
  `Option<Slot>` is a named slot; `Children` is the JSX children.
- **Events are an enum** with `#[derive(Event)]`; `cx.emit(ChartEvent::Select(101.5))` sends, and Rust listens with
  `.on_select(move |price: &f64| …)`. TSX gets `onSelect`.
- **Typed styles are named after the Tailwind utilities**: `.px_5()`, `.gap_4()`, `.rounded(px(10.0))`. A style that
  follows a signal takes `move || …`; a state style takes `|s| s.opacity(0.6)`.
- **Lengths are typed**: `px(20.0)`, `rems(16.0)`, `percentage(0.5)` (a fraction), `auto()`. A bare `f32` is not a
  length and does not compile in a public API.
- **Read `theme()` inside a binding** (`move || theme().primary`), or a theme change never reaches it.
- **`#[native_module("app/native")]` must be at the crate's root** (`src/lib.rs`); it registers the crate's
  components (`register::<T>()`), modules (`module::<T>()`) and classes (`class::<T>()`) for TSX to import.
- A Rust app carries **no QuickJS**: `nsh run` builds the app's own runner with the root mounted. A Rust edit
  **rebuilds and restarts**; a TSX edit **hot-updates** in place.

Full surface and idioms: `references/rust.md`.

## One bundle, every platform

The same app runs on iOS, Android, macOS, Windows, Linux, GPUI and in browsers. Most code needs no platform
check; when it does, NSH gives the difference one place:

- **Ask where it runs**: `platform.os` and `platform.idiom` (`"phone"` / `"pad"` / `"desktop"`) from
  `nsh/device`, and `platform.select({ ios: …, android: …, default: … })` for values. In Rust, `os()`,
  `idiom()` and `is_desktop()` from `nsh_core::ui::prelude`. On the web the idiom is decided once at start —
  a fine pointer that hovers and a container at least 600 points wide means `"desktop"`.
- **Platform-only props** go in `ios={{…}}`, `android={{…}}`, `desktop={{…}}` or `web={{…}}`; on other
  platforms the object is ignored, and a desktop browser applies both `desktop` and `web`. Platform-only APIs
  live in `nsh/ios`, `nsh/android` and `nsh/desktop` (in Rust: `toast`, `on_back_press`,
  `nsh_core::ui::desktop`) — they import everywhere and do nothing off their platform.
- **Per-platform styles** use the `ios:` / `android:` / `web:` Tailwind variants; in Rust,
  `.when(os() == Os::Ios, |v| …)`.
- **Prefer shared behavior.** Reach for a platform check only for conventions users rely on — Android's back
  button and ripple, iOS's sheets and autofill names — not to restyle the app per platform.

The web target has edges of its own — a `WebView` is an `<iframe>`, a `Video` with sound needs a user
gesture, native components render empty, and network requests obey the page's CORS. See `website/docs/web.md`
and the toolchain prerequisites in `references/cli.md`.

Full guide: `website/docs/platform-specific.md`.

## Testing

`nsh/test` is the module a `*.test.tsx` imports. It is served only to test files — an app's own bundle cannot import it.

```tsx
import { expect, open, test } from "nsh/test";

test("counts taps", async () => {
  const app = await open("./App.tsx");
  expect(app.texts()).toContain("0");
  await app.press("label:Tap");
  expect(app.texts()).toContain("1");
});
```

- `open(entry, options?)` starts the app on a fresh engine and resolves once it has rendered and is quiet. The path is
  relative to the test file. In a Rust app, `open(".")` starts the Rust root.
- Selectors: `id:<testID>`, `label:<label>`, `text:<text>`, `role:<role>`, `role:button[Save]`, `view:<id>`, a bare
  string, `>>` to look inside, `nth=<n>` to disambiguate, `/regex/` with `i`/`m`/`s`.
- `expect(value)` with `toBe`, `toEqual`, `toContain`, `toBeTruthy`, `toBeFalsy`, `toMatch`, `toHaveLength`,
  `toBeNull`, `toBeGreaterThan` (and the `Less`/`OrEqual` variants), `toThrow`, plus `.not` and `.rejects`.
- The `app` object drives it: `press`, `type`, `submit`, `clear`, `scroll`, `drag`, `longPress`, `hover`, `rightClick`,
  `chooseMenuItem`, `wait`, `waitFor`, `waitUntil`, `back`, `setViewport`, `setColorScheme`, `screenshot`, and reads
  `count`, `shows`, `enabled`, `texts`, `value`, `storage`, `dialogs`, `menu`, `requests`, `clipboard`.
- Headless behaves as **iOS** unless `open` is given `platform: "android"`; give `platform: "macos"` (or another
  desktop, or `"web"` for the desktop idiom in a browser) to get a mouse and a keyboard. `screenshot()` needs a real
  platform (`--target`).
- Tests run under the **`capabilities` the app's `nsh.json` declares**, as on a device: what it does not declare is
  refused in the test too. A test that reaches an undeclared host now fails where it used to pass.
- `nsh test <entry>` with steps on standard input drives an app without a test file:
  `echo 'await app.press("label:Tap")' | nsh test App.tsx`.
- A Rust root is tested in `cargo test` with `nsh_testing::Headless`, and from TSX with `open(".")`.

Details: `references/testing.md`.

## Packaging and signing

`nsh package` builds an installable app from the prebuilt runtime — no Xcode project, no Gradle build. The metadata
comes from `nsh.json`, overridden by `--name`, `--id`, `--app-version`, `--build`, `--icon`.

`nsh.json`'s `"name"` is the **display name and the output file name**: `tsx-app` produces `tsx-app.app`,
`tsx-app.ipa`, `tsx-app.apk` and `tsx-app/`. Set it before packaging, or pass `--name`.

```sh
nsh package my-app --target ios-simulator --run
nsh package my-app --target ios --sign "Apple Development: …" --profile dev.mobileprovision
nsh package my-app --target android --keystore release.jks --key-alias upload
nsh package my-app --target windows
nsh package my-app --target web
```

A **signed package** (`.nshpkg`) is a module a host app downloads after release. The root key stays offline; it
certifies a release key, and the release key signs the packages:

```sh
nsh keygen keys/root && nsh keygen keys/release
nsh certify --root keys/root.secret --key keys/release.pub --apps 'com.example.*' -o keys/release.cert
nsh publish my-app --key keys/release.secret --cert keys/release.cert --build 2
nsh verify out/com.example.my-app-2.nshpkg --root keys/root.pub
```

`--build` must grow with every release. `--source-map` decides where the source map goes (`package`, `file`, `none`);
`nsh symbolicate` maps a reported error stack back to the sources.

## Embedding NSH in a host app

A host app shows NSH in a view of its own — `NshAppView` (a `UIView`) on iOS, `NshView` (an Android `View`) on
Android, `AppView` on macOS, Windows and Linux, a GPUI view in GPUI apps, and a page element on the web. There are
four things a host can load:

| Source | How |
|---|---|
| A **Rust root** | `AppView::load(Root::of::<my_app::App>())` — no VM, compiled into the host. |
| A **built bundle** | `nsh build --release -o <dir>` writes `bundle.nshc`; the host loads it (`loadBundle(at:)` on iOS, `loadAssetBundle("nsh")` on Android). |
| The **dev server** | `nsh dev --server-only`; the host connects to `ws://host:port/ws` (`connectDevServer`). |
| A **signed package** | The host verifies it against a trust store (`NshTrust`) and runs it. |

On the web these collapse into one: the page loads the runtime `nsh package --target web` ships and runs a bundle
with `createPlayer` (an `<iframe>` of the published site is the simpler route); during development, open the page
`nsh run --target web` prints.

Per-platform APIs, the trust flow, the distribution artifacts (`NativeShell.xcframework`, `nsh.aar`), the web
player and the native component protocol: `references/embedding.md`.

## Verifying a change

Do not report an NSH change as done until:

1. `nsh check <entry>` passes — it is the only command that checks types. Prove it does by breaking one type on
   purpose once, if you have never seen it catch anything.
2. `nsh test <app>` passes — and for a behavior change, a test that presses, types or scrolls exists. Tests that only
   call internal functions prove nothing. The test runs under the app's own `capabilities`, so an undeclared network
   host fails here just as it would on a device.
3. For a UI change, `nsh run <app>` shows it, and `nsh test <entry> --trace` shows the mount items and commit times
   did not regress.
4. **Run the tests on a real platform** — `nsh test <app> --target macos|windows|linux|ios|android|web` runs the
   same tests in that platform's window, with its real input, its real keyboard and its real theme. Headless behaves
   as iOS on a phone-sized surface; a desktop-only bug (a header, a hover, a shortcut) never shows up headless.

The rules that make a change good — state ownership, stable identity, the press/keyboard/focus/accessibility
contract, theme tokens instead of literals, `List` for long data — are in `references/pitfalls.md`.

## Where to read more

In an NSH checkout (the directory that holds `AGENTS.md` — find it with `git rev-parse --show-toplevel`):

| Topic | File |
|---|---|
| The CLI, every flag | `website/docs/cli.md` |
| Getting started, running on each platform | `website/docs/getting-started.md` |
| Platforms and what differs | `website/docs/platforms.md`, `website/docs/platform-specific.md` |
| The web target in depth | `website/docs/web.md` |
| Installing the tools per platform | `website/docs/installation.md` |
| The dev server and debugging | `website/docs/dev-server-and-debugging.md` |
| Performance | `website/docs/performance.md` |
| Text and fonts | `website/docs/text.md`, `website/docs/fonts.md` |
| Touch and gestures | `website/docs/touch-and-gestures.md` |
| Canvas, web views and video | `website/docs/canvas.md`, `website/docs/web-view-and-video.md` |
| Architecture | `website/docs/architecture.md` |
| Migration and compatibility | `website/docs/comparison.md`, `website/docs/compatibility.md` |
| The files of an app, every `nsh.json` key | `website/docs/project-structure.md` |
| Reactivity | `website/docs/reactivity.md` |
| JSX and components | `website/docs/jsx-and-components.md` |
| Styling, colors, shadows, effects | `website/docs/styling.md` |
| Tailwind, variants, theme tokens | `website/docs/tailwind.md` |
| Layout, flexbox and grid | `website/docs/layout.md` |
| Lists and virtualization | `website/docs/lists.md` |
| Scrolling | `website/docs/scrolling.md` |
| Navigation | `website/docs/navigation.md` |
| Forms, inputs and the keyboard | `website/docs/forms-and-keyboard.md` |
| Animation | `website/docs/animation.md` |
| Images, fonts and assets | `website/docs/images-and-assets.md` |
| Networking | `website/docs/networking.md` |
| The platform's environment, dark mode | `website/docs/device-and-environment.md` |
| Accessibility | `website/docs/accessibility.md` |
| Native components | `website/docs/native-components.md` |
| Building with Rust | `website/docs/rust.md` |
| Testing and trace | `website/docs/testing-and-trace.md` |
| Packaging | `website/docs/packaging.md` |
| Signed packages | `website/docs/signed-packages.md` |
| The sandbox and capabilities | `website/docs/sandbox-and-capabilities.md` |
| Embedding (iOS / Android / desktop / web) | `website/docs/ios.md`, `android.md`, `macos.md`, `windows.md`, `linux.md`, `gpui.md`, `web.md` |
| Coding and design rules | `website/docs/coding-guides.md`, `website/docs/design-guides.md` |
| The API reference, per module | `website/modules/<module>.md` (e.g. `nsh.md`, `nsh-base.md`, `nsh-list.md`) |
| The element reference, per element | `website/elements/<element>.md` |

This skill's own references: `references/tsx.md`, `references/rust.md`, `references/testing.md`,
`references/embedding.md`, `references/cli.md`, `references/pitfalls.md`.
