# Integrator brief: actions whose behaviour changed upstream and that the native app does not follow (the few-line ones)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`. Your own scratch subfolder: `$S/dfol4/`.

Format only files you edited, taken from `git diff --name-only`. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Use `git diff -U3` when you read a diff.

## What this is

A reader compared the 454 action rows at parity whose registering file changed in the merge of upstream v0.1.46 with the native actions (`docs/gpui-port/notes/RETRIAGE_ACTIONS.md`: at the top the native differences grouped by native file, then one table per registering file with what changed and the webview's `file:line`). 68 rows were lowered; each lowered row's proof in `docs/gpui-port/parity.json` says what is missing. Its findings are hypotheses: read the webview's action at HEAD and at `3662ca0ea`, the upstream commit, and the native action before changing anything. This slice takes the differences the report sizes as "a few lines" or "one line"; the features (pinned Run and Debug tabs, Services folders, Hierarchy tabs, several query consoles, the combined diff, JavaScript browser debugging, the modal commit dialog, the agents' selection toolbar, the Docker Compose gutter menu) are not yours.

## The items, in this order

1. Shared shortcuts: when several actions share a key, the webview prefers the one whose `keyWhen` matches the place the keyboard is in; the native dispatcher takes the first enabled (`app-gpui/src/platform/dispatcher.rs`, `first_enabled`). Read the webview's rule (`src/platform/keymap.ts` and its spec at HEAD) and follow it exactly; test with the report's example (the Eclipse preset's rename key in the Project view against Edit Commit Message) and with every existing dispatcher test unchanged.
2. Settings › Keymap lists Cut, Copy, Paste, Redo and Select All: the data generator (`docs/gpui-port/gen-data.mjs`: the one file under `docs/` you may edit, say what you changed) drops the actions' new `keymap` flag; carry it into `app-gpui/assets/gen/actions.json`, regenerate (its check mode must pass afterwards), and make the Keymap page list by it as the webview does.
3. Version control: Pull, Merge, Rebase and Update act on the repository of the file the user is on (`app-gpui/src/workbench/vcs_operations.rs` near 135); the editor's Git submenu has the entries the webview's has (`app-gpui/src/workbench/context_menus.rs` near 428: Merge, Rebase, Reset HEAD, Stash, Unstash and the others the report lists); the Git log's changed-files pane sets `vcsLogChangesFocus`, so its Show Diff key works, an explicit Show Diff opens in the reusable preview tab, and F4 jumps to source; Rollback falls back to the checked changes; the changes view has the `.git/info/exclude` entry and the exclude file opens after it is written; Git notices show as a balloon when balloons are off, as the webview's do. History rewriting on pushed commits belongs to another brief (`int_follow_upstream2_brief.md`, item 3): if that has landed, check the six log rows the report names against it; if not, leave them. No test touches a repository outside its temp folder, the user's git configuration or the network.
4. Editor: Apply Quick Fix matches a fix to its diagnostic; Quick Definition drops an answer that arrives after the caret moved or the popup closed (stale answers: a test with the stand-in language server answering late).
5. Database: Submit, Revert and Go to Column reach a console's result grid when it is editable (`app-gpui/src/workbench/database.rs` near 294); and settle by reading whether a request is dropped when the project changed while the data sources were loading (four rows were lowered on that doubt: `db.execute`, `db.explainPlan`, `db.newDataSource`, `db.queryHistory`): if the Database slot is rebuilt per project say so with the line and a test, else fix it.
6. Trust: the question about a project that opens in a new window is asked in the current window before the new one is created (the stale test `workbench::app_actions::tests::a_project_opening_in_a_new_window_is_asked_about_in_that_window` is rewritten to the webview's order first); a declined question creates no window and trusts nothing. Read what "remote trust stored on the desktop side" is in the webview before touching it; if it concerns remote projects the native app cannot open, list it.
7. Small ones: Hide Active Tool Window clears an orphaned maximized side; the Welcome window's Open File button; the HTTP endpoint request takes the server address; debug launches load `.env` files as run launches do (read which files, in which order, and that a value from them is never logged); plugin command buttons show their running state; the texts "Couldn't copy the task" (`app-gpui/src/workbench/agents.rs` near 1051 and 1145) and "Open in Terminal" in the agent task menu follow the webview's; the two hard-coded conditions with the old text (`workbench/refactor_actions.rs:55`, `workbench/editor_extras.rs:40`). A Unity project's Unity Editor run configuration is listed, not built.

For each item: a test that fails first, then the change. No assertion is loosened: an expectation changes only to what the webview at HEAD does, named with its `file:line`.

## If the slice is larger than one pass

Take the items in order. When about two thirds of your budget is used, finish the item in hand, run the gates on what has landed, and report: which items are done with their tests, which are not started. Never leave an item half applied; an item that is not finished is undone by editing. The manager launches the rest as another integration.

## Rows

For each lowered row the items touch, say which missing clause of its proof is now proven, by which test. Do not write to `parity.json`.

## Changed since

HEAD is the commit after `c107abed7`. **Items 1, 2 and 3 of this brief have landed and are verified (code commit `1e6a4ba69`): do not redo them. Start at item 4 and take 4, 5, 6 and 7 in that order.** What item 3 left open is not yours either (the Commit panel has no context menu; the Project view's Git menu): it is listed in `docs/gpui-port/notes/OWED.md`.

Since items 1 to 3, the Git log changed (code commit `c107abed7`): it opens on the current branch with a "Branch: HEAD" chip, the Git window gives the keyboard to the Branches pane's filter field, the log asks for summaries only, and `workbench/log_window.rs`, `workbench/log_actions.rs` and `vcs/log_view.rs` changed. Earlier the same day: the save cleanup with idle saving (an editor is saved through `Editor::save_document`; the old `save` is `write_as_is`), New from Template, the remote-debug refusal (`run/remote_debug.rs`), the UI following the Language setting, Search Everywhere's scope and history. Every line number in this brief was written before all of that: re-anchor every place by its quoted name or text and read it before editing.

Added to item 7 from a run of the release build on a large real project: the Welcome window shows version 0.1.40 where the Tauri app built from the same commit shows 0.1.46 (find where the native app takes its version and make it the workspace's, with a test that compares what the window shows with the version the build declares), and it offers one action where the webview's offers six (New File, New Project, Open, Open File, Clone Repository, Remote Development): add those the native app already has an action for, each driven by a click in a test, and list the rest.

The other agents' undeclared drafts now in the tree, none of them yours, are listed in `$S/undeclared_now.txt`: leave every file named there alone. Never launch the real binary unless an item's check needs the window; then only through `$S/insp8/run.sh` with a neutral `INSPECT_BASE` and a temp project, and never the Agents tool window, Services, Settings' Agents pages or a debug session. Eight tests that wait for a run or debug process to stop timed out once under a machine load of 80 to 130 (`workbench::debug::tests::proofs40_debug_guards::*`, `workbench::tests::proofs18_e2e_run::cmd_f2_stops_a_running_process`, `workbench::tests::proofs40_run_start::*`) and passed in clean runs: if they fail in your whole-suite runs, quote them and go on. Run the twenty-fold repeats after the second whole-suite run has finished, never beside a suite run, and never start a test run with `&`.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings; `node docs/gpui-port/gen-data.mjs --check` (read its usage first) reports nothing. Never pipe a test run into `head`. Start no test run with `&`, in a script or from the shell, and do not leave a polling loop behind. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Besides the format of `$S/int_rules.md`: one section per item (the webview with `file:line` and the upstream commit, the native change, the tests and whether each failed first); what is left; the table of lowered rows whose missing clauses are now proven.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file. No edits under `crates/`, `src/`, `src-tauri/`, `e2e/`, nor under `docs/` except `docs/gpui-port/gen-data.mjs`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`. The real binary: only through `$S/insp8/run.sh` with a temp home and project under a neutral path `/private/tmp/openidea-inspect-<random>/`; never the Agents tool window, the Agents pages of Settings, the Services window's Services tab, a debug session, a database connection, or any Git command against a remote.
