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

## Rows

For each lowered row the items touch, say which missing clause of its proof is now proven, by which test. Do not write to `parity.json`.

## Changed since

BEFORE LAUNCH (for the manager; delete this paragraph then): name the undeclared drafts of other agents then in the tree, which earlier follow-upstream integrations landed, and whether the line numbers above moved.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings; `node docs/gpui-port/gen-data.mjs --check` (read its usage first) reports nothing. Never pipe a test run into `head`. Start no test run with `&`, in a script or from the shell, and do not leave a polling loop behind. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Besides the format of `$S/int_rules.md`: one section per item (the webview with `file:line` and the upstream commit, the native change, the tests and whether each failed first); what is left; the table of lowered rows whose missing clauses are now proven.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file. No edits under `crates/`, `src/`, `src-tauri/`, `e2e/`, nor under `docs/` except `docs/gpui-port/gen-data.mjs`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`. The real binary: only through `$S/insp8/run.sh` with a temp home and project under a neutral path `/private/tmp/openidea-inspect-<random>/`; never the Agents tool window, the Agents pages of Settings, the Services window's Services tab, a debug session, a database connection, or any Git command against a remote.
