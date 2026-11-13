# Rules for writing GPUI views in `app-gpui/` without a compiler

Worktree: /Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01 . The existing UI is a React/TypeScript webview in `src/`; the rewrite is the Rust crate `app-gpui/` (`openidea_gpui`, GPUI imported as `gpui_kit`). Goal: 1:1 functional parity with the webview, proven by Rust tests.

## Templates: read before writing

Views that built first time; copy their GPUI patterns:

- `app-gpui/src/everywhere/view.rs`, `everywhere/recent_view.rs`: a popup list with a search field and keyboard; key claims via `platform::input::claim_keys`.
- `app-gpui/src/find/popup_view.rs`: option toggles, hand-drawn checkbox, inline menus drawn with `deferred`, the single-line text field `app-gpui/src/ui/text_field.rs`.
- `app-gpui/src/project_tree.rs`, `app-gpui/src/services/view.rs`: trees with expand/collapse, selection, keyboard, toolbar, context-menu events.
- `app-gpui/src/run/view.rs`: tabs with status icons, toolbar buttons with enabled state, a long virtualised list of styled lines.
- `app-gpui/src/vcs/prompt_dialogs.rs`: small dialogs (title, message, fields with validation, checkboxes, buttons, Enter/Escape).
- `app-gpui/src/workbench/notifications_view.rs`: cards with buttons.
- `app-gpui/src/ui/popup_menu.rs` + `app-gpui/src/vcs_ops/menu.rs` (`MenuItem<C>`): menus the workbench shows; a view returns the items and gets the chosen command back.
- Text painting with styled runs: `app-gpui/src/editor/view.rs`.
  Theme tokens: `theme::color(cx, "…")`, `theme::length`, `theme::icon` (token names are the CSS variable names of `src/styles/tokens.css` and the component's CSS module; icons are in `app-gpui/assets/gen/icons.json`). Text: `cx.default_global::<Localization>().0.tx("…")` in views and `crate::platform::i18n::I18n` in models, with the same English keys the TSX uses. Click tests use `.debug_selector(|| "…")` and `debug_bounds`. Do NOT invent GPUI APIs you have not seen used in this crate: grep `app-gpui/src`, or read `~/.cargo/registry/src/index.crates.io-*/gpui-pre-0.3.12/src/`.

## Hard rules

- Create ONLY the new files your task names. Do not edit existing files, not even `mod.rs` files: list the `pub mod` lines in your report. Do NOT run cargo, rustc, rustfmt, git, node or pnpm: another agent owns the build. Reading and grepping is fine.
- If a model lacks something a view needs, do not edit it: write against what exists and list the exact addition in your report. Check what exists before writing (grep `app-gpui/src` for the subject).
- Views own no backend: they read models and report through events what the workbench must do (open a file at a line, run an action, show a menu at a point, load or save something).
- Keys arrive with GPUI's names (`up`, `down`, `left`, `right`, `enter`, `escape`, `backspace`, `delete`, `space`, `tab`, `home`, `end`, `pageup`, `pagedown`, `f2`, `f4`), never the browser's (`ArrowUp`, `Enter`…).
- Text offsets and columns are UTF-16 code units, lines 0-based.
- Production-ready, no stubs or TODOs. No `unwrap` outside tests. Doc comments say what the code does and why, present tense; never mention "port", "webview", "TypeScript", "React", "CodeMirror", "parity", tasks or history in comments or test names. Lines up to 120 columns. No new dependencies. Regexes: the `regex` crate (no look-around, no backreferences) in a `std::sync::LazyLock`. `localeCompare` sorting uses `crate::platform::collate`.
- Rust 2024; clippy runs with `-D warnings`: no needless clones/derefs/borrows/closures/lifetimes (`then_some` over `then(|| …)` for cheap values, `contains` over `iter().any(== )`, `std::slice::from_ref` over `[x.clone()]` in asserts, a `type` alias for complex types, a `Box` or `#[expect(clippy::large_enum_variant, reason = "…")]` for a lopsided enum); a closure that returns a reference derived from its argument does not compile (write a nested `fn`); a lazy iterator that borrows a value must be collected before that value is moved; do not mutate a list's state from inside its own scroll handler (defer it).
- GPUI test facts learned the hard way: `add_window_view` runs `run_until_parked` itself, so a view that emits in its first turn does so before a test can subscribe (emit on an explicit call instead); focus listeners only fire in an active window (`window.activate_window()` in the test); a view that takes focus inside a mouse-down handler loses it to a parent's `track_focus` unless the press is marked handled (`window.prevent_default()`); a selector passed to `debug_bounds` must be `&'static str`.
- Tests in each view file: `#[gpui_kit::test]` in the style of the templates: build the view in a test window on models filled by hand (the models' own tests show how), drive it with `simulate_keystrokes` / `simulate_click` / `simulate_input`, assert on public accessors you add for that purpose (`rows()`, `lines()`, `selected()`, `toolbar()` with enabled and pressed flags, …) and on the events emitted. A test that covers a case of a webview spec is NAMED AFTER THE SPEC'S SENTENCE in snake_case. Model tests are plain `#[test]`s, one per spec case, named the same way (an `it.each` table is a loop in one test).

## Report back (this is all the main session will see; exact, no narrative)

1. Files written; each view's constructor, events, accessors; the `pub mod` lines. Per spec file: cases covered / total, the rest with why.
2. The exact wiring for the workbench: which actions each view answers (ids and conditions from the feature's `index.ts`), where it is shown (tool window id from `TOOL_WINDOWS` / `tool_window_declaration` in `app-gpui/src/workbench/mod.rs`, the popup slot via `open_popup`, or a front-window layer as `workbench/run_configs.rs` does for its dialog), who owns the models, which backend operations and events (`ide_host::ops::*`, `ide_core` events) answer each event.
3. Every behaviour of the TSX you did NOT build, with why. Additions the models need. GPUI APIs used without a prior use in the crate. The tests most likely to fail on first run, with why.
