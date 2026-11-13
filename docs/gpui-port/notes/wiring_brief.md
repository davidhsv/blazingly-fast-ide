# Brief for drafting a feature's views AND workbench wiring without a compiler

Worktree (all paths relative to it): /Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01 . The existing UI of this IDE is a React/TypeScript webview in `src/`; it is being rewritten as the Rust crate `app-gpui/` (`openidea_gpui`, GPUI imported as `gpui_kit`) that links the backend crates (`crates/ide-*`) directly through `ide_host::Host` and `ide_host::ops::*`. The goal is 1:1 functional parity with the webview, proven by Rust tests that go through the application (an action run through the registry in a window with a project, then the visible state asserted).

You write code that ANOTHER agent (the integrator, who owns cargo) will compile later. You do NOT run cargo, rustc, rustfmt, git, node or pnpm. Reading and grepping is free. Everything you write must be exact enough to compile after small fixes: never invent an API; read the real signature first (in `app-gpui/src`, `crates/`, or `~/.cargo/registry/src/index.crates.io-*/gpui-pre-0.3.12/src/`).

## Read first, in this order

1. `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/view_rules.md`: the rules and templates for views, and the hard rules for code and tests. They all apply.
2. A wired feature, as the pattern to copy: `app-gpui/src/workbench/branches.rs` (Git operations, dialogs a Git operation waits on, notifications, tests on a temp repository), `app-gpui/src/workbench/commit.rs`, `app-gpui/src/workbench/log_window.rs`, `app-gpui/src/workbench/conflicts.rs`. Read how they: declare `impl Workbench` blocks in a child module; register actions (`register_*_actions` called from `mod.rs`); run backend work off the main thread and come back; show a menu (`Workbench::show_menu`), a popup (`open_popup`), a dialog (their own `deferred` layer below the popup slot); raise notifications; refresh the VCS state after an operation.
3. The tests at the bottom of those files and in `app-gpui/src/workbench/mod.rs` (`mod tests`): how a test window with a project and a temp git repository is built, `wait_for_notice`, how an action is run through the registry, how a dialog is answered. Tests wait for the state they assert (an operation refreshes and THEN notifies).
4. An example of integration notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/conflicts_integration.md`.
5. The webview sources of your assignment, completely, with their specs (`*.test.ts(x)` beside them), and the Rust models that already exist for them (`app-gpui/src/vcs/`, `app-gpui/src/vcs_more/`, `app-gpui/src/vcs_ops/`; grep before writing: much logic is already written and tested as models and only needs a view and wiring).

## What you write

- New view files where a dialog, popup or panel is needed (in `app-gpui/src/vcs/` unless told otherwise), each with its own `#[gpui_kit::test]`s as the rules say.
- ONE new child module of the workbench, named in your assignment (`app-gpui/src/workbench/<name>.rs`): `impl Workbench` blocks with the fields' types, the action registration, the handlers, the backend calls, and a `#[cfg(test)] mod tests` with end-to-end tests. Keep everything you can in this file; `mod.rs` gets only fields, a `mod` line and calls.
- ONE notes file: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/<name>_integration.md`, in the format of the example: the `pub mod` lines, every edit to a compiled file as an exact hunk anchored on distinctive quoted text (never line numbers; `mod.rs` moves), the menu entries to add (`app-gpui/src/workbench/menu_spec.rs` and context menus), what is not wired and why, your assumptions marked as assumptions, the compile risks.
- Do NOT edit any existing file. Do NOT touch files other agents are writing (other new `workbench/*.rs` drafts).

## Rules specific to wiring

- Every action id, its condition (`when`), its label and its menu place come from the webview source (`src/features/<feature>/…`, `src/platform/keymaps/`): copy them exactly. Register an action ONLY if one of your end-to-end tests runs it through the registry; list the ones you leave out.
- Use the backend operation the webview's `commands.<name>` maps to: find the IPC adapter in `src-tauri/src/commands/` and call the same `ide_host::ops::…` / crate function with the same arguments. Name each IPC command an action ends up calling in your notes (the parity records need them).
- Backend work never runs on the main thread (copy how `branches.rs` does it). Errors are shown as the webview shows them (notification or dialog text), never swallowed.
- Tests: temp directories only; `git` runs with a config that ignores the user's global config and hooks exactly as the existing tests do; no network (a "remote" is a second local bare repository), no real `gh`, no interactive shell, no system Trash, nothing under the user's home. Names of tests that cover a spec case are the spec's sentence in snake_case. Each end-to-end test proves a user-visible result (the repository state via `git` in the temp dir, the notification text, the rows a view shows), not that a function was called.
- A secret (password, token, passphrase) is never painted, logged, copied or part of a `Debug` output.
- Comments say what the code does and why, present tense; never mention "port", "webview", "TypeScript", "React", "parity", tasks or history.

## Report back (this is all the main session will see; exact, no narrative, under 900 words)

1. Files written (paths, line counts). Per view: constructor, events, accessors.
2. Actions registered: id → what it does → the end-to-end test that runs it → the IPC commands it stands for. Actions of your assignment NOT registered, each with why.
3. Spec files covered: cases covered / total per spec file, the rest with why.
4. Behaviours of the webview you did not build, with why. The assumptions an integrator must check first. The tests most likely to fail on first run, with why.
