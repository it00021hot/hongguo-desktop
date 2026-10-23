# Rust reference

The Rust side of an NSH app: the primitives and the typed styles (`nsh-core`), the unstyled base components
(`nsh-base`), and the macros that turn a crate's components, modules and classes into something TSX imports.

## The two crates

```rust
use nsh_core::ui::prelude::*;              // primitives, signals, typed styles, macros
use nsh_base::{button, theme};              // base components and the theme (only what you use)
```

`nsh_core::ui::prelude` re-exports, among others:

| Group | Items |
|---|---|
| Elements | `view`, `text`, `image`, `input`, `scroll`, `canvas`, `overlay`, `web_view`, `video`, `native`, `virtual_list`, `input_accessory_view` |
| Macros | `component`, `module`, `class`, `native_module`, `methods`, `asset`, `assets`, `Props`, `PropEnum`, `Event`, `JsObject` |
| Reactive | `signal`, `signal_with`, `computed`, `effect`, `stored`, `batch`, `cleanup`, `untracked`, `provide_context`, `use_context`, `create_root` |
| Flow | `when`, `each`, `dynamic`, `catch_error` |
| Units | `px`, `rems`, `percentage`, `relative`, `auto`, `deg`, `radians` |
| Colors | `Hsla`, `Rgba`, `rgb`, `rgba`, `hsl`, `hsla`, `black`, `white`, `AdaptiveColor` |
| Ambient | `spawn`, `fetch`, `timer`, `set_timeout`, `set_interval`, `toast`, `next_frame`, `element_ref`, `window`, `safe_area`, `dark`, `locale`, `os`, `tokens`, `measure_text`, `on_key`, `on_back_press`, `clipboard_string`, `set_clipboard` |

## `#[component]`

A plain function, at most two parameters, and a **mandatory** return type.

```rust
#[component]
pub fn App() -> impl IntoElement { … }

#[component]
pub fn Chart(props: ChartProps, cx: Scope<ChartEvent>) -> impl IntoElement { … }
```

The macro generates the component type (`App`), its element type (`AppElement`) and a **snake_case constructor**
(`app()`), and implements `Element`, `ParentElement`, `IntoElement` and `ComponentBuilder`.

The `Scope<E>` parameter is recognized by its type path ending in `Scope`; without it the event type is `NoEvent`.

### Props

```rust
/// A price chart.
#[derive(Clone, PartialEq, Props)]
pub struct ChartProps {
    /// The symbol to chart.
    pub symbol: Prop<SharedString>,
    /// The range shown.
    #[prop(default = SharedString::from("1D"))]
    pub range: Prop<SharedString>,
}
```

- Named fields only. `Props: Clone + PartialEq + 'static`.
- Field names become **camelCase** in TSX; `#[prop(rename = "…")]` overrides.
- **`Prop<T>` is held and reactive** — read with `props.x.get()` in a binding. A **plain field is read once**. Use
  `Prop<T>` for anything that changes.
- `Option<T>` is optional in TSX. `Option<Slot>` is a **named slot**. `Option<Renderer<T>>` is a render function.
  `Children` is the JSX children. `Items` is virtualized rows (at most one, and not with `Children`).
- `#[prop(default)]` / `#[prop(default = expr)]` makes it optional and defaulted.
- A `Style`-typed field becomes a style prop.
- Field doc comments become the TSX prop docs; the struct's own doc comment stays Rust's.

Rust setters are generated per field: `chart().symbol("NSH").range("1M")`, a slot with `.header(content)`, a
renderer with `.render_item(|v| …)`.

### Children and slots

```rust
pub struct PanelProps {
    pub children: Prop<Children>,
    pub header: Prop<Option<Slot>>,
}

view()
    .child(props.children)
    .child(props.header.place(|h| h.px_4()))
```

`#[prop(lazy)]` on the children field makes JS build them only while the view places them.

### Events

```rust
#[derive(Event)]
pub enum ChartEvent {
    /// A price was selected.
    #[args(price)]
    Select(f64),
    /// The chart was cleared.
    Clear,
}
```

- A variant becomes `onSelect` / `onClear` in TSX. A unit variant is `() => void`; one unnamed field is the value;
  **named** fields are an object of camelCase fields.
- `#[args(name)]` names the handler's argument. Several unnamed fields need `#[args(open, reason)]`.
- `cx.emit(ChartEvent::Select(101.5))` sends it.
- Rust listens with `.on_select(move |price: &f64| …)` / `.on_clear(move || …)`.

## Typed styles

Named after the Tailwind utilities they match (`.p_4()` is `p-4`).

| Kind | Examples |
|---|---|
| Spacing / size steps | `.p_4()`, `.px_5()`, `.py_3()`, `.pt_2()`, `.m_2()`, `.gap_4()`, `.gap_x_2()`, `.w_full()`, `.h_10()`, `.size_8()`, `.min_w_0()`, `.max_w_96()`, `.top_0()`, `.inset_0()`, `.basis_1_2()` |
| Explicit values | `.px(px(20.0))`, `.gap(px(6.0))`, `.w(relative(0.5))`, `.rounded(px(10.0))` |
| Layout keywords | `.flex()`, `.flex_row()`, `.flex_col()`, `.flex_1()`, `.flex_auto()`, `.flex_none()`, `.grid()`, `.items_center()`, `.items_start()`, `.justify_center()`, `.justify_between()`, `.hidden()` |
| Shape | `.rounded_lg()`, `.rounded_xl()`, `.rounded_full()`, `.border_1()`, `.rounded(px(8.0))` |
| Text | `.text_size(px(28.0))`, `.font_weight(FontWeight::SEMIBOLD)`, `.tabular_nums()`, `.uppercase()`, `.underline()` |
| Colors | `.bg(color)`, `.text_color(color)`, `.border_color(color)` |
| Listeners | `.on_press(..)`, `.on_long_press(..)`, `.on_press_down(..)`, `.on_press_up(..)`, `.on_layout(..)`, `.on_touch(..)`, `.on_context_menu(..)`, `.on_mouse_move(..)`, `.on_hover_in(..)` |
| State styles | `.active(\|s\| ..)`, `.hover(\|s\| ..)`, `.focus(\|s\| ..)`, `.disabled_style(\|s\| ..)`, `.selected_style(..)`, `.checked_style(..)` |
| Conditional | `.style_when(condition, \|s\| ..)` (follows the condition), `.when(condition, \|v\| ..)` (decided once) |

Every setter takes `impl IntoReactive<T>`: a value, a signal, or a closure.

```rust
view()
    .flex_1()
    .gap_4()
    .bg(move || theme().primary)                 // follows the signal/theme
    .active(|s| s.opacity(0.6))                  // the state style
    .style_when(move || busy.get(), |s| s.opacity(0.4))
```

Two closure kinds, and they are not interchangeable: `move || value` binds a **value**; `|s| s.…` binds a **state
style** and receives a `StyleRefinement`.

### Units

| Unit | Constructor | Note |
|---|---|---|
| points | `px(20.0)` | a CSS `px` is a point here |
| rems | `rems(16.0)` | `1rem` is 16pt |
| percent | `percentage(0.5)` | a **fraction**: 0.5 is 50% |
| fraction of a reference | `relative(0.5)` | |
| auto | `auto()` | |
| angle | `radians(1.0)`, `deg(90.0)` | |

A bare `f32` is not a length: the public API is typed and the compiler says so.

### Colors

`Hsla` is the color code holds, passes and computes with. `Rgba` is only for conversions. `AdaptiveColor` is
`{ light, dark }` and is what a theme token is.

```rust
use nsh_core::color::{rgb, rgba, Hsla};

let brand: Hsla = rgb(0x7c3aed);
view().bg(brand)
```

## Signals

```rust
let count = signal(0_u32);              // Signal<T> is Copy
let quote = signal(SharedString::from("…"));

count.get()                            // read (subscribes)
count.set(1)
count.update(|value| *value += 1)
count.get_untracked()                   // read without subscribing
```

| API | What it is |
|---|---|
| `signal(v)` | A value that changes. `Signal<T>` is `Copy`. |
| `signal_with(v, equals)` | `SignalEquals::Never` notifies on every write |
| `computed(f)` | The memo: a value derived from signals. `Memo<T>`. |
| `effect(f)` | Runs now and again when what it read changes — for side effects. |
| `stored(v)` | Not reactive, not displayed: a handle to a value the UI does not show. |
| `batch(f)` | Applies the writes together. |
| `untracked(f)` | Reads without subscribing. |
| `cleanup(f)` | Undoes work before the next run or when the owner goes away. |
| `provide_context(v)` / `use_context::<T>()` | A value for everything below. |
| `create_root(f)` | An owner without a component. |

A prop is read with `props.x.get()` **in a binding** and `props.x.get_untracked()` **in a listener**.

## `nsh_base`

The unstyled base components. They own behavior, state, focus, keyboard and accessibility, and leave every visual
choice to the app.

| Rust | TSX | What it is |
|---|---|---|
| `button()` | `Button` | A press target with the disabled and selected states and a role |
| `checkbox()` | `Checkbox` | Checked / unchecked / indeterminate |
| `radio()`, `radio_group()` | `Radio`, `RadioGroup` | One option of a set, and the set |
| `switch()` | `Switch` | An on / off control |
| `tab()`, `tabs()` | `Tab`, `Tabs` | One tab and the tab list |
| `nav_stack()`, `nav_page()` | `NavStack`, `NavPage` | The navigation stack and its pages |
| `dialog()` | `Dialog` | Content over everything else |
| `alert_dialog()` | `AlertDialog` | The platform's alert or action sheet |
| `safe_area_view()` | `SafeAreaView` | A view padded clear of the system UI |
| `safe_area()`, `cx.insets()` | `safeArea()`, `safeAreaInsets()` | The insets themselves, reactive |
| `keyboard_avoiding_view()` | `KeyboardAvoidingView` | Keeps content above the software keyboard |
| `popover()` | `Popover` | Content anchored to a trigger |
| `menu()`, `context_menu()`, `menu_item()`, `menu_separator()`, `submenu()` | `Menu`, `ContextMenu`, `MenuItem`, `MenuSeparator`, `Submenu` | Menus |
| `tooltip()` | `Tooltip` | A hint next to its trigger |
| `sheet()` | `Sheet` | A panel at an edge over a backdrop |
| `text_view()` | `TextView` | A Markdown / HTML document |
| `selection_area()` | `SelectionArea` | One selection across the texts inside it |

```rust
alert_dialog()
    .title("Delete")
    .description("This cannot be undone.")
    .cancel("Cancel")
    .destructive("Delete")
    .show();   // async: resolves with the chosen label
```

### `theme()`

`theme() -> Arc<Theme>` — reading it subscribes, so a theme change reaches what read it. **Read it inside a binding.**

```rust
view()
    .bg(move || theme().background)
    .child(text("Hi").text_color(move || theme().foreground))
```

Fields: `background`, `foreground`, `card`, `card_foreground`, `popover`, `popover_foreground`, `primary`,
`primary_foreground`, `secondary`, `secondary_foreground`, `muted`, `muted_foreground`, `accent`, `accent_foreground`,
`destructive`, `border`, `input`, `ring`, `chart_1`…`chart_5`, `sidebar`, `sidebar_foreground`, `sidebar_primary`,
`sidebar_primary_foreground`, `sidebar_accent`, `sidebar_accent_foreground`, `sidebar_border`, `sidebar_ring`,
`radius: Points`, `font_sans`, `font_mono`. The app's own tokens: `theme().color("up")`,
`theme().number("radius-card")`, `theme().font("…")`.

## Exporting to TSX

```rust
// src/lib.rs — the crate's root
use nsh_core::ui::prelude::*;

#[native_module("my-app/native")]
fn native(registry: &mut Registry) {
    registry
        .register::<Gauge>()   // a component
        .module::<Ledger>()    // a module of functions
        .class::<Account>();   // a class
}
```

- `#[native_module("…")]` must be **at the crate's root**; the specifier is a bare import specifier
  (`my-app/native`), not a path, and not `nsh`/`nsh/…`. One specifier per crate.
- The export name is the type's own name — nothing is written by hand.
- `registry.assets(assets!())` ships the crate's `assets/` directory.

### `#[module]` — functions

```rust
/// Money helpers.
#[module]
pub struct Ledger;

#[module]
impl Ledger {
    /// `amount` as money: `$1,234.50`.
    pub fn format(amount: f64) -> String { … }

    /// Puts `text` on the clipboard; resolves with the number of characters copied.
    pub fn copy(text: String, cx: &mut ModuleCx) -> u32 { … }

    /// Today's rate, from a round trip of 300 ms.
    pub async fn fetch_rate() -> f64 { … }
}
```

Three call kinds: no `cx` and not `async` runs inside the JS call and returns its value; a last `cx: &mut ModuleCx`
parameter makes it an op (run in order with the UI changes) and returns a `Promise`; `async fn` is a task of the
core. A doc comment on the `impl` block is a compile error (TypeScript would not show it).

### `#[class]` — objects

```rust
/// An account kept in Rust.
#[class]
pub struct Account {
    name: String,
    balance: f64,
}

#[class]
impl Account {
    /// A new account named `name`.
    pub fn new(name: String) -> Self { … }

    /// The balance.
    #[getter]
    pub fn balance(&self) -> f64 { self.balance }

    /// Renames the account.
    #[setter]
    pub fn set_name(&mut self, name: String) { self.name = name; }

    /// Adds `amount`; the new balance.
    pub fn deposit(&mut self, amount: f64) -> Result<f64, String> { … }

    /// Calls `listener` with the new balance after every change.
    pub fn on_change(&mut self, #[args(balance)] listener: Callback<f64>) { … }

    /// Adds a month's interest at `rate`.
    pub async fn accrue(this: Instance<Self>, rate: f64) -> f64 { … }
}
```

`new` is the constructor, `&self`/`&mut self` methods, no receiver a static, `#[getter]`/`#[setter]` a property,
`async fn` (with `this: Instance<Self>` first) a promise member. A `Callback<A, R>` parameter is a JS function.

### From TSX

```tsx
import { Gauge, Ledger, Account } from "my-app/native";

const account = new Account("Ada");
<Gauge value={0.5} onChange={(v) => account.deposit(v)} />
```

The declarations are generated into `.nsh/types` from the Rust types, doc comments included.

## Mounting a Rust root

```rust
nsh_core::ui::Root::of::<my_app::App>()          // the app's root component
    .app_id("com.example.my-app")
    .appearance(nsh_core::ui::Appearance::System)
    .tokens(theme_tokens)
```

`nsh.json`'s `"root"` names the component by its path in the crate (`App`, `shell::Gallery`); with several crates it
is a component of the first one listed. `nsh run` and `nsh package` of the app's directory mount it with no QuickJS,
on every platform.

## The `mixed` route

`nsh.json` has `"rust": "."` and **no** `"root"`, so `App.tsx` stays the entry and imports the crate:

```tsx
import { Badge, Counter, Format } from "my-app/native";
```

The Rust crate's `#[native_module("my-app/native")]` decides the specifier; `nsh init --template mixed` writes both
sides already wired.

## Testing a Rust root

In `cargo test`:

```toml
[dev-dependencies]
nsh-engine = "0.1"
nsh-testing = "0.1"
```

```rust
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[test]
fn a_tap_counts() {
    let mut app = Headless::start(EngineConfig::new(Root::of::<my_app::App>())).unwrap();
    app.wait_for("Count: 0").unwrap();
    app.press("Tap").unwrap();
    app.wait_for("Count: 1").unwrap();
}
```

`Headless` has `start`, `screen`, `shows`, `press`, `long_press`, `clear`, `type_text`, `submit`, `scroll`,
`wait_for`, `wait_for_within`, `wait_for_gone`, `wait_until`, `wait`, `run_for`, `fast_forward`,
`set_system_time`, `set_viewport`, `set_environment`, `on_dialog`, `pending_dialog`, `answer_dialog`,
`set_clipboard`, `first_render`, `back`.

To test a TSX entry against the crate's set:

```rust
let sets = vec![nsh_base::registry(), my_app_native::registry()];
let mut app = nsh_testing::tsx("../App.tsx", sets).unwrap();
```

`nsh_testing::compile_tsx(entry, &sets)` builds the entry once for several tests.

One integration test binary per crate: `tests/<name>/main.rs` with a module per area.
