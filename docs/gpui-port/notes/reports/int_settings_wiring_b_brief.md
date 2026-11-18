# Integrator brief: a trust step answered without asking (first), then the editor tabs settings that are written and read by nothing

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md` as far as it applies to an integrator (its test-writing rules do, its "do not run cargo" does not), `$S/view_rules.md`. Your own scratch subfolder: `$S/dswb/`.

Format only files you edited, taken from `git diff --name-only` plus new files you created. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Use `git diff -U3` when you read a diff. Leave no polling loop behind.

## Part 0 (first, and reported first): attaching a folder to the workspace answers its trust step without asking

Found by an earlier integrator while reading, not measured: in `app-gpui/src/workbench/app_shell.rs` the open flow's step `OpenStep::ConfirmTrust` is answered with `flow.trust_confirmed(Ok(()), …)` directly, under a comment saying that a folder is attached only when it is trusted already, where the webview asks the user (`src/features/app/windows.ts`, `attachProject`: read it at HEAD and its spec cases in `src/features/app/__tests__/windows.test.ts`, and the trust feature `src/features/project-trust/`). Restricted Mode is a security boundary: a folder that was never trusted must not become trusted, nor have anything of it run, without the user's answer.

1. Measure first, with a test on the real backend in a temp home: a project window is open and trusted; a second folder that was never trusted (and holds what a restricted project must not run: a run configuration and a task file made by the test) is attached to the workspace through the action the user has (find it: the attach action and the Open dialog's "attach" answer). Record what the native app does today: is a trust question shown, is the folder stored as trusted (`backend.host.trust.trusted(path)` before and after), is it attached, and what the window's restricted state is afterwards. Keep that run's log.
2. Then make it what the webview does, exactly: which question, in which window, with which answers, what each answer stores and whether the folder is attached after it (read `attachProject` and what follows it). A declined question attaches nothing and trusts nothing. If the webview attaches an untrusted folder in a restricted state instead of asking, do that. No answer is ever assumed.
3. Tests, each seen failing with the change switched off (one mutation per script run that restores the file in a trap and compares hashes; `docs/gpui-port/notes/tools/mutation_check_example.sh`): the question is asked; each of its answers ends in the stored trust and the attachment the webview gives; nothing of the attached folder is run or indexed as trusted before the answer; an already trusted folder is attached without a question.
4. While there, read every other place where the native open flow answers a trust step itself (`grep -n "trust_confirmed\|TrustAnswer\|trust_read" app-gpui/src`), compare each with the webview, and list any other step answered without the user: fix those that are the same few lines, list the rest with their `file:line`.

## Part 1: the editor tabs settings

An audit (`docs/gpui-port/notes/UNWIRED.md`, its section on settings keys) found, by text, settings that native Settings pages write and that no native code reads. This batch is Editor › General › Editor Tabs, written at `app-gpui/src/settings_model/builtin_pages/editor.rs` near lines 321 to 342: `editor.tabs.pinnedSeparateRow`, `showFileIcon`, `showExtension`, `showDirectoryForNonUnique`, `markModified`, `showFullPathOnHover`, `openAtEnd`, `tabLimit`, `closingPolicy`. The tab limit's model exists and has no production caller (`app-gpui/src/editor/tabs.rs`, `enforce_tab_limit`, as the row `unit:src/editor/core/cm/prefs.test.ts` says). The audit matched text, so it can be wrong in both directions: measure first.

For each key:

1. What the webview does (grep the key and its option name in `src/`, the tab strip's component and store, and their spec files): exactly what changes on screen or in the tabs (the label's text for a unique and a non-unique name, the icon, the modified mark, the tooltip, where a new tab opens, which tab closes when the limit is passed and which never do: pinned, modified, the active one; which tab is activated after a close for each closing policy), and when a change of the setting takes effect (at once for open tabs, or for new ones only).
2. A test through a project window on the real backend that shows whether the native app does it: the setting changed through the Settings dialog's Apply (or the test home's settings file for the default), files opened through the action, and the tab strip read as the user sees it (labels, marks, order, the active tab, the tooltip's text where the test platform can hover). Run it before changing anything and keep the log: it fails (no effect) or passes (the audit missed a reader: say which code reads it; the test stays as the proof).
3. Where it fails: the smallest change that makes the tab strip follow the setting, in the place the strip is built for every pane, taking effect when the webview's does. No assertion of an existing test is weakened: an existing test whose expectation changes because a default now applies is named with the webview's `file:line`; if more than six existing tests change, stop after the sixth and report before going on.
4. The tab limit closes tabs: a closed tab's document is released as any closed tab's is, an unsaved tab is never closed by the limit, and Reopen Closed Tab brings back what the limit closed if the webview's does (read it).

If the batch is larger than one pass: Part 0 first, then `tabLimit` and `closingPolicy`, then `openAtEnd`, then the label keys. When about two thirds of your budget is used, finish the key in hand, run the gates on what has landed, and report which keys are done and which are not started. Never leave a key half applied.

## Hard rules

- Tests use a temp home and project; no network; nothing under the user's home; no program of the machine is run by a test (the "run configuration" of Part 0 is only a file on disk: assert that nothing started). Deletes never reach the system Trash.
- The repository this is pushed to is public: your report and files hold no name, path, branch, author or content of any project of the owner other than this repository.
- No new dependencies, no `unsafe`, no stubs, no `#[ignore]`.
- The real binary is not needed for Part 0. For Part 1, once, through `$S/insp8/run.sh` with a neutral `INSPECT_BASE` and a temp project, light and dark: the tab strip with a pinned tab, two files of the same name in different folders, a modified tab, and the limit set to 3 with five files opened. Never the Agents tool window, Services, Settings' Agents pages or a debug session.

## Rows

Find the rows in `docs/gpui-port/parity.json` whose proofs these touch (the trust rows: `action:project.trust`, `ipc:project_trust_status`, `point.registerOverlay:project.trustDialog`, `module:src/platform/projectTrust.ts`, the attach action's row; the tabs: the settings page's row for Editor › General, `unit:src/editor/core/cm/prefs.test.ts`, the tab strip's spec rows): say for each which clause is now proven, by which test, and which row is at parity today on a proof that Part 0 shows to be false. Do not write to `parity.json`.

## Changed since

HEAD is `23f7e446d`. Since the audit's export (`81efb75f7`) six integrations landed, so every line number above is approximate: re-anchor by name or quoted text and read the place before editing. What they changed that is near this slice: the open flow and trust (`workbench/app_shell.rs`, `workbench/app_actions.rs`, `workbench/windows.rs`, `app_models/windows.rs`: the trust question about a project that opens in a new window is now asked in the current window first, with `TrustAnswer::NewWindow` and `OpenFlow::new_window_trust`: read them before Part 0, and keep their tests passing unchanged); saving (an editor is saved through `Editor::save_document`; idle saving in `workbench/idle_save.rs`); the Git log; the Welcome window; the Keymap page. The tab strip's code did not change in them (check with `git log --oneline 81efb75f7..HEAD -- app-gpui/src/editor/tabs.rs`).

The other agents' undeclared drafts now in the tree, none of them yours, are listed in `$S/undeclared_now.txt`: leave every file named there alone. Eight tests that wait for a run or debug process to stop timed out once under a machine load of 80 to 130 (`workbench::debug::tests::proofs40_debug_guards::*`, `workbench::tests::proofs18_e2e_run::cmd_f2_stops_a_running_process`, `workbench::tests::proofs40_run_start::*`) and passed in clean runs: if they fail in your whole-suite runs, quote them and go on.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside either (write `uptime` before and after each run into the same file); after the second, every test you added or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&`. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Your final message must be the full report (no short closing line after it). Besides the format of `$S/int_rules.md`: Part 0 first: what the native app did before (measured), what the webview does (`file:line`), what it does now, each test with its mutation, the other trust steps you read. Then one section per key: the webview's rule, the first run's result, the change, the tests. Then existing tests changed with the webview's line, the shots' paths and what each shows, the rows table, and what in this brief or the audit turned out false.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
