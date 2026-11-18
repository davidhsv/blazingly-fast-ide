# Integrator brief: Services, the actions on a configuration folder node (eight rows that share one clause), with the Services focus key riding along

Read first: `$S/int_rules.md` (hard rules and report format) and `$S/view_rules.md`. Your own scratch subfolder: `$S/dsvc/`.

Format only files you edited, taken from `git diff --name-only` plus new files you created. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Use `git diff -U3` when you read a diff. Leave no polling loop behind.

## Rows this slice is to raise

Eight action rows are partial on one shared clause, "on a configuration folder node of the Services tree, it acts on that folder's configurations (`src/features/run/services.tsx:657`)": `action:run-all`, `action:stop-all`, `action:restart-all`, `action:open-browser`, `action:run.services.runAll`, `action:run.services.stopAll`, `action:run.services.restartAll`, `action:run.services.openBrowser`. Read each row's whole text in `docs/gpui-port/parity.json` first: if a row has any other clause, say so and close it too or name it.

Riding along (same tree, same harness): `context:servicesViewFocus` is partial for want of a test: on while the Services tree has the keyboard and off in the editor, and Add Service's key bound only in the tree (read its paragraph in `docs/gpui-port/notes/designs/rows_added_98.md`).

## What to do

1. Read the webview at HEAD: `src/features/run/services.tsx` around line 657 and everything that decides the target of each of the four actions (a selected configuration, a folder node, a type node, nothing selected), the two id families and why both exist (the tool window's toolbar and menu against the registered actions), what Open in Browser opens for a folder (every configuration's address or the first), what is disabled when, and the spec `src/features/run/services.test.ts` cases about folders. Write the rule down exactly.
2. Measure first: a window test per action, from a folder node holding two configurations beside a configuration outside the folder, in the module that already drives the Services tree with stand-in configurations (find how `workbench::tests::proofs3_run` and the Services tests start and stop stand-in processes: nothing real runs, no Docker, no network; the browser opener is the test's recorder, never the system's). Run them before changing anything and keep the log: which fail and how.
3. The smallest change to how the native handlers resolve their target so that a folder node acts on that folder's configurations, for both id families, in one place.
4. Each test seen failing with the change switched off (one mutation per script run that restores the file in a trap and compares hashes; `docs/gpui-port/notes/tools/mutation_check_example.sh`). Rebuild any test a mutation does not fail.
5. No assertion of an existing test is weakened; an existing test whose expectation changes is named with the webview's `file:line`.

## Care

- Tests use a temp home and project; no network; nothing under the user's home; stand-in processes only (the ones the area's tests already use); nothing is opened in a real browser. Every wait covers exactly what the next assertion needs: the suite runs under a load above 100, and tests that wait for a process to stop have timed out there.
- The real binary is not used in this slice (the Services tab is never opened in it).
- The repository this is pushed to is public: your report and files hold no name, path, branch, author or content of any project of the owner other than this repository.
- No new dependencies, no `unsafe`, no stubs, no `#[ignore]`.

## Changed since

HEAD is the commit `git log -1` shows (the worktree is clean except docs the manager is editing: do not touch `docs/`). Since the ranking landed: wiring batch 1, three tests-only harvests (the last one, H1, added Database result tests and a small fix in `app-gpui/src/workbench/database.rs`: a cell being edited now counts as a change for `dbTableEditorChanges`, and the grid's own cell field counts as its focus). The undeclared files to leave alone are listed in `$S/undeclared_now.txt` (refreshed at this HEAD). The suite at this HEAD: 8,422 tests.

## One extra, after the Services rows (small, same gates)

`context:dbTableEditorChanges` is partial on one clause: on while a cell is being edited in the Database tool window's table editor although nothing is staged yet (`src/features/database/tableEditorFocus.ts:52`, `:63`). The code for it landed with H1 (`sync_database_context` in `app-gpui/src/workbench/database.rs`) and is proven only for the query console's result grid (`workbench::tests::proofs43_database_results::revert_confirms_the_result_cell_being_edited_and_drops_it`, helper `edit_a_result_cell`). Write the same proof for a table opened with Edit Data (the harness of `workbench::tests::edit_data_shows_the_rows_submit_writes_an_edit_and_revert_discards_one`): a cell's field open with a typed value, nothing staged, the key on, Revert available and dropping it, Go to Column confirming it first. One mutation of the table editor's branch of `sync_database_context`.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside either (write `uptime` before and after each run into the same file); after the second, every test you added or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&`. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Your final message must be the full report (no short closing line after it). Besides the format of `$S/int_rules.md`: the webview's rule; the first run's result; the change; a table with one line per row (key; raised or not; the tests by full name; the mutation; the clause that remains, quoted, if any); what in the rows' texts turned out false.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
