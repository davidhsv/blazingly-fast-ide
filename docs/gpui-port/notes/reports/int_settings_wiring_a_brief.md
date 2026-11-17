# Integrator brief: settings that are written and read by nothing, batch A: what happens to a file when it is saved

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`. Your own scratch subfolder: `$S/dswa/`.

Format only files you edited, taken from `git diff --name-only`. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Use `git diff -U3` when you read a diff. Leave no polling loop behind.

## What this is

An audit (`docs/gpui-port/notes/UNWIRED.md`, its section on settings keys) found, by text, settings that native Settings pages write and that no native code reads, so changing them does nothing in the running app, while their pages' rows said parity because a test changed the control and read the stored value back. The Language setting was the first found and is fixed. This batch is the keys that decide what is written to disk when a file is saved, and when it is saved:

- `files.trimTrailingWhitespace`, `editor.general.stripTrailingScope` (all lines or modified lines only), `editor.general.keepTrailingOnCaretLine`, `editor.general.removeTrailingBlankLines`, `files.ensureFinalNewline` (written at `app-gpui/src/settings_model/builtin_pages/editor.rs` near lines 134 to 145);
- `general.saveOnIdle` with `general.idleSeconds` (written at `app-gpui/src/settings_view/pages/general.rs` near 32; a reader exists in `app-gpui/src/settings_model/runtime.rs`, which nothing calls);
- and, if you find them in the same code path while reading, the other save-time keys of the webview (save on focus loss, save on window deactivation, format on save, optimize imports on save: read `src/editor/core/` for the save participants and the autosave triggers, and the General and Editor pages of the webview's Settings): list each with whether it has an effect natively; fix those that are a call away, list the rest.

The audit matched text, so it can be wrong in both directions: a key may be read through a typed field the script did not connect (the audit's own row for the editor preferences says cleanup on save is "reached"), and `.editorconfig` may override a setting (read `crates/ide-workspace/src/editorconfig.rs` and how the documents' file defaults are built: `FileDefaults::from_settings`). So measure first.

## For each key

1. What the webview does (the function that applies it, with `file:line`, and its spec cases: grep the key and the option's name in `src/` and its tests): exactly which bytes change on save (trailing spaces and tabs; which lines when the scope is modified lines; the caret's line when it is kept; blank lines at the end; one final newline; the file's own line ending kept), what an `.editorconfig` entry does to it, and what never triggers it (a save of an unchanged document? autosave? a save made by Replace in Files or a refactoring?).
2. A test through a project window on the real backend that shows whether the native app does it: the setting written through the Settings dialog's Apply (or the settings file of the test's home, for the default), a file edited in an editor, saved (the action through the registry), and the bytes on disk compared with what the webview's rule gives. Run it before changing anything and keep the log: it either fails (the setting has no effect: the audit was right) or passes (the audit's text match missed a reader: then say which code reads it, and the test stays as the proof).
3. Where it fails: the smallest change that makes the native save follow the setting, in the place where a save is prepared for every kind of document that the webview cleans (not the notebook's or an image's save: read which), with the caret and the selections kept where the webview keeps them, one undo step if the webview makes it one, and nothing done on the window's thread that the save did not already do there.
4. Idle saving: the editors' documents are saved after the configured seconds without input when the setting is on, and never when it is off; a timer exists only while there is something unsaved (the app must not tick when idle: assert no timer is left after the save and when the setting is off); a save that fails is told as any failed save is and does not loop. Test with the test clock, never with a real wait.

## Hard rules

- These settings change users' files. Every test asserts the file's exact bytes after the save, for the setting on and off, and that a file the user did not edit is not rewritten (its modification time or a watcher: read how other save tests assert "not written"). A setting that is off changes nothing in the file beyond the user's edit.
- Mixed line endings and files without a final newline keep what the webview keeps; binary and very large files are not touched (read the webview's guards).
- Tests use a temp home and project; no formatter, language server or other program of the machine; no network; nothing under the user's home; deletes never reach the system Trash.
- No assertion of an existing test is weakened: an existing save test whose bytes change because a default is now applied (trim and final newline default to on in the backend's settings: `crates/ide-core/src/settings.rs` near 1051) is named with the webview's `file:line` that requires the new bytes; if many existing tests change for that reason, stop after the first three and report before going on: the default's effect on every existing test is the manager's call.
- No new dependencies, no `unsafe`, no stubs, no `#[ignore]`.

## Rows

The settings rows of the pages that hold these keys and the unit rows of the specs you read (find the keys in `docs/gpui-port/parity.json`; `unit:src/editor/core/cm/prefs.test.ts` was lowered on 2026-11-17 for the tab limit and indent detection, not for cleanup): say for each what is now proven through a window. Do not write to `parity.json`.

## Changed since

HEAD is `6ed9bfb98`. Since the audit's export (`81efb75f7`) two commits changed `app-gpui/` (measured: `git log --oneline 81efb75f7..HEAD -- app-gpui`; 19 files, +838 −180): the UI now follows the Language setting (`platform/ui_language.rs`), and Search Everywhere gained its scope box, history and layout (`everywhere/view.rs` and its parts, `workbench/tests/proofs41_*`). Neither touches saving. The other agents' undeclared drafts now in the tree, none of them yours, are listed in `$S/undeclared_now.txt` (the reach check's output of the last verification): leave every file named there alone.

While you work, another agent runs release builds of both apps from a separate folder (`/private/tmp/openidea-measure-i3NdVc`) and posts keys to them: never touch that folder or its processes, never launch any app or real binary yourself, and never signal a process you did not start. The machine also carries the owner's own load: a test that fails only under load is still yours to make robust by fixing its wait.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&`, in a script or from the shell. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Your final message must be the full report (no short closing line after it). Besides the format of `$S/int_rules.md`: one section per key: the webview's rule with `file:line`, the first test run's result (failed: no effect; passed: read by which code), the change, the tests with the bytes they assert; the idle-save section with the timer assertions; existing tests whose bytes changed and why; the keys you listed and did not fix.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`. The real binary is not needed for this slice.
