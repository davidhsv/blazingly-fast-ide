# Integrator brief: wiring batch 1, twelve behaviours whose native code exists and is called by nothing

Read first: `$S/int_rules.md` (hard rules and report format) and `$S/view_rules.md`. Your own scratch subfolder: `$S/dwb1/`.

Format only files you edited, taken from `git diff --name-only` plus new files you created. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Use `git diff -U3` when you read a diff. Leave no polling loop behind.

## The slice

A reader checked the audit's 103 "a few lines" entries at `00bd90824` and found twelve that are a call plus a test, with no new view. Its notes are `$S/drafts/wiring_few_lines_batches.md`, section "Batch 1": for each item the function where the call belongs (with a quoted line), what is at hand there, the webview's behaviour with `file:line`, and an existing window test to copy the setup from. The notes were written without a compiler: they are hypotheses. Where a note and the webview source disagree, the webview source wins.

The twelve, in the order to land them:

1. Back/Forward and Last Edit Location forget deleted files.
2. Selected text stays visible inside a find match.
3. The TODO preview is editable (a ready window test exists, undeclared: `workbench/tests/proofs16_todo_run_pending.rs`; declare what passes, say what does not).
4. and 5. Breakpoints move with edits (the edit event gains the text before the edit).
5. Reload Changed Classes in the editor's menu.
6. Stash rows show the message and the branch apart.
7. The editor colour scheme follows a light/dark theme switch.
8. The hover shows a quick-fix link.
9. Search Everywhere asks for endpoints again when they change.
10. Find in Files shows the replacement inline.
11. Paste in the Project view follows files copied in the file manager.

For each item:

1. Read the webview's wiring and its spec file at HEAD and write down exactly what the user sees or what lands on disk, and when.
2. Write the test first, through a project window on the real backend (or the harness the notes name), starting from an action, a key or an event and ending in what the user sees or what is on disk. Run it before changing anything and keep the log: it fails (not wired) or passes (the audit and the reader were wrong: say which code does it; the test stays).
3. Where it fails: the smallest change that reaches the existing model from the place the notes name (re-anchored by you at HEAD). If an item turns out to need a new view, a backend call the app does not make, or more than about eighty lines, stop that item, undo it by editing, and report it as "larger" with what it needs.
4. Each test seen failing with its guard switched off (one mutation per script run that restores the file in a trap and compares hashes; `docs/gpui-port/notes/tools/mutation_check_example.sh`).

No assertion of an existing test is weakened: an existing test whose expectation changes is named with the webview's `file:line`; if more than six existing tests change, stop after the sixth and report before going on.

## Care

- Item 12 reads the system pasteboard's file list: tests put file paths of the test's temp folder on the test platform's clipboard only; nothing of the machine's real clipboard is read by a test or logged; a pasted path outside any project is copied as the webview copies it and never deleted or moved unless the webview's source says cut applies (read it).
- Item 6 starts nothing in a test: the action's presence and enabled state with a stand-in debug session; no JVM, no real debug session.
- Items 4 and 5: breakpoints are stored per project; tests assert the stored lines after edits and after a reload of the window's breakpoints.
- Tests use a temp home and project; no network; nothing under the user's home; no program of the machine is run by a test. Deletes never reach the system Trash.
- The repository this is pushed to is public: your report and files hold no name, path, branch, author or content of any project of the owner other than this repository.
- No new dependencies, no `unsafe`, no stubs, no `#[ignore]`.
- The real binary once, through `$S/insp8/run.sh` with a neutral `INSPECT_BASE` and a temp project, light and dark: a find match with a selection inside it, the stash list, Find in Files with a replacement typed. Never the Agents tool window, Services, Settings' Agents pages or a debug session.

## Rows

The notes name the rows each item could raise (`unit:src/editor/core/model/editHistory.test.ts`, `unit:src/editor/core/cm/selectionMarks.test.ts`, `unit:src/features/todo/TodoView.test.tsx`, `unit:src/features/debugger/__tests__/breakpoints.test.ts`, `unit:src/features/debugger/__tests__/hotswap.test.ts`, `unit:src/features/vcs/__tests__/decorations.test.ts`, `module:src/platform/colorScheme.ts`). For each: which clause is now proven, by which test, and what of the row's "Missing:" text remains. Do not write to `parity.json`.

## Changed since

BEFORE LAUNCH (for the manager; delete this paragraph then): say HEAD, which files of the notes' call sites changed since `00bd90824`, and refresh `$S/undeclared_now.txt`.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside either (write `uptime` before and after each run into the same file); after the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&`. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

When about two thirds of your budget is used, finish the item in hand, run the gates on what has landed, and report which items are done and which are not started. Never leave an item half applied.

## Report

Your final message must be the full report (no short closing line after it). Besides the format of `$S/int_rules.md`: one section per item (the webview's rule, the first run's result, the change, the test and its mutation, or why it was stopped), existing tests changed with the webview's line, the shots' paths and what each shows, the rows table, and what in the notes turned out false.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file this brief does not hand you. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
