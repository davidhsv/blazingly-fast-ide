I finished Parts A to D and ran every gate, but two things are open. Two of my three full suite runs failed one test, and the agents test from Part A failed 1 time in 50.

## 1. Gates (measured)

All runs are from `app-gpui/`; the outputs are in `$S/d108/`.

| Gate                                                                              | Result                                             |
| --------------------------------------------------------------------------------- | -------------------------------------------------- |
| Suite run 1 (`suite1.txt`), with the Part A1 test looping beside it               | FAILED: 7211 passed, 1 failed, 0 ignored, 292.87 s |
| Suite run 2 (`suite2.txt`), with the Part A2 test looping beside it               | FAILED: 7211 passed, 1 failed, 0 ignored, 292.80 s |
| Suite run 3 (`suite3.txt`), extra run with nothing beside it                      | ok: 7212 passed, 0 failed, 0 ignored, 159.39 s     |
| `cargo test --test web_view`                                                      | ok: 1 passed                                       |
| `cargo clippy --all-targets -- -D warnings`                                       | exit 0                                             |
| `cargo build`                                                                     | 0 warnings                                         |
| Every test I added, declared or changed (38 tests, `tests_20x.txt`), 20 runs each | 20/20 for all 38 (`twenty_summary.txt`)            |
| Part A1 test, 50 runs beside suite 1                                              | 50/50                                              |
| Part A2 test, 50 runs beside suite 2                                              | 49/50                                              |

- **The failure in suites 1 and 2** was the same test both times: `workbench::plugin_java_debug::tests::stop_ends_the_process_and_the_session`. The message was "the process ends did not happen within thirty seconds", and the condition was reached later both times, at 73.97 s and 74.63 s. I did not edit that test or its module. Alone it passed 3/3 and then 20/20, and suite 3 passed with it.
  - Hypothesis: the doubled load from the loop running beside the suite pushes the stop past 30 s, and the process then ends on its own at about 74 s. Confirm with `loop.sh` on that test 50 times beside a full `cargo test --lib`.
- **The Part A2 failure** in its 50 runs was again "the dialog did not happen within thirty seconds … reached later: None".
- **An older failure** I found while reading d107's suite runs: `workbench::context_menus::tests::move_to_in_a_tool_windows_options_moves_it` failed once in `d107/run1/g_suite1.txt`. I did not look into it.
- Nothing under `src/`, `src-tauri/`, `docs/`, `e2e/` or `crates/` changed. No new dependency, no `unsafe`, no `#[ignore]`.
- The formatted files came from `git diff --name-only` plus `proofs16_run_badge.rs` and the three proofs16 test files handed to me, all through `fmt.sh` (list in `$S/d108/fmt_list.txt`).

## 2. Parts

### Part A1: the restricted-banner test

- The test now waits with `wait_long` for the frame without the banner (and with it, for the two "banner is drawn" checks) instead of reading the bounds once after `run_until_parked`.
- The cause comes from reading the code, not a measurement: `decide_trust` changes the trust store in a backend call and the workbench is redrawn only in a later step, so "not restricted" can be true one frame before the banner disappears.
- The same wait is in `offers_three_choices_cancels_without_a_trust_decision_and_supports_restricted_preview` (20/20).
- File: `app-gpui/src/workbench/app_actions.rs`.

### Part A2: the agents dialog test

- No wall-clock wait or backend thread sits between the click and the dialog. The click goes through `SessionEvent::Message`, then `ReplyActions::regenerate`, `AgentsStore::rerun_from` (an immediate confirm request) and `show_prompt_dialog`.
- The paths that can end silently are the two early returns in `regenerate` (a reply action already under way, or `resolve` returning `None`) and a click that never lands.
- Measured: "reached later: None" with `notices=[]`, so 30 more seconds did not help, in d106 and in my 49/50 run.
- Not fixed, because I don't yet know which silent return it is. Hypothesis: the drawn frame is older than the store state at the moment of the click. To confirm, after the click in `proofs13_agents_failed_send.rs` record the reply-actions busy flag (`ReplyActions::is_busy`), `store.task(task).status` and the reply's regenerate permission (`reply_operations(...).regenerate`), and print them in the failure message.

### Part B: zoom

**What changed:**

- `ActiveTheme.zoom` plus `theme::zoom` and `theme::zoomed` (`theme/mod.rs`, `theme/appearance.rs`).
- `theme::icon` scales every icon by the zoom but keeps the icon variant of the unzoomed size. I traced all non-literal sizes passed to it; all four are constants, so nothing is scaled twice.
- Editor font size, line height, editor font zoom and the fold column (`editor/view.rs` `metrics`). Line numbers follow because they use the same font size.
- Diff and merge viewers (`vcs_diff/view.rs` `CodeStyle::of`, `viewer_line_height`).
- Terminal font size (`terminal/view.rs`).
- The Project view's chevron box (`project_tree.rs`).
- The shared dialog helpers `frame`, `title_row`, `button`, `checkbox` and `field_frame` (`app_views/mod.rs`).
- Test-only accessor `Editor::painted_text_metrics`, and a `icon:<name>` debug selector on every icon (debug and test builds only).

**Webview source:** `src/features/settings/appearanceRuntime.ts:58-67`.

**Test:** `workbench::zoom_actions::tests::the_ide_zoom_scales_the_editors_code_and_the_icons`. At 150 % the painted code font size, the code row height and the Project chevron icon are 1.5 times their 100 % values, and Reset restores them. It goes through `view.zoomIn` and `view.resetZoom`.

**Screenshots:** taken in my own binary's window with temp `OPENIDEA_GPUI_HOME`, `HOME` and `ZDOTDIR`, stopped by PID; the Services window was not opened.

- Files: `$S/d108/insp/shots/zoom-z100-1.png`, `zoom-z150-1.png`, `zoom-t100-1.png`, `zoom-t150-1.png`.
- What I saw at 150 %: editor text, line numbers, every icon (stripe, toolbar, tree, tab), interface text and the terminal text are all larger. The terminal text measured about 1.5 times wider (crops `t100crop.png` and `t150crop.png`).
- Still wrong: close-× icons inside fixed-size pixel boxes overflow and sit off-centre (editor tab, terminal tab).

**Left out:** the 2,590 lines with literal `px(<number>)` outside test folders (inline test modules and `px(0.0)` included). Of those, 1,667 are gap, padding or margin calls written inline in each view, so no shared helper covers most of them.

| Directory     | Literal `px(` lines |
| ------------- | ------------------- |
| vcs           | 388                 |
| database      | 312                 |
| workbench     | 267                 |
| settings_view | 150                 |
| agents        | 147                 |
| run           | 128                 |
| github        | 121                 |
| everywhere    | 111                 |
| editor        | 109                 |
| editor_lsp    | 107                 |

The `ui/` helpers that take a literal size are few: `tooltip.rs` 16, `popup_menu.rs` 13, `popup_anchor.rs` 12. The zoom actions stay partial.

### Part C: proofs16

- I declared the 4 test modules. All 13 written tests pass (20/20 each).
- **One app fix:** the editor of a lone file (opened without a project) had no right-click menu. `light_editor_element` in `workbench/editor_find.rs` now opens the same editor and gutter menus. Webview: the editor owns its menus with no project check (`src/editor/core/model/slot.ts:160-182`, `src/editor/core/gutterMenu.ts:39-50`).
- I added the test `…::proofs16_menus::the_editors_menu_without_a_project_opens_on_the_text_and_copies`.
- **Run stripe badge (built):**
  - `Workbench::stripe_badge(&self, id, cx: &App)`. It needs `cx`, unlike the signature in the notes.
  - The badge is drawn over the Run stripe icon, shows "99+" past 99, and honours Show tool window badges. `RunContext` now carries the running count so the window redraws when it changes.
  - Webview: `src/features/run/index.ts:343-345,384`, `src/workbench/toolwindows/ToolWindows.tsx:381-386,432`, `ToolWindows.module.css:328-344`.
  - Test: `workbench::tests::proofs16_run_badge::the_run_stripe_button_shows_the_number_of_running_processes`, moved out of the pending file into a new declared file.
- **Still in `_pending` files:**
  - `proofs16_todo_run_pending::the_todo_preview_edits_the_file_and_moves_its_caret_to_another_item_of_the_same_file`: waits for an editable TODO preview that keeps one editor per file.
  - `proofs16_menus_pending::the_gutters_add_bookmark_row_is_not_greyed_out_without_a_project`: waits for the bookmark row to stay enabled without a project.

### Part D: Run and Debug actions

- I applied every hunk of the notes. All anchors matched the current code, and the drafts needed only one lifetime fix to compile.
- **Current File re-selection:** Run or Debug with Current File selected now passes `{ "currentFile": true }` from all three callers (`run/driver.rs`, the chooser in `run_configs.rs`, `debug_selected` in `debug.rs`), and the context launch selects Current File again (`run/store.rs` `current_file_arg`). Webview: `src/features/run/index.ts:231-235` `runCurrentFile`.
- **Run group of the menus:** wired the existing `context_menu_items` as group `run.context` in the editor, Project view and tab menus (Run '<name>', Debug '<name>' where the file can be debugged, More Run/Debug › Modify Run Configuration…).
  - The menu builders now take `&mut App`.
  - Like the webview's menu preparation, the Project view reads the start of a file whose run depends on its text, but synchronously and capped at 400,000 UTF-16 units.
  - Webview: `src/features/run/contextRun.ts:341-388`, `src/features/run/index.ts:440-445`.
- **Evaluate dialog:** now built on the shared `app_views` frame helpers inside the shared `dialog_frame`, with a History icon button using the shared tooltip and a menu of the 20 newest expressions. Webview: `src/features/debugger/ui/Overlays.tsx:746-814`.
- **Test fixture:** the draft's `long` wait now calls `wait_long`.
- **Tests:**
  - 9 in `workbench::run_context::tests`, the 5 drafted plus:
    - `run_with_current_file_selected_runs_the_file_and_keeps_current_file_selected`
    - `the_editors_menu_offers_run_for_the_file_and_runs_it`
    - `the_project_views_menu_offers_run_for_the_file_and_runs_it`
    - `the_tabs_menu_offers_modify_run_configuration_for_its_file`
  - 3 in `workbench::debug_extras::tests` (the Evaluate one extended with the History menu).
  - 7 in `debugger::evaluate_dialog::tests`, including the new `the_history_button_offers_the_newest_expressions_and_recalls_one`.
- **Left out:** the 11 actions the writer did not register (reasons in the writer's report); multi-line expressions, completion and an expandable result in Evaluate; the "Step Into" dialog form of Smart Step Into; executor rows (no executor exists); the run gutter.

## 3. Parity rows

`parity.json` has no entry yet for the six `action:` rows, `point.registerMenuGroup:run.context` or the two Smart Step `ipc:` rows. These are new keys.

| Key                                                  | State                                 | Proving tests                                                                                                                                                                                                                                                                                                                                                                | What is missing                                                  |
| ---------------------------------------------------- | ------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| point.registerOverlay:plugins.consent                | done                                  | `workbench::plugins_window::tests::proofs16_plugins::a_running_plugins_permission_request_shows_the_permission_page_and_ok_grants_it`                                                                                                                                                                                                                                        | —                                                                |
| ipc:plugin_grant                                     | done                                  | the same test, and `…::proofs6_plugins::review_permissions_grants_a_plugin_waiting_for_consent`                                                                                                                                                                                                                                                                              | —                                                                |
| ipc:plugin_list                                      | done (no new test, per notes item 10) | `…::project_plugins_per_window::*`                                                                                                                                                                                                                                                                                                                                           | —                                                                |
| point.registerMenuGroup:search.bookmarks             | partial                               | `workbench::context_menus::tests::proofs16_menus::the_gutters_menu_without_a_project_offers_add_bookmark_and_bookmarks_nothing`                                                                                                                                                                                                                                              | Row greyed without a project (pending test)                      |
| point.registerMenuGroup:plugins                      | partial                               | `…::proofs16_menus::a_plugins_editor_tab_entry_is_offered_in_the_tabs_menu_and_runs_its_command`                                                                                                                                                                                                                                                                             | Problems menu, Open With                                         |
| point.registerMenuGroup:agents.send                  | partial                               | `…::proofs16_menus::the_send_to_agent_row_is_left_out_for_a_virtual_document` (model level)                                                                                                                                                                                                                                                                                  | Window test, multi-selection                                     |
| point.registerEditorKind:pluginLog                   | partial                               | `workbench::plugin_runtime::tests::proofs16_runtime::keeps_a_plugin_log_tab_and_its_program_while_another_tab_is_shown`                                                                                                                                                                                                                                                      | Find in Logs                                                     |
| point.registerToolWindow:run                         | partial                               | adds `workbench::tests::proofs16_run_badge::the_run_stripe_button_shows_the_number_of_running_processes`                                                                                                                                                                                                                                                                     | Tabs in the header, test-tree and stack-trace tabs               |
| unit:src/features/plugins/**tests**/plugins.test.tsx | partial                               | the 13 proofs16 tests                                                                                                                                                                                                                                                                                                                                                        | See counts below                                                 |
| unit:src/features/todo/PreviewEditor.test.tsx        | partial                               | —                                                                                                                                                                                                                                                                                                                                                                            | Editable preview, one view per file, TODO colouring (pending)    |
| action:run.runContext                                | done                                  | `workbench::run_context::tests::run_context_runs_the_file_as_the_selected_temporary_configuration`, `…::run_context_says_so_when_the_file_has_nothing_to_run`, `…::the_editors_menu_offers_run_for_the_file_and_runs_it`, `…::the_project_views_menu_offers_run_for_the_file_and_runs_it`, `…::run_with_current_file_selected_runs_the_file_and_keeps_current_file_selected` | —                                                                |
| action:run.debugContext                              | partial                               | `…::debug_context_hands_the_files_configuration_to_the_debugger`                                                                                                                                                                                                                                                                                                             | No test starts a real debug session from a context               |
| action:run.modifyContext                             | done                                  | `…::modify_run_configuration_opens_the_contexts_configuration_in_edit_configurations`, `…::the_tabs_menu_offers_modify_run_configuration_for_its_file`                                                                                                                                                                                                                       | —                                                                |
| action:run.chooseDebugConfig                         | done                                  | `…::debug_opens_the_chooser_in_debug_mode_and_enter_debugs_the_configuration`                                                                                                                                                                                                                                                                                                | —                                                                |
| action:debug.evaluate                                | partial                               | `workbench::debug_extras::tests::evaluate_expression_evaluates_the_selection_in_the_paused_frame_and_adds_it_to_the_watches`                                                                                                                                                                                                                                                 | Multi-line field, completion, expandable result                  |
| action:debug.smartStepInto                           | partial                               | `…::smart_step_into_steps_straight_in_when_the_debugger_lists_no_calls`, `…::smart_step_into_lists_the_calls_of_the_line_and_steps_into_the_chosen_one`                                                                                                                                                                                                                      | Dialog form, inline picking, the "direct step for one" spec case |
| ipc:debug_step_in_targets, ipc:debug_step_in_target  | done                                  | the Smart Step Into list-and-step test                                                                                                                                                                                                                                                                                                                                       | —                                                                |
| point.registerMenuGroup:run.context                  | partial                               | the three menu tests                                                                                                                                                                                                                                                                                                                                                         | Executor rows; Project view reads synchronously                  |
| action:view.zoomIn / zoomOut / resetZoom             | partial                               | adds `workbench::zoom_actions::tests::the_ide_zoom_scales_the_editors_code_and_the_icons`                                                                                                                                                                                                                                                                                    | Literal pixel sizes                                              |

**plugins.test.tsx counts** (the notes' table, unchanged by my work; all 13 of their tests now pass): 22 full, 10 in part, 18 page only, 9 model only or missing, 2 with no counterpart, out of 61.

## 4. Hypotheses

- The cause in A2 is not known (see Part A2 for the check to add).
- The java-debug failure is load-related (see Gates for the confirming command).
- The synchronous read for the Project view's Run rows never stalls noticeably. Confirm by timing `run_menu_rows` on a slow file system.
- Diff and merge viewers rebuild their code style when the zoom changes. Confirm by asserting `CodeStyle` in a viewer test after `view.zoomIn`.

## 5. Stale or false in notes and briefs

- `point.registerToolWindow:run`'s "no stripe badge" is now out of date.
- d108's decision that zoom stops at what GPUI scales is superseded: the editor, terminal and icons now follow the zoom.
- The pending test's signature `stripe_badge(&self, id)` cannot read the run store; it now takes `cx`.
- The notes' hypotheses that the `tool` configuration is named `tool` with an id starting `context:file:`, and that an `echo`-kind configuration runs and is listed, hold: the tests confirm them.
- `docs/gpui-port/notes/inspections/2026-11-14-c/run.sh` sends clicks through a helper and switches the frontmost application with osascript. I used d107's own-window-only variant instead.
- The draft fixture's `long` reimplemented `wait_long`; it now calls it.
