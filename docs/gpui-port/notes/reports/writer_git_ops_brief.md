# Writer brief: Git operations offered and not carried out (Create Repository, Clone, tags, stash, hunks), written without a compiler

You write a draft for a native Rust (GPUI) rewrite of an IDE's UI. Another agent is compiling and editing files under `app-gpui/src` right now: you NEVER run cargo, rustc, rustfmt, clippy or the binary, you NEVER edit an existing file, and you run no git command that changes anything. You write NEW files only, plus notes that tell the integrator which hunks to apply to existing files.

Worktree (work only here): `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01` (branch `gpui`). Native app: `app-gpui/` (crate `openidea_gpui`). The app it must match 1:1: the webview UI under `src/` (React/TypeScript), tokens in `src/styles/tokens.css`. Shared scratch: `S=/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad`; your own subfolder `$S/dgit/` (create it). Temp files go there, never `/tmp`. You read nothing under the user's home outside this worktree and the scratch folder (not the cargo registry either: if you need a dependency's API, read how existing native code calls it).

Stable anchors: read every existing file under `app-gpui/` with `git show HEAD:<path>` (or `git grep … HEAD -- <path>`), never from the working tree. Files under `src/`, `src-tauri/`, `e2e/` and `crates/` are not being edited: read them directly. Use `git diff -U3` when you read a diff.

Read first, whole: `docs/gpui-port/notes/wiring_brief.md`, `docs/gpui-port/notes/tests_brief.md`, `docs/gpui-port/notes/view_rules.md`.

## What a row is, and how this slice is measured

`docs/gpui-port/INVENTORY.md` lists every item of the webview the rewrite must reproduce; `docs/gpui-port/parity.json` records the state of each (`done`, `partial` with a "Missing:" clause, or no entry at all: nothing native). A row reaches parity only on named native tests that start from a window, a registered action, a key or an event and end in what the user sees or what is on disk, covering what the webview's item does. A backend-command row (`ipc:<name>`) is judged by what the webview does at each place it calls the command; the native app calls the backend function directly (`ide_host::ops::<domain>::<function>`), or reaches the same behaviour another way, which is then said. This slice is measured by the rows it lets reach parity, so every piece you build is tied to rows, by key.

## The slice: Git operations the menus name and nothing carries out

Rows with nothing native today: the actions `vcs.createRepository` (`src/features/vcs/actions.tsx:266`), `vcs.clone` (`:275`), `vcs.stashSelected` (`:704`); the backend commands `vcs_init`, `vcs_clone`, `vcs_delete_tag`, `vcs_restore_tag`, `vcs_stash_drop`, `vcs_stage_hunks`, `vcs_file_mode`, `vcs_ignore_blame_revision`, `vcs_revision_files`, `vcs_revision_children`, `vcs_compare_paths`, `vcs_move_worktree`. Partial rows of the same pieces: `unit:src/features/vcs/__tests__/clone.test.ts` (its model is tested; the dialog is not built), `ipc:scaffold_create` (New Project with Git: a test file for it exists and is not declared, `app-gpui/src/workbench/app_actions/tests/proofs6_app_actions.rs`: read it and say what declaring it needs), `ipc:vcs_stash_push` and `ipc:vcs_stashes` (read their clauses). A reader's notes on each command's call sites are in `docs/gpui-port/notes/designs/ipc_rows_read_3.md` (the Git section and its "Not built" list): a starting point, to be checked.

What the user gets in the webview, to be read exactly for each: where it is offered (main menu, the Branches pane's tag rows, the Git log's commit menu, the Stash list, the Changes view's hunk gutter, Annotate's menu, a file's properties), what it asks (dialog fields, defaults, validation, confirmation texts), what it runs, and what the user sees after (notification, the list refreshed, an Undo offered: Delete Tag's notification offers Restore, which is what `vcs_restore_tag` is for).

Order by rows and by cost: the small ones first (Create Git Repository, Delete Tag with its Restore, drop a stash, Stash Selected), then Clone (from a local folder path in tests, never a network address; the dialog's folder chooser is the system's: use the seam the native tests already use for a chooser's answer, grep `prompt_for_paths`), then the rest as far as each is complete. A piece that needs a view that does not exist natively (say the list of a revision's files, or the comparison of two paths) is marked `needs` with what the view is, unless it is small.

Git in tests: the backend's own Git functions against a repository the test creates in its temp folder, as the existing Git window tests do (read `workbench::vcs_operations::tests` for the harness and how it keeps the user's configuration out).

## Your first deliverable: the list

For every row above: what the webview does (the handler, the dialog or view, each call site of the command, the spec's cases), read at HEAD with `file:line`; what the native app has today (grep the action id, the backend function and the feature's module under `app-gpui/src` at HEAD; say `file:line`, or what you grepped when there is nothing); and one verdict: `already there` (only a test is missing: write it), `build` (in this slice), `needs` (what it waits for, named), `cannot be tested` (the backend function runs a program of the machine or needs the network, and no stand-in exists: say which line under `crates/` does it). Add the rows you find on the way that the same pieces would raise (the inventory's menu, context-key, overlay and spec rows of the same feature; partial rows whose clause names this feature).

Then build everything marked `build`, most rows first, each piece complete or absent, with its tests.

## Hard rules

- Nothing reads or writes the project's files on the window's thread; long work goes through the backend's functions as the neighbouring native code does. A request that is superseded or whose window changed project is dropped (the Database slot's `reset_database` and its tests show the pattern).
- Tests use a temp home and a temp project and the real backend; no network (a local folder or a loopback stand-in only), nothing under the user's home, no program of the machine beyond what the area's existing tests already run (say which); the user's global git configuration is never read (`GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null`, as the existing Git tests arrange it: read how). Deletes never reach the system Trash. Every wait covers exactly what the next assertion needs: the suite runs under a load above 100.
- Restricted Mode is a security boundary: nothing here runs or writes in an untrusted project beyond what the webview allows there (read its `when` clauses).
- The repository is public: nothing you write names any other project, path, branch or author of the machine's owner.

## What you deliver

1. New files only, untracked: logic and views in new files beside the modules they extend, tests in new files with the prefix `proofs49_` (beside the modules they test; window tests under `app-gpui/src/workbench/tests/`). Every new file's name ends in `_pending.rs`: nothing declares them, so they are not compiled. Write code that will compile, reading the real signature of everything you call. Keep each file under about 800 lines. No `#[ignore]`.
2. Notes at `$S/drafts/git_ops_integration.md`: (a) the list (row, what the webview does, what is native, verdict); (b) every hunk for existing files, in order, grouped by piece so each lands alone, each with the file, a quoted anchor copied exactly from `git show HEAD:<file>`, and the new lines; (c) per row the state it reaches after your pieces, with the tests by name, and what stays missing; per spec file every case with its native test; (d) your hypotheses, each with the cheap check that settles it; (e) the existing tests whose expectations change, with the webview's `file:line`; (f) the tests most likely to fail first and why; (g) what a look at the real window must check.
3. Code comments say why the code is as it is: no history, no "port", "webview", "TypeScript", "upstream", no task names. No stubs, no TODO for core logic, no new dependencies, no `unsafe`. User-visible strings exactly as the webview has them (`src/platform/locales/text/en.json` or the component's own text: read how other native views pass text through the localization lookup).

Do not touch: every existing file. Read freely.

Markdown you write: one paragraph per line, no hard wrapping.

Your final message (all I will see): the files you wrote with line counts; the list's totals per verdict; the rows table (row, state after the slice, tests); your hypotheses; anything in the existing native code, the inventory or these instructions that turned out wrong. Mark every statement you did not verify by reading the code as "Hypothesis:".
