# Brief for drafting tests (only tests) without a compiler

Worktree (all paths relative to it): /Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01 . The existing UI of this IDE is a React/TypeScript webview in `src/`; it is being rewritten as the Rust crate `app-gpui/` (`openidea_gpui`, GPUI imported as `gpui_kit`), which links the backend crates (`crates/ide-*`) directly. Parity with the webview is tracked row by row in `docs/gpui-port/INVENTORY.md`; a row is ticked only when a named Rust test proves it THROUGH THE APPLICATION: an action run through the registry (or its key pressed) in a test window, and a state a user can see asserted.

A sweep found rows whose native code exists and is reachable but that no test proves. The list, with a recipe per group, is `docs/gpui-port/notes/sweep/needs_test.md`. Your assignment names the groups that are yours. You write the tests; ANOTHER agent (the integrator, who owns cargo) compiles and runs them later. You do NOT run cargo, rustc, rustfmt, git, node or pnpm. You do NOT edit any existing file: another agent is compiling in this worktree right now and is editing `app-gpui/src/workbench/mod.rs` and other files.

## How the tests are packaged

- Tests go in NEW files that are child modules of an existing `mod tests`, so they can use its private helpers with `use super::*;`:
  - for `app-gpui/src/workbench/mod.rs` (inline `mod tests`): `app-gpui/src/workbench/tests/<name>.rs`, declared by adding `mod <name>;` inside that `mod tests`;
  - for a file `app-gpui/src/a/b.rs` with an inline `mod tests`: `app-gpui/src/a/b/tests/<name>.rs`, declared by `mod <name>;` inside its `mod tests`;
  - for a child module of the workbench with its own tests (`workbench/debug.rs`, `workbench/database.rs`…): `app-gpui/src/workbench/<child>/tests/<name>.rs`.
    Check the real layout before choosing the path (is `tests` inline in that file? is it `#[cfg(test)] mod tests;` pointing at a file already?).
- Read the neighbouring tests COMPLETELY before writing: how a test window with a project is built, the helpers (`run_action`, `wait_long`, `wait_for_notice`, `context_is_on`, `init_repo`, `git_in`, fixtures with a fake language server in `app-gpui/src/editor/intelligence.rs`, a fake debug adapter in `workbench/debug.rs`, a fake agent in the agents tests, SQLite in the database tests). Use only helpers and accessors that EXIST: read each signature. If a test needs an accessor that does not exist, do not invent it: either assert through what exists, or list the exact accessor to add (file, signature, body) in your notes and mark the test as depending on it.
- Each test asserts what a user sees or what is on disk, never that a function was called. A boolean context key is proven only by reading it ON and OFF. An action is proven only by running it through the registry (`run_action`) or pressing its key. An IPC row is proven only when the asserted state can only come from that backend operation, and the native path calls the same `ide_host::ops::…` function the adapter in `src-tauri/src/commands/` calls (check it).
- If, reading the code, you find the behaviour is NOT what the webview does (read the webview source for every recipe: the sweep's recipes are hypotheses), do not write a test that enshrines the difference: write the test the webview's behaviour demands and note the expected failure and the fix.
- GPUI test facts: keys arrive with GPUI's names (`up`, `enter`, `escape`, `backspace`, `f4`…); `add_window_view` runs `run_until_parked` itself; focus listeners only fire in an active window (`window.activate_window()`); `input::init` only while the `Keymap` global is missing; a double click is one press with `click_count: 2`; `debug_bounds` needs a `&'static str`; `set_text` on a text field does not emit the `Changed` a dialog reads (type with `simulate_input` instead); an open context menu swallows the next click (close it with Escape); an operation refreshes and THEN notifies (wait for the state you assert); tests wait with the existing wait helpers, never with fixed sleeps.
- Tests use temp directories only; no network, no real `gh`, no interactive shell, no system Trash, nothing under the user's home, never the user's keychain or the owner's installed app.
- Rust 2024, clippy `-D warnings` (no needless clones/borrows/lifetimes; `std::slice::from_ref`; a nested `fn` instead of a closure returning a reference). Test names are sentences in snake_case saying what is proven (the webview spec's sentence where the test covers a spec case). Doc comments say what the test proves, present tense; never mention "port", "webview", "TypeScript", "React", "parity", sweeps, rows or history. Lines up to 120 columns.

## Notes file (one, path in your assignment)

- The `mod` lines to add and where (quote the anchor line).
- Per test: name → the inventory rows it proves, each as the exact `parity.json` key (`action:<id>`, `ipc:<cmd>`, `context:<key>`, `event:<name>`, `module:<path>`, `unit:<spec path>`, `point.<registerFn>:<id>`, `feature:<name>`; read `docs/gpui-port/inventory.mjs` and existing entries of `docs/gpui-port/parity.json` for the formats) with one clause on what the test asserts that proves it, and whether the row becomes done or stays partial (and what is missing).
- Accessors or small fixes the tests depend on, as exact hunks. Recipes of your groups you did NOT write a test for, each with why (not reachable after all, needs a daemon, behaviour differs from the webview: say how).
- The tests most likely to fail on first run, with why.

## Final message (under 500 words)

Files written with line counts; number of tests; number of rows they would prove (done / partial); the hunks they depend on; what you left out and why; behaviours you found that differ from the webview.
