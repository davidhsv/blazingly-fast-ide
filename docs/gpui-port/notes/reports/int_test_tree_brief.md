# Integrator brief: the test results tree of the Run tool window (a draft to declare, wire and prove; five empty rows and up to nine partial rows)

Read first: `$S/int_rules.md` (hard rules and report format) and `$S/view_rules.md`. Your own scratch subfolder: `$S/dtt/`.

Format only files you edited, taken from `git diff --name-only` plus new files you created. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Use `git diff -U3` when you read a diff. Leave no polling loop behind.

## What exists

A writer without a compiler left a draft, committed and not declared (nothing references it, it has never been compiled): `app-gpui/src/run/test_tree_pending.rs`, `app-gpui/src/run/test_tree_pending/{model,rows,view,view_tests}.rs` and `app-gpui/src/workbench/tests/proofs45_test_tree_pending.rs`. Its notes are `docs/gpui-port/notes/designs/test_tree_integration.md`: read them whole before anything else. They hold the hunks for existing files (section "Hunks for existing files"), what was settled by reading, six hypotheses with how to settle each, the tests most likely to fail first, and the rows. The notes' anchors were taken at an older HEAD: find each anchor again by its text, not by its line number.

The draft is a proposal, not a specification: the webview at HEAD is. Where the draft and the webview differ, the webview wins and you say so.

## Rows this slice is to raise

Empty today, to reach parity (each needs its own window test, named in the report against the row):

- `action:run.rerunFailed` (`src/features/run/index.ts:661`): from a finished run with failures, the action reruns only the failed tests (assert the command line the stand-in received) and is refused while nothing failed.
- `action:run.expandAllTests` (`Meta+=`, `src/features/run/index.ts:682`) and `action:run.collapseAllTests` (`Meta+-`, `:691`): through the registry and through their keys while the tree has the keyboard; assert the rows drawn.
- `context:run.canRerunFailed` (`src/features/run/index.ts`): on after a run with failures, off after a run without, off while the run is going if the webview says so (read it).
- `context:testTreeFocus` (`src/features/run/testTree.ts`): on while the tree has the keyboard, off in the console and in the editor; a key bound with it reaches the tree only then.
- `unit:src/features/run/TestTreeView.test.tsx` (11 cases): one native window test per spec case, or the exact reason a case has no native counterpart. The notes say ten of eleven are covered by `view_tests`; check each against the spec's own assertions, not against the draft's test names.

Partial today, whose clause this slice narrows or closes (read each row's whole text in `docs/gpui-port/parity.json` first and quote in the report the clause your test closes):

- `unit:src/features/run/testing/comparison.test.ts`, `unittest.test.ts`, `go.test.ts`, `phpunit.test.ts`, `ruby.test.ts`: their clause is that nothing in the app reaches the parser. A row rises only when a window test feeds that framework's output through a run (a stand-in process printing canned output, as the Run tests already do) and asserts the tree's rows and the console's text for it. If a framework's records would show raw in the console because nothing decodes them (the notes say so for go, phpunit and ruby), that is the remaining clause: say it with the webview's `file:line` for the decoding, and do not raise the row.
- `unit:src/features/run/testing/rerun.test.ts` and `states.test.ts`: the model-only clause, closed by the rerun command line and the gutter's test state through a window. The zig cases stay.
- `point.registerToolWindow:run`: the "no test tree" part of its clause.
- `e2e:e2e/run.spec.ts`: Rerun Failed Tests in the Run window.
- `action:navigate.nextOccurrence` and `action:navigate.prevOccurrence`: stepping through the failed tests while the Run tool window has the tree; their other clauses stay.

Rows that will stay partial whatever you do (do not spend time on them): `dotnet.test.ts` (TRX reports read at exit), `plugin.test.ts` (plugin output is not routed).

## What to do

1. Settle the six hypotheses of the notes first, each by the cheap check the notes name. A hypothesis that turns out false changes the draft before it is compiled.
2. Declare the draft under its final names (no `_pending` in a declared module or file name; rename with `git mv`), apply the hunks, and make it compile. Fix what the compiler and clippy find in the draft's own files; a hunk that no longer fits the code at HEAD is rewritten against the code as it is.
3. Run the draft's tests. Each one that fails: decide by the webview whether the test or the code is wrong, and say which in the report.
4. Compare with the webview, reading `src/features/run/TestTreeView.tsx`, `src/features/run/testTree.ts`, the tree's part of `src/features/run/index.ts` and `Run.module.css`: the header (counts, buttons, filters and where they are stored), the rows (icon, name, duration), the default expansion, selection and the console swapping to the selected test's output, the context menu, the keys, Jump to Source, Show Diff. Write the differences down; fix the small ones, report the rest with `file:line`.
5. Each new window test is seen failing with its guard switched off: one mutation per script run that restores the file in a trap and compares hashes (`docs/gpui-port/notes/tools/mutation_check_example.sh`). Rebuild any test a mutation does not fail. Model tests of the draft need no mutation.
6. No assertion of an existing test is weakened; an existing test whose expectation changes is named with the webview's `file:line`.

If the whole draft cannot land within your budget, land it in this order and stop cleanly at the end of a step: the model and the routing of a run's output to it (so the parsers are reached); the tree view in the Run tool window with selection and the console; the three actions and the two context keys; the context menu, Show Diff and the gutter states; the stepping. Never leave a declared module half wired: a step that is not finished is taken out again by editing.

## Care

- Tests use a temp home and project; no network; nothing under the user's home; no real toolchain is run (no cargo, go, python, php, ruby, dotnet of the machine): a run is a stand-in script the test writes into its temp folder, as the area's tests already do. Every wait covers exactly what the next assertion needs: the suite runs under a load above 100, and tests that wait for a process to stop have timed out there.
- The real binary is not used in this slice.
- The repository this is pushed to is public: your report and files hold no name, path, branch, author or content of any project of the owner other than this repository.
- No new dependencies, no `unsafe`, no stubs, no `#[ignore]`. Comments say why the code is as it is, never where it came from or which task added it.

## Changed since

HEAD is the commit `git log -1` shows (the worktree is clean except docs the manager is editing: do not touch `docs/`). Since the draft (`4a4811397`) landed, among others: the Editor Tabs settings, the remote folder chooser, wiring batch 1 (the TODO preview is an editable editor now; breakpoints move with edits), three tests-only harvests, the Services tree's folder nodes (`app-gpui/src/run_testing/services.rs`), and a fix of the tool window stripe buttons (a click's action is deferred out of the listener: a listener of a view must never run an action whose handler updates that same view; use `window.defer` as `app-gpui/src/workbench/mod.rs` does for the stripe button). The undeclared files to leave alone are listed in `$S/undeclared_now.txt` (refreshed at this HEAD; your own draft's files are in it: those you declare). The suite at this HEAD: 8,436 tests.

The owner uses release builds of this branch daily now. In your report, add a section "Mouse paths not tested": every clickable thing your slice adds or wires (buttons, rows, menu rows, links) for which no test clicks it.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside either (write `uptime` before and after each run into the same file); after the second, every test you added or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings; `node docs/gpui-port/gen-data.mjs --check` if you touched anything it reads. Never pipe a test run into `head`. Start no test run with `&`. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Your final message must be the full report (no short closing line after it). Besides the format of `$S/int_rules.md`: a table with one line per row above (key; raised, narrowed or not; the tests, by full name; the mutation; the clause that remains, quoted, and why); the six hypotheses and how each was settled; every difference from the webview you found, fixed or not, with `file:line`; the draft's tests that failed first and which side was wrong; what in the notes or in the rows' texts turned out false.

## Do not touch

Every other undeclared draft under `app-gpui/src` and every other undeclared `proofs*` or `*_pending.rs` test file (`$S/undeclared_now.txt`). No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
