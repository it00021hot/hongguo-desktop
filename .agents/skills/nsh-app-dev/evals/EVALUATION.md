# Evaluation of the `nsh-app-dev` skill

The skill is evaluated against a baseline that runs the same tasks **without** it. Four tasks, one run each per
configuration.

| Eval | What it asks for | Assertions |
|---|---|---|
| `tsx-settings-screen` | A Settings screen (dark-mode switch, language dialog, saved items with delete, reachable from the home screen) in a TSX app | 8 |
| `rust-component` | A Rust `Rating` component (0..5 stars, reports the value) in a mixed app's crate, exported to TSX | 7 |
| `package-and-sign` | The id/version/icon in `nsh.json`, packages for four targets, and the signed-package chain | 6 |
| `ios-embedding` | Embedding a TSX module in an iOS host: artifacts, Swift, capabilities, limits, the trust store, the CLI | 6 |

Both configurations had the same environment: the fixture app, the checkout's `nsh` CLI
(`target/debug/nsh`) and the NSH checkout itself, with its guides and API reference.

## Result

**27/27 assertions in both configurations. No difference in pass rate.**

That is the honest headline, and it deserves an explanation rather than a victory lap: with the NSH checkout at
hand, the framework's own generated declarations (`.nsh/types`) and its guides carry most of the knowledge a model
needs. A model that knows nothing about NSH can still find the answer by reading the docs.

## Where the skill did change the outcome

1. **Verification depth.** Two of the four with-skill runs also ran the suite on a real platform
   (`nsh test . --target windows`) and with `--trace`. The baseline run of `tsx-settings-screen` states plainly that
   the app was never run outside the headless engine, so its desktop-only header was covered by the type check and
   the build alone.
2. **A robustness detail.** Only the with-skill run gave the `<Each>` list an explicit `key={(item) => item.id}`. The
   baseline relies on item identity surviving the delete — correct for its store, broken the moment items are rebuilt
   as new objects.
3. **A sharper "done" bar.** The skill's verification checklist names `nsh check` as the only type-checking command,
   asks for a test that presses, types or scrolls, and asks for `--trace` on hot paths. Both configurations met it
   here, but only the with-skill runs took the extra steps.

## Where the baseline was stronger

The baseline created a real `release.jks` with `keytool` and packaged the web target for real; it injected a
deliberate type error to prove `nsh check` really checks; it flagged that the display name still names the output
files. The skill added nothing the baseline could not reach.

## Conclusion

For someone who always has the checkout at hand, this skill is a **navigation and correctness shortcut** — the doc
map, the load-bearing rules, the pitfalls list — not a knowledge gap-filler. Its value rises sharply for an agent
working against an installed `nsh` with no checkout, and for the parts of the framework the docs state once and
nowhere else (the `#[component]` shape, the typed style API, the iOS embedding surface, the `nsh/test` API).

## Reusing the evals

- The prompts are in `evals/evals.json`; the assertions are in `evals/evals.json` and in each run's
  `eval_metadata.json`.
- Two fixes before the next run: the fixture was the stock `nsh init` counter while the prompt described a shopping
  list (both runs noticed and adapted); and the signing assertion names `--build 7` explicitly, while the with-skill
  run took the build number from `nsh.json` — equally correct, so re-word it as "publish carries the build number 7".
- The harness reported neither wall-clock time nor token counts for these subagent runs, so those columns are 0 and
  mean "not measured".
- A fifth eval, `web-target` (`id: 4` in `evals.json`), was added after this report: web development and packaging
  with the wasm toolchain prerequisites, the deploy serving rules, a `createPlayer` embed with limits, and a
  desktop-idiom layout branch. It is not covered by the numbers above.

The workspace of the run that produced this report is outside the repository, under
`%LOCALAPPDATA%\Temp\opencode\nsh-app-dev-workspace`.
