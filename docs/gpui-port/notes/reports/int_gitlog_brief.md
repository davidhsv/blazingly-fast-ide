# Integrator brief: the Git log on a real repository (empty graph and subjects, the default branch filter, paging, re-reads)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md` as far as it applies to an integrator, `$S/view_rules.md`. Your own scratch subfolder: `$S/dglog/`.

Format only files you edited, taken from `git diff --name-only` plus the draft file this brief hands you by name. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Use `git diff -U3` when you read a diff. Leave no polling loop behind.

## What this is

A release build run on a large real repository showed the native Git log with an empty graph column and an empty subject column in every row, and no branch chip, where the Tauri app showed HEAD's commits with graph and subjects. A reader found the causes by reading (nothing was compiled): the notes are `$S/drafts/gitlog_real_repo_integration.md` (copy: `docs/gpui-port/notes/designs/gitlog_real_repo.md`): four findings with `file:line` on both sides, seven hunks H1 to H7 with quoted anchors in the order to apply them, the existing tests the hunks change, and five tests written in the undeclared file `app-gpui/src/workbench/tests/proofs42_gitlog_real_repo_pending.rs` (declare it where the notes say). Everything in the notes is a hypothesis until it compiles and runs; where the notes and the webview's source disagree, the webview's source at HEAD wins.

## Order

1. Declare the five tests first with only H1's selectors applied and run them: keep the log of that run. Each must fail for the reason the notes' table gives; a test that passes before the change proves nothing and is strengthened until it fails, or dropped with the reason.
2. H2 and H3 (the graph packed and laid out over everything loaded; the cell at its row's width; labels that shrink; the split and the date column as the webview's). Then H4 (read-ahead, PageDown), H5 (summaries only; Copy Commit Message reads the whole message when truncated: a test of its own with a multi-line message, asserting the clipboard text through the test platform's clipboard), H6 (the filter settled and remembered per repository; re-read only on a ref move and only while shown), H7 (the keyboard on opening) only after you confirm from the webview's source and its own tests what the Tauri app focuses; if it cannot be settled by reading, leave H7 out and say so.
3. The existing tests the notes list as changing: each changed expectation is named with the webview's `file:line` that requires it. No assertion is weakened. If more than six existing tests change, stop and report before going on.
4. Measure, do not assume: the time of `present_graph` at 3,000 commits in a unit test (if it is above 10 ms, say so and lay out incrementally or off the window's thread, as the webview's store does: read it); that a second page keeps the first page's rows' lanes stable or, if they move, that the webview's do too.
5. The real window: once, through `$S/insp8/run.sh` with a neutral `INSPECT_BASE`, on the test's own 140-commit fixture repository built in a temp folder (never a repository of the owner), light and dark: the log on opening (chip HEAD, graph, subjects), with the Branch chip cleared (the newest rows' nodes and subjects), and a 900-wide window. Never the Agents tool window, Services, Settings' Agents pages or a debug session.

## Hard rules

- No test touches a repository outside its temp folder, the user's git configuration (`GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null`), the network or anything under the user's home.
- The branch filter's memory goes to the test's own storage, never a stored list of the user's.
- The repository this is pushed to is public: your report and files hold no name, path, branch, author or content of any project of the owner other than this repository.
- No new dependencies, no `unsafe`, no stubs, no `#[ignore]`.

## Rows

The notes' last table. For each, say which missing clause of its proof is now proven, by which test. Do not write to `parity.json`.

## Changed since

BEFORE LAUNCH (for the manager; delete this paragraph then): HEAD, what landed since the notes were written (90f5b83b2), the undeclared drafts then in the tree.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&`. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Your final message must be the full report (no short closing line after it). Besides the format of `$S/int_rules.md`: the first run's log per test (failed for the stated reason or not); per hunk what landed and what differed from the notes; the measurements of item 4; the existing tests changed with the webview's `file:line`; the shots' paths and what each shows; the rows table.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file this brief does not hand you. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
