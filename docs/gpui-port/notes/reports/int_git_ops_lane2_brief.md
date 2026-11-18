# Second-lane brief: compile, wire and prove the Git operations draft in a worktree of your own (no whole-suite runs)

You work in a second worktree, beside another agent that integrates a different draft (Editor settings pages) in the main worktree and runs the whole test suite there. Your worktree: `$L` = `$L` (branch `gpui-lane2`, at the commit `git -C $L log -1` shows, clean). `cd` into `$L` explicitly in every shell command. You never read, write or run anything in the main worktree (`$W`). The shared scratch folder is `$S` = `$S`; your own subfolder is `$S/dgit2/`.

Read first: `$S/int_rules.md` (hard rules and report format; where it names the main worktree or `$W`, read `$L`; its rule 12 is replaced by the gates below) and `$S/view_rules.md`.

## The machine is shared: what you may run

- cargo only inside `$L/app-gpui/` (its own `target/`, warm from the previous slice), always with `-j 5`, one cargo command at a time, never in the background with `&`.
- Allowed: `cargo check --all-targets -j 5`, `cargo clippy --all-targets -j 5 -- -D warnings`, `cargo build -j 5`, and `cargo test --lib -j 5 <filter>` for the tests you add or change and the test modules of the files you edit. NEVER the whole suite (`cargo test --lib` without a filter): the other lane runs it, and two at once make both fail on timeouts. A filter that selects more than about three hundred tests is too wide: split it.
- Format only files you edited or created, with `$S/fmt.sh <absolute paths under $L>` (absolute paths matter: the script changes directory).
- No git command that changes anything: no add, commit, stash, checkout, reset, worktree. Renames are plain `mv` (the manager takes your work as a diff of the working tree). Never restore a file from a backup or from git: undo your own edit by editing. Use `git diff -U3` to read a diff.

## What exists

A writer without a compiler left a draft, committed and not declared (nothing references it, it has never been compiled): `app-gpui/src/vcs_ops/git_ops_pending.rs`, `app-gpui/src/vcs/clone_form_pending.rs`, `app-gpui/src/workbench/vcs_operations/menu_ops_pending.rs`, and the tests `app-gpui/src/workbench/vcs_operations/tests/proofs49_{git_ops,clone}_pending.rs`, `app-gpui/src/workbench/annotate/tests/proofs49_ignore_commit_pending.rs`, `app-gpui/src/vcs_ops/commit_view/tests/proofs49_stash_selected_pending.rs`. Its notes are `docs/gpui-port/notes/designs/git_ops_integration.md`: read them whole before anything else (the list; the hunks B0 to B9, each piece landing alone in that order, B0 first; the state each row reaches; nine hypotheses each with its check; the existing tests whose expectations change; the tests most likely to fail first). The writer's own summary is `docs/gpui-port/notes/reports/git_ops_writer.md`. The notes' anchors were taken at an older HEAD: find each anchor by its text, not by its line number.

The draft is a proposal, not a specification: the webview at HEAD is (`src/features/vcs/`). Where they differ, the webview wins and you say so.

## Rows this slice is to raise

As the notes' section (c) lists them: `action:vcs.createRepository`, `context:projectLocal`, `ipc:vcs_init`, `ipc:scaffold_create` (stays partial; its clause loses the undeclared test file), `action:vcs.clone` (partial), `ipc:vcs_clone`, `unit:src/features/vcs/__tests__/clone.test.ts`, `ipc:vcs_delete_tag`, `ipc:vcs_restore_tag`, `ipc:vcs_stash_drop` (partial), `action:vcs.stashSelected`, `ipc:vcs_stash_push`, `unit:src/features/vcs/__tests__/stashSelectedFiles.test.ts`, `ipc:vcs_move_worktree`, `ipc:vcs_ignore_blame_revision`. For every row: the tests by full name, checked against what the webview does at each of its call sites (an `ipc:` row is the behaviour at the webview's call sites), not against the draft's test names; and the row's whole current text in `docs/gpui-port/parity.json` read first (several of these keys have no entry yet: say which).

## What to do

1. B0 first, alone: after it, run the existing Git test modules by filter, in pieces of at most about three hundred tests each (`workbench::vcs_operations::`, `workbench::log_actions::`, `workbench::annotate::`, `workbench::vcs_text_actions::`, `workbench::tests::proofs42_gitlog_real_repo`, and whatever else `git grep -l "init_repo\|git(" -- app-gpui/src` shows runs the backend's git), and report the counts. A test that fails because it relied on the machine's git configuration is a finding: fix the test's setup, never the isolation. The list B0 sets is process-wide: say in the report whether any non-test code path reads or replaces it.
2. Settle the notes' other hypotheses, each by the cheap check the notes name. A hypothesis that turns out false changes the draft before it is compiled.
3. Piece by piece, in the notes' order: declare the piece's files under their final names (no `_pending` in a declared module or file name; `mv`), apply its hunks (a hunk that no longer fits the code is rewritten against the code as it is), make it compile, run its tests, and for each test that fails decide by the webview whether the test or the code is wrong.
4. Each new window test is seen failing with its guard switched off: one mutation per script run that restores the file in a trap and compares hashes (the pattern is `docs/gpui-port/notes/tools/mutation_check_example.sh`, with your own paths). Rebuild any test a mutation does not fail. Tests of a model alone need no mutation and raise no row.
5. No assertion of an existing test is weakened; an existing test whose expectation changes is named with the webview's `file:line`.
6. A listener of a view never runs a registry action directly when the action's handler can update that same view: defer it with `window.defer`, as the tool window stripe button in `app-gpui/src/workbench/mod.rs` does. Every button, row, menu row and link you add or wire is clicked by a test, or is listed in your report under "Mouse paths not tested".

If the whole draft cannot land within your budget, stop cleanly at the end of a piece: a declared module is never left half wired; a piece that is not finished is taken out again by editing and its files keep their `_pending` names.

## Care

- Tests use a temp home and project. No network: a clone's URL is a folder of the test's temp directory, a remote is a bare repository there; no test passes a URL with a scheme or a host. Nothing under the user's home: the clone's remembered parent folder and recent URLs are stored under the test's temp home (check where `RecentProjects` and the form's two keys are written in a test). Every git the test itself starts runs with `GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null`, as the area's helpers do. No SSH, no credential helper, no askpass program is started.
- The system's folder chooser is never opened: a chooser's answer comes through the seam the existing tests use, and that seam panics when no chooser is pending, so a test waits until the chooser has been asked for before it answers (the previous slice found this for the save dialog: its tests wait for `did_prompt_for_new_path()`; find the counterpart for the folder chooser).
- Restricted Mode is a security boundary. The folder a clone or Create Git Repository produces must not become trusted, and nothing in it may run, without the user's answer: the clone test asserts what the app asks before the clone is opened as a project and what state the project opens in. If the draft opens a clone as trusted without asking, that is a defect to fix by the webview's flow (`src/features/vcs/clone.tsx`, `openProject`), not a test to adapt.
- A cancelled or failed clone leaves no half-written folder that a later test or the user would mistake for a repository: report what is left on disk in both cases, with the webview's behaviour beside it.
- Drop Stash and tag deletion destroy data: the confirmation is asserted before the operation, and declining it changes nothing on disk.
- Never quit, restart, focus or signal the running "Blazingly Fast IDE" application or any process you did not start. Another process may be building under `/private/tmp/openidea-gpui-use-*`: not yours.
- The repository is public: no name, path, branch, author or content of any other project of the machine's owner in anything you write.
- No new dependencies, no `unsafe`, no stubs, no `#[ignore]`, nothing under `crates/`. Comments say why the code is as it is, never where it came from or which task added it.

## Changed since the notes

The worktree is at the manager's latest commit: since the notes were written, the test results tree of the Run tool window and the untitled documents (Save As, the byte order mark, files changed on disk; `app-gpui/src/workbench/untitled_tabs.rs`, `disk_sync_window.rs`, `encoding_bom.rs`) landed; `workbench/mod.rs` changed in both. A window that becomes active again now checks its open files against the disk. Other undeclared drafts are not yours and stay untouched: every other `*_pending.rs` file under `app-gpui/src` (the Editor settings pages, the agents list).

## Gates

`cargo clippy --all-targets -j 5 -- -D warnings` clean; `cargo build -j 5` with 0 warnings; every test you added or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure whole); the test modules of every file you edited, once by filter, with the counts; the Git modules of step 1 once more on the final tree. `node docs/gpui-port/gen-data.mjs --check` needs `node_modules`, which this worktree lacks: say it was not run. The whole suite is NOT yours to run: say so in the report; the manager runs it after taking your diff.

## Report

Your final message must be the full report (no short closing line after it): the gates with their numbers; a table with one line per row (key; raised, narrowed or not; the tests by full name; the mutation; the clause that remains, quoted, and why); the hypotheses and how each was settled; every difference from the webview you found, fixed or not, with `file:line`; the draft's tests that failed first and which side was wrong; the existing tests whose expectations changed; "Mouse paths not tested"; what a cancelled and a failed clone leave on disk; the exact list of files you edited, created, renamed (old → new) and deleted, relative to `$L` (the manager builds the diff from it); what in the notes turned out false.
