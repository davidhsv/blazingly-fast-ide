# Second-lane brief: compile, wire and prove the untitled documents draft in a worktree of your own (no whole-suite runs)

You work in a second worktree, beside another agent that integrates a different draft in the main worktree and runs the whole test suite there. Your worktree: `$L` = `(the second worktree)` (branch `gpui-lane2`, cut from `gpui` at the commit `git -C $L log -1` shows). `cd` into `$L` explicitly in every shell command. You never read, write or run anything in the main worktree (`…/worktrees/agent-a307dd904b5ae1f01`). The shared scratch folder is `$S` = `$S`; your own subfolder is `$S/ddoc2/`.

Read first: `$S/int_rules.md` (hard rules and report format; where it names the main worktree or `$W`, read `$L`; its rule 12 is replaced by the gates below) and `$S/view_rules.md`.

## The machine is shared: what you may run

- cargo only inside `$L/app-gpui/` (its own `target/`, built cold the first time: expect ten minutes or more), always with `-j 5`, one cargo command at a time, never in the background with `&`.
- Allowed: `cargo check --all-targets -j 5`, `cargo clippy --all-targets -j 5 -- -D warnings`, `cargo build -j 5`, and `cargo test --lib -j 5 <filter>` for the tests you add or change and the test modules of the files you edit. NEVER the whole suite (`cargo test --lib` without a filter): the other lane runs it, and two at once make both fail on timeouts. A filter that selects more than about three hundred tests is too wide: narrow it.
- Format only files you edited or created, with `$S/fmt.sh <absolute paths under $L>` (absolute paths matter: the script changes directory).
- No git command that changes anything: no add, commit, stash, checkout, reset, worktree. Renames are plain `mv` (the manager takes your work as a diff of the working tree). Never restore a file from a backup or from git: undo your own edit by editing. Use `git diff -U3` to read a diff.

## What exists

A writer without a compiler left a draft, committed and not declared (nothing references it, it has never been compiled): `app-gpui/src/editor_model/untitled_pending.rs`, `app-gpui/src/editor/view/untitled_pending.rs`, `app-gpui/src/workbench/untitled_tabs_pending.rs`, `app-gpui/src/workbench/encoding_bom_pending.rs`, `app-gpui/src/workbench/disk_sync_window_pending.rs`, and the tests `app-gpui/src/workbench/tests/proofs48_{untitled,save_as,light_edit_untitled,bom,disk_sync}_pending.rs`. Its notes are `docs/gpui-port/notes/designs/documents_integration.md`: read them whole before anything else (the list; the hunks in three pieces, to land in order: untitled buffers with Save As and the Welcome path, then the byte order mark, then files changed on disk; the state each row reaches; twelve hypotheses each with its check; the one existing test whose expectation changes; the tests most likely to fail first). The writer's own summary is `docs/gpui-port/notes/reports/documents_writer.md`.

The draft is a proposal, not a specification: the webview at HEAD is (`src/editor/core/`, `src/features/app/windows.ts`, `src/platform/project.ts`). Where they differ, the webview wins and you say so.

## Rows this slice is to raise

As the notes' section (c) lists them: `action:file.newUntitled`; `ipc:doc_open_untitled`, `ipc:doc_save_as`, `ipc:light_edit_open_untitled`, `ipc:doc_set_bom`, `ipc:doc_sync_from_disk`; `point.registerStatusItem:editor.encoding`; `module:src/editor/core/diskSync.ts`; `unit:src/editor/core/diskSync.test.ts`; `unit:src/editor/core/untitled.test.ts` (6 of 7); the two VS Code keymap rows and their two module rows (only if their sole open clause is the New Untitled File binding: read each row's whole text in `docs/gpui-port/parity.json` and say what else it has); `ipc:doc_set_encoding` narrowed. For every row: the tests by full name, checked against what the webview does at each call site, not against the draft's test names.

## What to do

1. Settle the notes' hypotheses first, each by the cheap check the notes name. A hypothesis that turns out false changes the draft before it is compiled.
2. Piece by piece, in the notes' order: declare the piece's files under their final names (no `_pending` in a declared module or file name; `mv`), apply its hunks (a hunk that no longer fits the code is rewritten against the code as it is), make it compile, run its tests, and for each test that fails decide by the webview whether the test or the code is wrong.
3. One hunk is under `crates/` (`crates/ide-host/src/project.rs`, the function `open_light_empty_in`, so that no test roots a LightEdit window at the real home folder). You may apply exactly that hunk, additive only: no existing function of the crate changes its signature or behaviour. Nothing else under `crates/` is touched.
4. Each new window test is seen failing with its guard switched off: one mutation per script run that restores the file in a trap and compares hashes (the pattern is `docs/gpui-port/notes/tools/mutation_check_example.sh`, with your own paths). Rebuild any test a mutation does not fail. Tests of a model alone need no mutation and raise no row.
5. No assertion of an existing test is weakened; an existing test whose expectation changes is named with the webview's `file:line`. The notes say every window test that activates a window and advances the clock will now run a disk check: find which existing tests that touches (grep `activate_window`) and run those modules by filter.
6. A listener of a view never runs a registry action directly when the action's handler can update that same view: defer it with `window.defer`, as the tool window stripe button in `app-gpui/src/workbench/mod.rs` does. Every button, row, menu row and link you add or wire is clicked by a test, or is listed in your report under "Mouse paths not tested".

If the whole draft cannot land within your budget, stop cleanly at the end of a piece: a declared module is never left half wired; a piece that is not finished is taken out again by editing and its files keep their `_pending` names.

## Care

- Tests use a temp home and project; no network; nothing under the user's home (the reason for the `crates/` hunk: check that every new test's LightEdit or untitled document lives under the test's temp home); the system's file chooser is never opened: a chooser's answer comes through the seam the existing tests use. Deletes never reach the system Trash. Every wait covers exactly what the next assertion needs: assume a machine under heavy load.
- Never quit, restart, focus or signal the running "Blazingly Fast IDE" application or any process you did not start. Another process may be building under `/private/tmp/openidea-gpui-use-*`: not yours.
- The repository is public: no name, path, branch, author or content of any other project of the machine's owner in anything you write.
- No new dependencies, no `unsafe`, no stubs, no `#[ignore]`. Comments say why the code is as it is, never where it came from or which task added it.

## Gates

`cargo clippy --all-targets -j 5 -- -D warnings` clean; `cargo build -j 5` with 0 warnings; every test you added or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure whole); the test modules of every file you edited, once by filter, with the counts; `node docs/gpui-port/gen-data.mjs --check` if you touched anything it reads (it needs `node_modules`, which this worktree lacks: if so, say it was not run). The whole suite is NOT yours to run: say so in the report; the manager runs it after taking your diff.

## Report

Your final message must be the full report (no short closing line after it): the gates with their numbers; a table with one line per row (key; raised, narrowed or not; the tests by full name; the mutation; the clause that remains, quoted, and why); the hypotheses and how each was settled; every difference from the webview you found, fixed or not, with `file:line`; the draft's tests that failed first and which side was wrong; the existing tests whose expectations changed; "Mouse paths not tested"; the exact list of files you edited, created, renamed (old → new) and deleted, relative to `$L` (the manager builds the diff from it); what in the notes turned out false.
