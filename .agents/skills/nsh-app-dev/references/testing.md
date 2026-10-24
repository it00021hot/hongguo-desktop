# Testing reference

Two ways to test an NSH app: `nsh/test` (TypeScript, works for every route) and `nsh_testing::Headless` (Rust, for a
Rust root). Both run headless with no window; `--target` runs the same tests in a real platform's window with its real
input.

## `nsh test`

```sh
nsh test                       # the test files in the current directory
nsh test my-app                # the test files under my-app
nsh test my-app/App.test.tsx   # one test file
nsh test my-app/App.tsx        # the app's entry, with the steps on standard input
nsh test my-app --filter cart  # only the tests whose name contains "cart"
nsh test my-app --trace        # every mount batch and every commit's time
nsh test my-app --junit out.xml
nsh test my-app --target macos # the same tests in a real window
```

Test files are `*.test.tsx` and `*.test.ts`, found recursively; hidden directories, `node_modules` and `target` are
skipped. Each test file runs on an engine of its own, and each app a test opens on another, fresh one. Any failure
makes the command exit non-zero.

## The `nsh/test` module

```ts
import { expect, open, test } from "nsh/test";
// also: describe, beforeAll, afterAll, beforeEach, afterEach
```

There is no default export. `nsh/test` is served **only** to test files — an app's own bundle cannot import it.

### Declaring tests

```ts
test(name: string, body: () => void, timeout?: number): void   // timeout in ms, default 30 000
describe(name: string, body: () => void): void
beforeAll(body: () => void, timeout?: number): void
afterAll(body: () => void, timeout?: number): void
beforeEach(body: () => void, timeout?: number): void
afterEach(body: () => void, timeout?: number): void

test.skip(name, body, timeout?)   test.only(name, body, timeout?)   test.todo(name)
describe.skip(name, body)         describe.only(name, body)
```

`describe` runs its body at once, as the file loads; its name prefixes its tests (`the cart › starts empty`).
`beforeEach` of outer blocks runs first; `afterEach` of inner blocks runs first. `.only` anywhere in the file skips
the file's other tests. A `test` or a hook declared **inside** a test or a hook is an error.

### Opening an app

```ts
const app = await open("./App.tsx", options?);
```

- The path is **relative to the test file**. Each distinct entry is built once per run.
- It resolves once the app has **rendered and is quiet** (nothing rendered for 100 ms, no native animation running).
- Each `open` is a **fresh engine**: tests never share state.
- In a Rust app (`nsh.json` `"root"`), `open(".")` starts the Rust root. In a mixed app, `open("./App.tsx")`.

```ts
interface OpenOptions {
  storage?: Record<string, string>;       // seeded into the app's storage
  network?: "live" | Record<string, StubResponse | ((req: StubRequest) => StubResponse | Promise<StubResponse>)>;
  width?: number;                         // points, default 390
  height?: number;                        // points, default 844
  safeArea?: { top?: number; right?: number; bottom?: number; left?: number };
  clipboard?: string;
  colorScheme?: "light" | "dark";          // default "light"
  locale?: string;                        // BCP 47
  reducedMotion?: boolean;
  platform?: "ios" | "android" | "macos" | "windows" | "linux" | "web";   // default "ios"
  clock?: { time?: Date | number };        // a manual clock
}

interface StubResponse {
  status?: number;                        // default 200
  headers?: Record<string, string>;
  body?: unknown;                         // a string, or a value sent as JSON
  error?: string;                         // the request fails instead
  delay?: number;                         // ms
}

interface StubRequest {
  method: string;
  url: string;
  headers: Record<string, string>;
  body?: string | null;
}
```

Without `network`, **every request fails** (nothing reaches the network in a test unless you ask for `"live"`).

Headless behaves as **iOS** unless `platform` says otherwise: `platform: "android"` gives the Android idiom,
`platform: "macos"` (or another desktop) gives a mouse and a keyboard, and `platform: "web"` gives a browser in the
desktop idiom. On an iOS or Android phone, `hover` and `rightClick` reject; the web takes them at any width.

The app runs **under the `capabilities` its `nsh.json` declares**, as on a device: a host it does not list is refused
in the test too.

### Selectors

| Selector | Matches |
|---|---|
| `id:<testID>` | the view with that `testID` |
| `label:<label>` | the view with that `accessibilityLabel` |
| `text:<text>` | a text that reads it; a span of a pressable text; an input holding it; an empty input with that placeholder |
| `role:<role>` | a view with that `role` |
| `role:<role>[<name>]` | that role **and** that name |
| `view:<id>` | the view with that node id (as `--trace` prints it) |
| anything else | a `testID`, else a label, else a text, as written |

- A **regex**: `text:/^Buy/i`, `label:/…/`, `role:button[/…/]`. The flags `i`, `m`, `s` apply.
- **Chaining** with ` >> ` looks *inside* what the previous part matched: `id:cart >> role:button[Remove]`.
- **Disambiguation** with `nth=<n>` (0-based): `text:Delete >> nth=1`.
- A selector only finds views a user would see: the top modal first, then what is on screen, and of a navigation
  stack only the top screen.
- An **action** (`press`, `type`, `clear`, `submit`, `scroll`, `longPress`) needs **exactly one** match; when several
  match, the error says how to narrow it down.

### The `app` object — actions

```ts
await app.press(selector)          await app.longPress(selector)
await app.type(selector, text)     await app.submit(selector)
await app.clear(selector)          await app.scroll(selector, distance)
await app.drag(selector, { x?, y? })
await app.hover(selector)          await app.rightClick(selector)     // desktops and the web only
await app.chooseMenuItem(label)    await app.closeMenu()
await app.wait(ms)                 await app.waitFor(selector, { timeout?, state?: "shown" | "gone" })
await app.waitUntil(() => boolean, { timeout? })
await app.back()                   await app.setViewport(width, height)
await app.setColorScheme("light" | "dark")
await app.screenshot(path?)        // PNG bytes; also written to path — needs a real platform
```

A step resolves once the app is quiet, so what it changed is on screen when the next line runs. A step rejects when
nothing matches its selector (the error lists the texts on screen), when the app threw meanwhile, or when the app
asked for something only a real platform answers.

### The `app` object — queries

```ts
app.count(selector)         // how many views match now
app.enabled(selector)       // an input that is not disabled or readonly
app.shows(selector)         // whether a view matches now
app.texts()                 // the texts on screen, in order
app.value(selector)         // an input's text, or null
app.storage()               // the app's storage now
app.dialogs()               // the dialogs shown: { title?, message?, buttons, cancelable, sheet }
app.menu()                  // the open context menu's actions: { label, disabled, checked? } | null
app.requests()              // the requests the app made, in order
app.clipboard()             // the clipboard now
```

Handlers and helpers:

```ts
app.onDialog((dialog) => "Cancel" | null);   // answer the next dialog
app.keyboard.press("mod+k");                 // "escape", "shift+tab", "shift+/", …
app.clock.runFor(ms);                        // a manual clock: advances it
app.clock.fastForward(ms);
app.clock.setSystemTime(new Date());
```

The clock exists only if `open` was given one (`{ clock: {} }`).

### Assertions

```ts
expect(value: unknown): Expectation
```

| Matcher | Checks |
|---|---|
| `toBe(expected)` / `toEqual(expected)` | the same check: JSON's comparison (undefined is null, objects member by member) |
| `toContain(item)` | an array holds the item, or a string contains it |
| `toBeTruthy()` / `toBeFalsy()` / `toBeNull()` | the value |
| `toMatch(pattern)` | a string contains the pattern, or matches the `RegExp` |
| `toHaveLength(n)` | an array's items, or a string's UTF-16 units |
| `toBeGreaterThan(n)` / `toBeGreaterThanOrEqual(n)` / `toBeLessThan(n)` / `toBeLessThanOrEqual(n)` | a number |
| `toThrow(message?)` | a **function** throws (a string means "contains", a `RegExp` means "matches") |

`.not` inverts every check. `await expect(() => app.press("Pay")).rejects.toThrow("is disabled")` checks that a
promise rejects. Every check throws on failure, with a message like
`expected ["Buy milk","1 item left"] to contain "2 items left"`.

`expect` given a function and a value check is an error: call it, or check what it throws.

### A test file

```tsx
import { expect, open, test } from "nsh/test";

test("counts taps", async () => {
  const app = await open("./App.tsx");
  expect(app.texts()).toContain("0");
  await app.press("label:Tap");
  expect(app.texts()).toContain("1");
});

test("keeps what was typed", async () => {
  const app = await open("./App.tsx", { storage: { todos: "[]" } });
  await app.type("id:field", "milk");
  await app.submit("id:field");
  expect(app.texts()).toContain("milk");
});
```

A failing test reports what was thrown, the frames in the test file (symbolicated) and the **texts on screen** of the
app it opened.

## Driving an app from standard input

`nsh test <entry>` reads steps from standard input. The text is inlined into a generated test, so it is JS source and
may `await` and call `expect`:

```sh
echo 'await app.press("text:+1"); expect(app.texts()).toContain("1");' | nsh test App.tsx
echo 'await app.scroll("id:photos", 3000)' | nsh test examples/gallery/Bench.tsx --trace
```

The available functions are the `app` methods above plus `expect`. The app is opened with `{ network: "live" }` and
waits 1.2 s first.

## `--trace`

`--trace` prints every mount batch of the apps the tests open, under the step it came in, and the time of every
commit:

```text
── press(Add): 7 items
── commit: 1.2 ms (js 0.3, layout 0.4, mount 0.5)
```

It is how you check that a change did not send more mount items or cost more time. The baselines live in
`docs/dev/performance.md`.

## On a real platform

`nsh test <app> --target macos|windows|linux|gpui|web|ios|android` runs the same test files in a real window, with
the platform's real input. Each `open` starts the app in a window of its own with a fresh data directory. A press is
a click at the view's middle, and a view off screen is scrolled into view first. The web needs a browser
(`--browser <PATH>`); iOS boots a simulator; Android uses the first device `adb devices` lists.

```sh
xvfb-run -a nsh test examples/gallery --target linux      # on a machine without a display
echo 'await app.screenshot("a.png")' | nsh test App.tsx --target macos
```

## Testing a Rust root

From TypeScript, in a Rust app: `open(".")` starts the root.

```tsx
import { expect, open, test } from "nsh/test";

test("counts taps", async () => {
  const app = await open(".");
  expect(app.texts()).toContain("Count: 0");
  await app.press("Tap");
  await app.press("Tap");
  expect(app.texts()).toContain("Count: 2");
});
```

From Rust, in `cargo test`: `nsh_testing::Headless`. See `references/rust.md`.

## What a test cannot do headless

- Images are not loaded: no `onLoad` / `onError`, and `Image.getSize` rejects.
- `app.screenshot()` needs a real platform.
- A press's **coordinates** only reach the app on backends that know the pointer: GPUI and the headless driver in
  0.2.0. Every other backend leaves the `PressEvent` argument `undefined`.
- Capabilities **are** applied: a test runs under the app's `nsh.json` `capabilities`, as on a device, so an undeclared
  host is refused. That is the one way to test capability rules without a device — and the reason a test that passed
  against an undeclared host now fails.
