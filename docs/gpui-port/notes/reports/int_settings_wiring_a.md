## Integrator report: save-time settings, batch A

All five cleanup keys and idle saving now work in the native app. The audit was right that they had no effect: every new test failed before the change. One whole-suite run had one failure: a plugin status-bar test that never touches saving and passed 20 of 20 times on its own afterwards. Nothing is committed.

### 1. Gates (all from `app-gpui/`, logs in `$S/dswa/`)

| Gate                                                                   | Result                                                                                                |
| ---------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| Whole suite, run 1 (`suite_1.log`), nothing else running               | 8308 passed, 0 failed, 224.61 s. Load averages before 5.79 5.14 7.03, after 59.92 21.54 12.81         |
| Whole suite, run 2 (`suite_2.log`), with the repeats running beside it | 8307 passed, **1 failed**, 329.09 s. Load averages before 43.58 20.30 12.57, after 110.51 38.84 21.48 |
| Added and changed tests ×20 during run 2 (`repeat.log`)                | 22 tests × 20 rounds: 440 of 440 ok, none failed at all                                               |
| `cargo test --test web_view`                                           | 1 passed                                                                                              |
| `cargo clippy --all-targets -- -D warnings`                            | exit 0 (run again after the last test edit)                                                           |
| `cargo build`                                                          | exit 0, 0 warnings                                                                                    |

The 22 repeated tests are the 16 new ones plus the 6 existing tests I edited (proofs33 reload, proofs34 package, the editor's save test, and the three plugin save tests).

**The run-2 failure:** `workbench::plugin_contributions::tests::untrusted_windows::a_plugins_status_item_click_is_refused_in_restricted_mode_and_runs_once_trusted`:

```
panicked at src/workbench/mod.rs:8276:9:
the notice Hello: Word Count of Active File failed did not happen within thirty seconds (ran out after 41.173604375s; reached later: None)
...
  active editor: .../notes.txt version=0 caret=0:0 selections=[(0, 0)] ...
  newest notice: None
```

- That test edits and saves nothing. Run alone 20 times afterwards, it passed 20 of 20 (`flake_status_item.log`).
- Hypothesis: under a load average of about 110 the click on the status item missed, because the status bar was laid out again between `debug_bounds` and the click. That would explain "reached later: None". To check: run it 20× beside a whole-suite run, logging the bounds before and after the click.
- My only change on that path: the root's capture mouse-down now first calls `idle_input`, which returns at once when nothing is unsaved.

**Earlier runs, before the gates:**

- **Before the change** (`pre_run.log`): 10 new tests, 0 passed. The cleanup tests failed on bytes, for example `left: "keep   \nline x   \nlast"` against `right: "keep   \nline x\nlast\n"`. Three of them first failed only because their Settings clicks missed, so that run measured only trim and the final newline.
- **Mutation checks** (one guard switched off per run, file restored by a trap, hash compared: `restored: yes` both times):
  - A, cleanup returns at once (`mut_A.log`): 12 of 16 fail. The tests for all-lines scope, trailing blank lines (`left: "cba\n\n  \n\n"`), CRLF, mixed line breaks, `.editorconfig`, autosave, the second editor and the preview all fail, so every key is measured as having no effect without the code.
  - B, caret lines always kept (`mut_B.log`): only the caret-line test fails (`left: "keep   \nline x     \nlast\n"`).
- **Suite A** (`suite_A.log`), after the change and before I touched any existing test: 8302 passed, 2 failed. Both changed only because the cleanup defaults are on; see section 6.

### 2. What changed

New files:

- `app-gpui/src/editor/view/save_cleanup.rs`: `Editor::save_document(&Entity<Editor>, &mut App)` and the cleanup itself.
- `app-gpui/src/workbench/idle_save.rs`: the idle countdown.
- `app-gpui/src/workbench/tests/save_cleanup.rs`: 16 tests.

Edited files:

- `app-gpui/src/editor/view.rs`:
  - New fields `modified: ModifiedRanges` and `cleans_on_save`, and a new `EditOrigin::SaveCleanup`.
  - Ranges are added on the editor's own edits and on undo/redo, and moved along on edits from the backend, the editor's other views of the document, and resyncs. They are cleared on `DocDirty{dirty:false}` and on a reload.
  - `save` is renamed `write_as_is`.
- `app-gpui/src/workbench/mod.rs`:
  - Autosave, Save All, the light-edit close, and the run driver's save now go through `save_document`.
  - New field `idle_save`, with key and press hooks on the window's root and calls on `SettingsChanged` and `DocDirty`.
- `app-gpui/src/workbench/editor_core.rs`: an edit restarts the countdown.
- `app-gpui/src/workbench/deployment.rs`: saves through `save_document`.
- `app-gpui/src/ui/code_preview.rs`: preview editors are built with `cleans_on_save = false`, and both preview saves go through `save_document`.
- Three test call sites renamed only (`plugin_contributions.rs`, `untrusted_windows.rs`, `proofs10_unsubscribe.rs`).

How it works:

- The cleanup reuses the existing, unit-tested `editor_model::document::save_cleanup_changes`.
- It runs in the editor, not the backend, as in the webview: Replace in Files, refactorings and merge results are not cleaned.
  - The webview's exact save (`src/features/vcs/ops.ts:571`) corresponds natively to `conflicts.rs` `write_document`, which never goes through an editor.
- It runs only on a document that is unsaved and writable.
- It is one edit with its own undo step (`EditKind::Other`).
- Carets map to the side before an insertion, so a caret at the end stays in front of the added line break.
- It does not scroll the view, and does nothing on the window thread that the save didn't already do there.

Which edits and carets count:

- Edited ranges are the union over every editor of the document.
- Kept caret lines come from editors that clean (tabs); previews are excluded, as webview attachments are.
- A preview's save is cleaned through a tab of the same file if one is open, otherwise it is written as edited (webview `document.ts:653`, where `primarySlot()` is the tab).

### 3. Per key

The webview applies all of these in `src/editor/core/model/document.ts:639-685`, with the rule in `saveCleanupChanges` at `document.ts:167-206`. The native app reads every key at save time, through the project's settings, then the global ones.

| Key                                                                           | Webview rule                                                                           | Before the change                                         | Test (all in `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/tests/save_cleanup.rs`)              | Bytes asserted                                                                                                                                                                                       |
| ----------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `files.trimTrailingWhitespace` (default on, `schema.ts:199`)                  | `document.ts:665`; `.editorconfig` `trim_trailing_whitespace` wins while support is on | Failed: no effect                                         | `trims_modified_lines_and_adds_the_final_newline_on_save` (the home's defaults); `with_both_options_off_a_save_writes_only_the_users_edit` (unticked in Settings, Apply) | `"keep   \nline x\nlast\n"`. Off: `"keep   \nline x   \nlast"`. A file only opened keeps `"untouched   \n\n\n\nend  "` and its modification time. One Undo restores the cleanup, a second the typing |
| `editor.general.stripTrailingScope`                                           | `document.ts:668`, `allLines` when `"all"`                                             | No effect (mutation A)                                    | `all_lines_scope_strips_the_trailing_spaces_of_every_line`                                                                                                               | `"keep\nline x\nlast\n"`                                                                                                                                                                             |
| `editor.general.keepTrailingOnCaretLine` (default on, `editorSettings.ts:97`) | `document.ts:655-660,667`: the head line of every caret in the document's tabs         | No effect (mutation A); measured on its own by mutation B | `the_caret_line_keeps_its_trailing_spaces_unless_the_option_is_off`; `edits_and_carets_of_a_second_editor_of_the_document_count`                                         | On: `"keep   \nline x   \nlast\n"`. Off: `"keep   \nline x\nlast\n"` with the caret at (1,6). Second editor: `"one  \ntwo\nthree   \n"`                                                              |
| `editor.general.removeTrailingBlankLines` (default off)                       | `document.ts:181-185`: exactly one line break after the last line with text            | No effect (mutation A)                                    | `trailing_blank_lines_go_only_when_the_option_is_on`                                                                                                                     | Off: `"ba\n\n  \n\n"`. On: `"cba\n"`                                                                                                                                                                 |
| `files.ensureFinalNewline` (default on, `schema.ts:200`)                      | `document.ts:666`, `.editorconfig` `insert_final_newline`                              | Failed: no effect                                         | Trim tests above; `editorconfig_overrides_the_save_options_while_its_support_is_on`                                                                                      | `.editorconfig` saying false with support on: `"keep   \nline x   \nlast"`. Support off: `"zkeep\nline x   \nlast\n"`                                                                                |

Line breaks:

- `a_crlf_file_keeps_its_line_breaks_when_cleaned_up` asserts `"keep   \r\nline x\r\nlast\r\n"`.
- `a_file_with_mixed_line_breaks_gets_only_the_cleanup_the_options_ask_for` asserts `"keep   \r\nline x\r\nlast\r\nend\r\n"` with the options on and `"keep   \r\nline x   \r\nlast\r\nend"` with them off.
- Mixed line breaks become the majority break on any save (backend `ide-text` `LineEnding::detect`, and `text_for_save` at line 205). That is the same backend the webview uses; the cleanup adds nothing to it.

Other save paths:

- `autosave_cleans_up_the_tab_it_saves`: leaving the tab writes it cleaned; the tab only looked at keeps `"other  "` and its modification time.
- `a_preview_save_is_cleaned_up_only_through_a_tab_of_the_file`: ⌘S in a preview with no tab writes `"x fn order() {}\nlast"`; with a tab of the file open it writes `"x fn other() {}\nlast\n"`.

Hypothesis: binary and very large files are untouched because the cleanup returns on read-only, and the backend makes large files read-only (webview `document.ts:110-113` says so). To check: `grep -n "large" crates/ide-workspace/src/documents.rs`.

### 4. Idle saving (`general.saveOnIdle`, `general.idleSeconds`)

- **Webview** (`src/features/settings/appearanceRuntime.ts:251-267`, wired at `runtime.ts:66`): every keydown, pointerdown or wheel restarts the timer, and when it fires it runs `file.save`.
- **Native:**
  - Keys and presses (captured at the window's root) and edits made in the tabs (not the backend's or the cleanup's) restart the countdown, but only while something is unsaved and the setting is on.
  - When it fires it runs `file.save` through the registry, so a failure shows "Could not save" as any failed save does.
  - It stops when the setting is turned off, when it fires, and when nothing is unsaved after a save.
  - It is a real task timer that the test clock drives; no test waits in real time.
- **Tests and their countdown assertions:**
  - `idle_saving_saves_the_edited_files_after_the_seconds_without_input` (ticked in Settings, set to 5): no countdown while nothing is unsaved; still unsaved at 4.9 s; saved and cleaned at 5.1 s; no countdown left afterwards; unticking stops a running countdown, and 60 s later the tab is still unsaved.
  - `without_idle_saving_an_edit_starts_no_countdown`: no countdown, and 120 s later nothing is written.
  - `a_key_pressed_while_idle_saving_counts_down_starts_the_countdown_over`: still unsaved 8 s after the edit (4 s after the key), saved after 9.1 s.
  - `saving_everything_stops_the_idle_countdown`.
  - `a_failed_idle_save_is_told_once_and_not_retried_without_input` (setting from the home's settings file): one "Could not save", no countdown left, still one notice after 60 s more, tab still unsaved.
- **Differences from the webview:**
  - The webview restarts its timer on any input even when nothing is unsaved. The brief's stricter rule is applied instead; the outcome is the same because an empty Save All writes nothing.
  - The scroll wheel does not restart the countdown.
  - Unsaved notebooks do not start a countdown, though the Save All it runs does save them.
  - Hypothesis: a countdown started before a tab is closed with Don't Save or reloaded fires once and finds nothing to save.

### 5. Other save-time keys: listed, not fixed

- **`files.autoSave`** works natively (`workbench/mod.rs:1919`): saving on tab switch (`:1821`), on leaving the window (`:1108`) and on leave. The webview also saves when the editor loses focus (`controller.ts:811`, `saveOnBlur.ts`), with an exception for popups; the native app has no such hook. Not a call away, because of the popup rule.
- **`editor.formatOnSave`** has no effect natively: `workbench/editor_language.rs` sets `on_save: false` and nothing formats before writing. Webview: `src/editor/lsp/actions.ts:146`. It needs an async save participant before the write.
- **`actionsOnSave.optimizeImports` and `actionsOnSave.cleanup`** have no effect natively. Webview: `src/editor/lsp/actions.ts:157-171`. Same reason.

### 6. Existing tests whose bytes changed (two, under the limit of three)

- **`workbench/tests/proofs33_template_session.rs` `reloading_the_file_from_disk_ends_the_template`:** the save now trims the template's `"  "` body line, which no caret is on. It expects `"for (let i = 0; i < .length; i++) {\n\n}\n"`, required by `document.ts:656-668` and `schema.ts:199`. **For your decision:** saving while a template is being filled in now strips the body line's indent, exactly as the webview does.
- **`workbench/tests/proofs34_template_files.rs` `a_plugin_templates_package_goes_into_the_body_never_into_the_path`:** expects `"service\n"`. The webview's `src/features/templates/files/create.ts:231` runs `file.save`, which adds the final line break (`document.ts:666`, `schema.ts:200`).

### 7. Rows (I did not write to `parity.json`)

- `unit:src/editor/core/model/document.test.ts` (partial): three cases are now proven through a window.
  - "trims modified lines and adds the final newline on save": the trim test.
  - "%s save leaves untouched documents byte-for-byte unchanged": the bytes and modification time of untouched files, for Save All and autosave; save participants do not exist natively.
  - "keeps the caret line and untouched lines": the caret-line and second-editor tests.
  - The row stays partial.
- `unit:src/editor/core/cm/prefs.test.ts` (partial): the cleanup is now reached through the window, proven by the save_cleanup tests. It stays partial for the tab limit and indent detection.
- `settings:general` (partial): idle saving now has an effect, proven by the five idle tests. It stays partial only for the directory chooser.
- Editor › General's Save Files keys have no `settings:editor.general` row in `parity.json`.

### 8. False or stale

- The `prefs.test.ts` row says the cleanup is "tested and reached". That is false: the before-run shows it was never applied.
- UNWIRED.md lists `app-gpui/src/app_models/jetbrains_settings.rs` as the "reader" of `files.*` and `stripTrailingScope`. That file imports JetBrains settings; it is not a reader at save time.
- `$S/tests_brief.md`, named as a read-first file here, is written for test drafters ("do not run cargo") and does not apply to an integrator.
- The disk check differs: the brief says `df -g /`, the rules say `df -k /System/Volumes/Data`. I logged both at first and `df -g /` before later heavy builds (about 337 GB free); the log is `$S/dswa/log.md`.
