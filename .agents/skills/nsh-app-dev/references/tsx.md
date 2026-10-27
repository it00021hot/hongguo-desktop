# TSX reference

Everything an app's TypeScript side touches. The authoritative source is the generated declarations in the app's
`.nsh/types/` (`nsh/types.d.ts`, `nsh/base.d.ts`, one file per module) — read them for the exact prop types rather
than guessing.

## The modules you can import

`nsh` and `nsh/…` are served by the runtime, not bundled. Importing a module NSH does not have (`nsh/foo`) is a build
error, and so is a name the module does not export (the error names the closest export).

| Module | What it has |
|---|---|
| `nsh` | `signal`, `computed`, `effect`, `batch`, `untracked`, `cleanup`, `context`, `mount`, `split`, `token`; the host elements `View`, `Text`, `Scroll`, `Input`, `Overlay`, `Image`, `List`, `Canvas`, `WebView`, `Video`; the types (`ViewProps`, `TextProps`, `ImageProps`, `ScrollProps`, `InputProps`, `CommonProps`, `ViewStyle`, `TextStyle`, `StyleProp`, `ColorValue`, `LengthValue`, `YNode`, `Point`, `Size`, `Bounds`, `Edges`, `AccessibilityRole`, `AccessibilityState`, `OS`, `GestureResponderEvent`, …) |
| `nsh/base` | `Button`, `Checkbox`, `Radio`, `RadioGroup`, `Switch`, `Tab`, `Tabs`, `Collapsible`, `Progress`, `ResizablePanel`, `ResizablePanelGroup`, `DockArea`, `NavStack`, `NavPage`, `createNavStack`, `Dialog`, `SafeAreaView`, `safeArea`, `safeAreaInsets`, `AlertDialog`, `Keyboard`, `keyboardFrame`, `KeyboardAvoidingView`, `Popover`, `Menu`, `ContextMenu`, `MenuItem`, `MenuSeparator`, `Submenu`, `Tooltip`, `Sheet`, `TextView`, `SelectionArea` |
| `nsh/animated` | Native animations: `AnimatedValue`, springs, timelines |
| `nsh/gesture` | `PanResponder` |
| `nsh/device` | `platform`, `windowSize`, `colorScheme`, `fontScale`, `pixelRatio`, `locale` |
| `nsh/accessibility` | Screen-reader state, reduce motion, announcements |
| `nsh/native` | The components and functions the embedding app registers |
| `nsh/ios`, `nsh/android` | Platform-only APIs |
| `nsh/storage` | `Storage` — the app's persistent key/value store (`getItem`, `setItem`, …) |
| `nsh/clipboard` | `Clipboard` — `writeText`, `readText` |
| `nsh/keymap` | `keymap` — bind keystrokes |
| `nsh/desktop` | `Window`, `titleBar`, `isAvailable` — the desktop's own window chrome |
| `nsh/image`, `nsh/list`, `nsh/canvas`, `nsh/webview`, `nsh/video` | The primitives' own pages (deprecated — import them from `nsh`) |

The eleven **standard modules** use bare specifiers: `buffer`, `console`, `crypto`, `fs/promises`, `net`, `os`, `path`,
`process`, `url`, `websocket`, `zlib`. `node:` prefixes and other Node modules (`fs`, `events`, `util`, …) are refused.

`nsh/test` is served **only** to test files — an app's own bundle cannot import it.

## The reactive API

```ts
function signal<T>(value: T, options?: { equals?: false | ((a: T, b: T) => boolean) }): Signal<T>;
interface Signal<T> {
  (): T;                              // read (subscribes, inside a tracking scope)
  set(value: T): void;
  update(fn: (value: T) => T): void;
}

function computed<T>(fn: (previous: T | undefined) => T, options?): Accessor<T>;
function effect(fn: () => void): void;
function batch<T>(fn: () => T): T;
function untracked<T>(fn: () => T): T;
function cleanup(fn: () => void): void;
function context<T>(defaultValue: T): Context<T>;      // Context<T> is an Accessor<T> with .Provider
function mount<P>(app: Component<P>, props?: P): () => void;
function split<T, K extends readonly (keyof T)[]>(props: T, keys: K): [Pick<T, K[number]>, Omit<T, K[number]>]>;
function token<K extends keyof Tokens>(name: K): Tokens[K];
```

### How a component stays reactive

A component function runs **once**. What updates is the **binding**: an expression inside JSX, a `computed`, an
`effect`, or a closure passed to a style or a prop.

```tsx
function Counter() {
  const count = signal(0);

  // ✗ a snapshot: the body runs once, `count` never changes here
  const label = `Count: ${count()}`;

  // ✓ the expression is a binding: it re-runs when `count` changes
  return <Text>Count: {count()}</Text>;
}
```

The same in a style object: `style={{ opacity: busy() ? 0.5 : 1 }}` is reactive; `const o = busy() ? 0.5 : 1` is not.

Props are read **where they are used**, not in the body. A component that handles some props itself and passes the
rest on uses `split`, which keeps both halves reactive:

```tsx
function LabeledInput(props: InputProps & { label: string }) {
  const [own, rest] = split(props, ["label"]);
  return (
    <View style={{ gap: 4 }}>
      <Text>{own.label}</Text>
      <Input {...rest} />
    </View>
  );
}
```

### Effects and cleanup

`effect` runs after rendering and again whenever a signal it read changes. It belongs to the component (or effect) it
was created in and is disposed with it. `cleanup` registers what to undo before the next run or when the owner goes
away.

```tsx
function Clock() {
  const now = signal(new Date());
  const timer = setInterval(() => now.set(new Date()), 1000);
  cleanup(() => clearInterval(timer));
  return <Text>{now().toLocaleTimeString()}</Text>;
}
```

Never write one signal from another inside an effect (a feedback loop). Compute instead:

```tsx
const total = computed(() => items().reduce((sum, item) => sum + item.price, 0));
```

### Context

```tsx
const Theme = context<"light" | "dark">("light");

function App() {
  const dark = signal(false);
  return (
    <Theme.Provider value={dark() ? "dark" : "light"}>
      <Label text="Hello" />
    </Theme.Provider>
  );
}
```

## Host elements

| Element | What it is | Key props |
|---|---|---|
| `View` | The container: layout, background, borders, shadows, press, gestures | `onPress`, `onLongPress`, `onLayout`, `onContextMenu`, `hitSlop`, `disabled`, `role`, `accessibilityLabel`, `testID`, `ref` |
| `Text` | A run of text | `numberOfLines`, `onPress`, plus the text styles (`fontSize`, `fontWeight`, `color`, `textAlign`, …) |
| `Scroll` | A scrollable view | `onScroll`, `scrollEnabled`, `contentContainerStyle`, `ref` |
| `Input` | A text field | `value`, `onChangeText`, `placeholder`, `onSubmit`, `secureTextEntry`, `keyboardType`, `autoFocus`, `ref` |
| `Image` | An image | `src` (a URL, an asset import, `asset:/…`, a `data:` URI or a `blob:` URL), `resizeMode`, `alt`; the `objectPosition` style decides where it sits in its box |
| `List` | A virtualized list (long data only) | `data`, `renderItem`, `keyExtractor`, `onEndReached` |
| `Overlay` | Children over the window, placed against an anchor | `anchor`, `placement` |
| `Canvas` | A drawing surface | `onPaint`, `width`, `height` |
| `WebView` | A web view | `source`, `onLoad`, `onMessage` |
| `Video` | A video | `source`, `onPlay`, … |

Read `website/elements/<element>.md` for a full prop list.

### Press events carry the point

`onPress`, `onLongPress`, `onPressDown` and `onPressUp` receive an optional `PressEvent`:
`{ position, pagePosition }`. `position` is in the pressed view with its transforms undone; `pagePosition` is in the
root view (the surface or its modal), the same coordinates touches use — that is what `Menu.popup(items, at)` takes.
The payload is **absent on backends that do not know the pointer**: in 0.2.0 only the GPUI backend and the headless
driver send it, so every other backend (iOS, Android, macOS, Windows, Linux) leaves the argument `undefined`. Treat it
as optional:

```tsx
<View onPress={(e) => place(e?.position)} />
```

In Rust, `on_press` still takes no value; a listener that wants the point reads the raw payload.

### Layout defaults

Flexbox, as in CSS, with NSH's defaults: `flexDirection: "column"`, `flexShrink: 0`. `flex` is a **row**;
`flexDirection: "row"` is the same. Children stack in a column by default. `gap` between children, `justifyContent`
along the main axis, `alignItems` across it.

## Styling

### `style` — a style object

CamelCase keys, the values of CSS in a browser, **numbers in points**.

```tsx
<View
  style={{
    flex: 1,
    padding: 24,
    gap: 12,
    backgroundColor: "#ffffff",
    borderRadius: 16,
    flexDirection: "row",
    alignItems: "center",
    boxShadow: "0 4px 12px rgba(0, 0, 0, 0.08)",
  }}
/>
```

A style prop accepts one style or a list merged left to right, skipping falsish entries:

```tsx
<View style={[styles.card, selected() && styles.selected]} />
```

State styles are their own props: `activeStyle`, `hoverStyle`, `focusStyle`, `disabledStyle` (and `selectedStyle` on
the base components that have the state).

### `class` — Tailwind

```tsx
<View class="rounded-lg px-4 py-2 transition-colors" classList={{ "bg-indigo-600": selected(), "opacity-50": busy() }} />
```

- `class` may be dynamic (`` class={`h-6 rounded bg-${color()}-500`} ``) — classes resolve at run time.
- `classList` maps a class to a condition and applies after `class`.
- Later classes in the same string win (`"p-2 p-4"` is `p-4`). No `tailwind-merge` needed.
- Variant classes apply after plain ones, in Tailwind's variant order, wherever they sit in the string.
- **`style` wins over classes.**

### What differs from the web

- **Units are points.** `p-4` is 16pt, `1rem` is 16pt, `p-[13px]` is 13pt.
- `flex` is a row (as in CSS). A view with no `flex` is a column.
- **Text utilities on a view are inherited** by the texts inside it, as in CSS: `<View class="text-white">` makes its
  labels white.
- `space-x-*` / `space-y-*` are `gap-x` / `gap-y`.
- A state background replaces a gradient while it holds (in CSS a gradient would cover it).
- Utilities that are props in NSH (`line-clamp-*`, `truncate`, `pointer-events-*`, `object-*`, `select-text`,
  `placeholder:text-*`) lose to the explicit prop.
- `peer-*`, `before:`/`after:` and arbitrary properties (`[mask-type:…]`) are **unknown classes** and are reported.
  Web-only utilities that need nothing natively (`antialiased`, `will-change-*`, `touch-*`, `snap-*`, …) are accepted
  and ignored.
- Everything else behaves as Tailwind 4 documents it: the palette, opacity modifiers (`bg-black/40`), arbitrary
  values, fractions, shadows and rings, gradients (`bg-linear-to-r`, `from-`/`via-`/`to-`), filters, transforms,
  transitions, `animate-*`, and grid.

### Effects: filters, glass and masks

```tsx
<View style={{ backdropFilter: "blur(30px)", maskImage: "linear-gradient(#000 70%, #00000000)" }} />
<View style={{ backdropFilter: "blur(12px)", glassEffect: "regular" }} />
```

- `filter` / `backdropFilter` are CSS filter strings; `backdropFilter` is what frosted glass is made of.
- `glassEffect: "regular" | "clear"` draws Apple's Liquid Glass behind the view's content on **iOS 26 and later only**;
  every other platform draws `backdropFilter` as before, so always give a glass view a blur to fall back to.
- On iOS, `ios={{ backdropMaterial: "regular" }}` picks the system material a `backdropFilter` blur uses
  (`ultraThin` default, `thin`, `regular`, `thick`, `chrome`). Pair it with a positive radius; the radius approximates
  strength, full at 30 points.
- `maskImage` takes a CSS **gradient** whose alpha decides how much of the view is drawn, and it masks everything the
  view draws — its backdrop filter included. That is how a blurred bar fades out instead of cutting off hard. iOS and
  web draw it; other backends ignore the key for now. Gradients only: an image or a shape there is an invalid value.

### Theme and tokens

`nsh.json`'s `"theme"` holds the app's design tokens (shadcn/ui's `cssVars`, over the default `neutral` theme); the
token classes name them directly and follow dark mode by themselves:

```tsx
<View class="bg-background text-foreground" />
<Text class="text-primary">…</Text>
<View class="rounded-(--radius)" />   {/* the token in a CSS-value position */}
```

`token("primary")` reads one in code; the reading is reactive (a theme or appearance change re-runs what read it).

`nsh.json`'s `"tailwind"` extends the Tailwind theme the classes resolve against:

```json
{
  "tailwind": {
    "colors": { "brand": { "500": "#7c3aed", "DEFAULT": "#7c3aed" }, "ink": "#1f2328" },
    "radius": { "card": 14 },
    "fontFamily": { "display": "Pacifico" },
    "breakpoints": { "tablet": 600 }
  }
}
```

A typo in `nsh.json` is a **build error**, not a setting that silently does nothing.

## Control flow

`<If>` and `<Each>` are globals — no import. A binding of the same name in scope takes their place.

```tsx
<If when={user()} else={<Text>Signed out</Text>}>
  {(u) => <Text>Hello, {u().name}</Text>}
</If>

<Each of={todos()} key={(t) => t.id}>
  {(todo, index) => (
    <View style={{ flexDirection: "row", gap: 8 }}>
      <Text>{index() + 1}.</Text>
      <Text>{todo().title}</Text>
    </View>
  )}
</Each>
```

- The row function receives **accessors**: call `item()` and `index()`.
- `key` is the item's identity. **Give an object list a key** — `key={(item) => item.id}`. Without one the key is the
  item itself, by identity, so a delete that rebuilds the array (an immutable `{ ...item }` update) makes every row a
  new row and rebuilds them instead of moving them. Arrays of strings or numbers compare by value and need no key.
- Plain JavaScript inside JSX (`cond ? a : b`, `items.map(…))`) rebuilds the elements every time what it reads
  changes. `If`/`Each` keep the elements that stay.
- `Each` creates every row: right for tens or a few hundred items. For long or unbounded data use `List`, which
  creates the rows near the viewport and recycles them.

## Lists, navigation, forms

- **`List`** virtualizes: `data`, `renderItem`, `keyExtractor`. Per-row state lives in the data, not in a signal
  created inside a row.
- **`NavStack` / `NavPage` / `createNavStack`** are the navigation stack. `createNavStack` makes the stack's state;
  `NavStack` takes it, `NavPage` is a screen with a `header` slot.
- **`Dialog`** is content over everything else (sheets, full-screen flows). **`AlertDialog`** is the system's own
  alert / action sheet.
- **`SafeAreaView`** pads clear of the system UI; `safeArea()` / `safeAreaInsets()` give the insets themselves.
- **`Button`, `Checkbox`, `Radio`, `RadioGroup`, `Switch`, `Tab`, `Tabs`** are controlled: they report changes and
  the app owns the value.
- **`Collapsible`** is a disclosure region: its `children` always show (summary plus the trigger the app renders), its
  `content` shows after them while `open`. The content is made once and kept while collapsed, so what is inside keeps
  its state.
- **`Progress`** is a linear progress bar: `value` (0–100) sets the indicator's width and screen readers' value,
  `indeterminate` drops the value and leaves the indicator's width and motion to the app. `trackClass` /
  `indicatorClass` style the two parts; its children are the label.
- **`ResizablePanelGroup` / `ResizablePanel`** are panels side by side (`axis="horizontal"`) or stacked, with a
  handle the user drags. The group owns the drag, the limits and the cursor; the sizes are the app's (`size`, reported
  with `onResize`).
- **`DockArea`** is a dockable workspace: tab groups, splits and left / right / bottom docks. The layout is app data
  (`layout` + `onLayoutChange`); the area owns tab drags, drops, resizing, zoom and the lock, and switches to drawers
  and a bottom sheet under 600 points unless `compact` says otherwise.
- **`TextView`** renders Markdown / HTML documents.

## Assets

```tsx
import logo from "./assets/logo@2x.png";   // { url, size: { width, height }, scales? }
import Inter from "./assets/Inter.ttf";     // "Inter"

<Image src={logo} />
<Text style={{ fontFamily: Inter }}>Hello</Text>
```

An `@2x` / `@3x` suffix divides the pixel size by 2 / 3; a file without a suffix ships its density siblings, listed
in `scales`. Fonts register before the first frame. JSON files import as their parsed value.

## Networking

`fetch`, `URL`, `URLSearchParams`, `Headers`, `Request`, `Response`, `AbortController`, `TextEncoder` /
`TextDecoder`, `Blob`, `FormData`, `ReadableStream` / `WritableStream` and `WebSocket` are available. There is no DOM
and no Node.

`nsh.json`'s `"capabilities"` decides what the app may reach outside its VM (network hosts, web view hosts, the
status bar). Without the key an app may use the network freely; with it, everything not listed is denied.

The standard module `net` (a bare specifier, `import { … } from "net"`) is the low-level socket API, for the rare
app that needs a raw connection. `fetch` is the right tool for requests.

## Platform checks

```tsx
import { platform } from "nsh/device";

<View class={platform() === "ios" ? "pb-8" : "pb-2"} />
```

Prefer the `ios:` / `android:` / `web:` Tailwind variants over a runtime branch when the difference is only visual.
