# Pitfalls and the rules that make a change good

The list NSH's own coding guide keeps, plus the traps that cost the most time. Read the coding guide
(`website/docs/coding-guides.md`) and the design guide (`website/docs/design-guides.md`) for the full text.

## Before editing

An agent must read the nearest implementation, its tests, the module's public exports and the relevant reference page.
It must search the current source and `.nsh/types` for signatures instead of translating a React, React Native, CSS
or GPUI example by analogy: **NSH components run once**, and bindings follow the signals they read.

"Compiles" is not a UI quality bar. Generated code must be reviewed and tested by a person.

## The traps that cost the most time

| Trap | What happens instead |
|---|---|
| Reading a signal in a component's body | The value is a snapshot; the UI never updates. Read it in JSX (`{count()}`) or in a closure. |
| Destructuring props (`const { label } = props`) | The props are read once and lose their updates. Use `split(props, keys)`. |
| Writing text straight in a `<View>` | A build error in TSX. Text lives in `<Text>`. |
| A lowercase tag (`<view>`) | A compile error. Every tag names a binding in scope. |
| `useState` / `useEffect` / `key` | There is no React. `signal` / `computed` / `effect` / `<Each key=…>`. |
| `import { … } from "nsh/list"` | Deprecated: import the primitives from `nsh`. |
| `import … from "solid-js"` | Refused by the bundler: `nsh`'s own reactive API replaces it. |
| `import … from "node:fs"` | Refused. The standard modules use bare specifiers (`fs/promises`, `net`, …). |
| `document`, `window`, `node_modules` | There is no DOM. Web APIs (`fetch`, `URL`, streams) exist; Node's do not. |
| A bare `f32` for a length in Rust | Does not compile: lengths are typed (`px`, `rems`, `percentage`, `auto`). |
| `theme()` read outside a binding in Rust | A theme change never reaches it. Read it as `move \|\| theme().primary`. |
| A doc comment on a `#[module]` / `#[class]` `impl` block | A compile error (TypeScript would not show it). |
| `#[native_module]` not at the crate's root | The build fails: move it to `src/lib.rs`. |
| `Prop<T>` vs a plain field in Rust | A plain field is read **once**. Use `Prop<T>` for anything that changes. |
| An `<Each>` over objects with no `key` | The key is the item's identity. A delete that rebuilds the array makes every row a new row. Give it `key={(item) => item.id}`. |
| `nsh run` for a Rust app expecting hot reload | A Rust edit **rebuilds and restarts**; only TSX hot-updates. |
| `nsh build` and trusting it to catch type errors | It does not. Only `nsh check` checks types. |
| Judging a UI change headless only | Headless is iOS on a phone-sized surface: a header, a hover or a shortcut never shows up. Run `nsh test --target <platform>`. |
| A test that only calls internal functions | It proves nothing. A test presses, types or scrolls. |

## Patterns to avoid

- One signal holding the app's unrelated state.
- Business logic and network requests inside a long component body.
- Random or index-based keys for reorderable content.
- Per-row state in signals created inside a `List` row.
- Reading a prop or a signal in a component body and expecting it to update.
- Literal colors and radii that break custom themes and dark mode — use the theme's tokens.
- A custom pressable view where a base component already supplies focus, keyboard, disabled and accessibility behavior.
- Duplicated local state that drifts from a controlled value.
- Effects that copy one signal into another, and writes that loop through a binding.
- A decision the system needs on the spot left to a callback, instead of a prop set beforehand.
- Nested scroll views without an explicit owner.
- A new component variant for a one-off screen.
- A confirmation dialog for a reversible, low-risk action.

## What to name for each change

Before opening a change for review, be able to name:

1. the **behavior owner** and the **presentation owner**;
2. the **retained identity** and the **state lifecycle**;
3. the **press, keyboard, focus and accessibility contract**;
4. the **layout and scrolling owner**;
5. the **theme tokens** and the intentional exceptions to them;
6. the **test that would fail if the behavior regressed**.

## The checklist

- State and side-effect ownership are explicit.
- A plain function versus a component is chosen deliberately, and props and signals are read where they are used.
- Repeated elements have stable domain-based keys and test ids.
- Theme tokens and the Tailwind scale replace isolated visual literals.
- Presses, keyboard shortcuts, focus, the disabled state and overlays work together.
- Decisions the system needs on the spot are props, set beforehand.
- Loading, empty, error and cancellation paths are represented.
- Long data sets use `List`, with the per-item state in the data.
- Public API additions only widen.
- Tests prove behavior at the right layer, headless and on the platforms the change touches.
- `nsh check`, the targeted tests and the relevant examples pass.

## Performance

NSH's first priority. The rules that matter most in an app:

- **JS does as little as possible.** Parsing, diffing and string processing happen in Rust. Move heavy work to Rust
  or off the interaction path.
- **Cross threads in batches.** One JS turn → its ops queued natively → one commit → one mount batch. Never cross per
  item.
- **Do not send mount items that are not needed.** New views get no default style and no empty event mask; unchanged
  styles and frames are not sent. Check the item counts with `nsh test --trace`.
- **Long data goes in `List`**, which creates the rows near the viewport and recycles them.
- **Measure before caching.** `nsh test --trace` first; a cache needs a clear invalidation owner.
- **Animation work stays bounded**: animated values on the UI thread rather than signals written every frame, and
  honor reduce motion.

The baselines live in `docs/dev/performance.md`.

## Accessibility and interaction

- A pressable view needs a role and a label (`role="button"`, `accessibilityLabel`), or it is invisible to a screen
  reader.
- The `Button`, `Checkbox`, `Radio`, `Switch` and `Tab` base components already own focus, the disabled state and the
  role. Prefer them over a hand-rolled pressable view.
- Every interactive element needs a visible focus state (`focus:` classes or `focusStyle`).
- A state change must be understandable before the click ("Make the result understandable before the click").
- Honour reduce motion: `reduceMotion()` from `nsh/accessibility`.
