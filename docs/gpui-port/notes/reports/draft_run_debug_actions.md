I wrote and wired 6 of the 17 actions, each with an end-to-end test: `run.runContext`, `run.debugContext`, `run.modifyContext`, `run.chooseDebugConfig`, `debug.evaluate` and `debug.smartStepInto`. The other 11 are not registered (reasons in §2). Nothing has been compiled or run, so every test result is still a Hypothesis.

## 1. Files written

- `app-gpui/src/debugger/evaluate_dialog.rs` (594 lines) is the Evaluate dialog.
  - Constructor: `EvaluateDialog::new(debugger, expression, history, window, cx)`.
  - Events: `EvaluateDialogEvent::{Evaluate{request, expression}, AddWatch{expression, close}, Closed}`.
  - Accessors: `field`, `expression`, `history`, `result`, `result_line`, `paused`, `can_evaluate`, `note`, `buttons`, `press`, `evaluated`.
  - History helpers: `history_key`, `read_history`, `pushed_history`.
  - Tests: 2 plain `#[test]` and 4 view tests.
- `app-gpui/src/workbench/debug_extras.rs` (505 lines): `DebugExtrasSlot`, `SmartStepChoice`, 3 end-to-end tests, and a test fixture that `run_context.rs` also uses.
- `app-gpui/src/workbench/run_context.rs` (730 lines): `RunContextSlot` and 5 end-to-end tests.
- Integration notes (256 lines): `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/run_debug_actions_integration.md`.
  - The `pub mod` line and the `workbench/mod.rs` hunks: two fields, the modules, the registration and the Evaluate layer.
  - Hunks in `debug.rs`, `run_configs.rs`, `debugger/actions.rs` (new `Debugger::step_into_target`) and `run/driver.rs` (new `RunDriver::modify_temporary`).
  - The IPC names were checked against `src-tauri/src/commands/`. No line is over 120 columns.

## 2. Actions registered

| Action                  | What it does                                                                                                                                                      | Test                                                                                                                                              | IPC commands                                                                                                               |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `run.runContext`        | The context from the argument or editor is made the selected temporary configuration and run. Several productions → a chooser menu; none → "Nothing to run here". | `run_context_runs_the_file_as_the_selected_temporary_configuration`, `run_context_says_so_when_the_file_has_nothing_to_run`                       | `run_configs`, `fs_read_text`, `sdk_locate_tools`, `sdk_python_effective`, `sdk_php_effective`, `fs_read_dir`, `run_start` |
| `run.debugContext`      | Same context, debugged through `run.debugConfig`. A compiled file is built first, then debugged.                                                                  | `debug_context_hands_the_files_configuration_to_the_debugger`                                                                                     | as above, plus `debug_start`                                                                                               |
| `run.modifyContext`     | Opens Edit Configurations on that temporary configuration, with changes pending.                                                                                  | `modify_run_configuration_opens_the_contexts_configuration_in_edit_configurations`                                                                | as above, plus `run_without_derived`, `run_templates`                                                                      |
| `run.chooseDebugConfig` | Opens the chooser in Debug mode, or closes it when open.                                                                                                          | `debug_opens_the_chooser_in_debug_mode_and_enter_debugs_the_configuration`                                                                        | `run_configs`, `debug_start`                                                                                               |
| `debug.evaluate`        | Opens the dialog on the selection. Enter evaluates; the expression goes into the project history; Add to Watches adds and closes.                                 | `evaluate_expression_evaluates_the_selection_in_the_paused_frame_and_adds_it_to_the_watches`                                                      | `debug_evaluate_in`                                                                                                        |
| `debug.smartStepInto`   | Steps straight in, or lists the calls in a menu and steps into the chosen one.                                                                                    | `smart_step_into_steps_straight_in_when_the_debugger_lists_no_calls`, `smart_step_into_lists_the_calls_of_the_line_and_steps_into_the_chosen_one` | `debug_step_in_targets`, `debug_step_in_target`, `debug_step`                                                              |

Not registered:

- **`debug.browseConsoleHistory`:** the debug console pane is read-only, so the model's "console input focused" flag is never true.
- **`debug.quickEvaluate`:** there is no value hint in the editor; `debug.rs` only logs that dispatch.
- **`debug.attach`:** `plugin_java_debug.rs` already calls it directly. Testing it needs a JVM or debugpy.
- **`debug.attachToProcess` and `run.updateApplication`:** their models exist; the views were not written in this pass.
- **`run.importVscodeLaunch` / `run.importJetbrains`:** the backend ops exist (`run_import_sources`, `run_import_configs`); the import dialog was not written.
- **`run.rerunFailed`:** nothing publishes `run.canRerunFailed`.
- **`run.withExecutor`:** no executor is contributed, because coverage and the profiler are not built.
- **`run.sampleOutput`:** the webview only registers it in dev and perf builds.
- **`run.pluginStart`:** its argument carries a JavaScript callback (`onExit`). Not attempted.

## 3. Spec files

None of the 13 new tests is named after a spec sentence. Their spec cases are already covered by model tests, and these end-to-end tests cover only part of each:

- The Evaluate test covers the selection half of "Evaluate prefills the expression at the caret, or the whole selection" (`actions.test.ts`).
- The two Smart Step tests cover "chooser for several calls" and "Step Into without support" (`actions.test.ts`), but not "direct step for one".

## 4. Not built, assumptions, likely failures

Not built:

- **Current File re-selection:** Run or Debug with Current File selected now runs the context. The temporary configuration then stays selected; the webview selects Current File again. The notes describe a fix.
- **Producers' input:** Go modules, Python packages, content roots, the Gradle model, templates and source roots are left at their defaults.
- **Run targets:** a debug on a non-local run target is not refused (`remoteDebugBlocked`).
- **Build then debug:** written, but no test (no compiler in tests).
- **Evaluate dialog:** one-line field (multi-line selections are joined), no completion, no History button, and the result cannot be expanded.
- **Smart Step Into:** a menu instead of the dialog, and no inline picking in the editor.
- **Menus and gutter:** the Run group of the editor, project view and tab menus and the run gutter are not wired anywhere natively.

Check first:

- Hypothesis: the extensionless `tool` file (`#!/bin/echo hello`) produces a configuration named `tool`, id `context:file:tool`, that runs `/bin/echo hello tool`.
- Hypothesis: a configuration of kind `echo`, which has no registered type, starts and is listed in Edit Configurations.
- Hypothesis: the dialog's claimed keys do not also reach the text field.

Most likely to fail on first run:

- **Smart Step menu test:** whether the session reports the fake adapter's `supportsStepInTargetsRequest`, and whether my patched adapter script is still valid Python.
- **Modify test:** whether `dialog.selected()` reads exactly "tool".
- **Evaluate test:** the watch-row prefix in the Variables pane (Hypothesis).
- **Nothing-to-run test:** whether `README.md` produces nothing (Hypothesis).
