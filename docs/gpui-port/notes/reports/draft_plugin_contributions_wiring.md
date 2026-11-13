I've written the drafts for all six items, but nothing has been compiled or run, so every "works" below is a hypothesis until the integrator builds and runs the tests. A user can now cancel a plugin task from the Background Tasks popup. Plugin runs reach the Run tool window. Debug requests still can't be served: they fail with a notification, as below.

## 1. Files written

- `app-gpui/src/workbench/plugin_contributions.rs` (1,589 lines, 15 end-to-end tests).
- `app-gpui/src/workbench/background_tasks_view.rs` (371 lines, 3 view tests).
  - `BackgroundTasksPopup::new(tasks, corner, cx)`, plus `follow(source, read, cx)` to stay in sync with the workbench's tasks.
  - Event: `BackgroundTasksEvent::{Cancel(TaskId), Dismissed}`.
  - Accessors: `rows()` (title, percent, message, and the Cancel button as `Working` / `Disabled(reason)` / `Hidden`).
- `app-gpui/src/plugin_views/settings_page.rs` (1,119 lines, 9 tests).
  - Constructors: `PluginSettingsPage::fields(...)` and `::index(...)` (the parent page listing declared pages).
  - Event: `PluginSettingsEvent::RunAction{index, command, scope}`; answered with `action_finished`.
  - Accessors: `lines`, `field`, `is_invalid`, `action_states`, `value`; helpers `settings_pages`, `settings_page_id`, `declared_page_id`.
- Notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/plugin_contributions_integration.md`. Apply it after the runtime draft's notes. It has hunks for:
  - `Cargo.toml`: exactly `ide-extension = { path = "../crates/ide-extension" }`.
  - `plugin_views/mod.rs` and `plugin_runtime.rs` (four helpers made `pub(super)`; the `WindowRequest::Run` arm; `DocDirty`; the end of `plugin_tabs_changed`).
  - `mod.rs`, `menu.rs` (the `pluginTools` slot), `project_files.rs` (`with_menu_context` takes the contributed groups) and `settings_window.rs`.

## 2. Actions registered

- `view.backgroundTasks` toggles the Background Tasks popup, anchored at the window's bottom right. It stands for `plugin_cancel_task` / `cancel_task` and is tested by `cancelling_a_plugins_task_from_the_background_tasks_popup_stops_it`. It binds no shortcut, and nothing else here does either.
- Not registered:
  - `run.pluginStart`: requests go straight to the run driver.
  - `debug.pluginStart`: there is no debugger path a plugin session can be followed in.
- IPC rows by path:
  - Runs: `plugin_finish_operation`, `plugin_answer_command`.
  - Gutter: `plugin_language_visuals`, `lsp_execute_command`.
  - Settings page actions: settings writes, then `plugin_run_command` with `global_settings` set by the page's scope.
  - Events: `plugin_post_events`, `plugin_activation_event`.
- Every host call goes through `authorize_command` first. Activation events also return early in Restricted Mode, as the Tauri command does.

## 3. Spec cases covered

- `__tests__/settings.test.tsx`: 9 of 12. Not covered:
  - "does not run a command when applying fails": no apply failure can be provoked in a test.
  - "keeps Reset ownership": belongs to the staging model.
  - The project-scope half of "passes scope".
- `languageVisuals.test.ts`: 2 of 2, end to end.
- `workbench/tasks.test.tsx`: the 2 popup cases. The other cases are the tasks model's.
- `plugins.test.tsx`: "reports actions to action-performed plugins", using the Key Promoter plugin.
- End-to-end tests for items 1–6:
  - **Run:** a plugin run shows its output and exit code 3 in the Run tab, and the plugin hears `run-exited`.
  - **Debug:** the failure is visible and the plugin hears the run ended without a status.
  - **Run detectors:** they appear in the configuration chooser (proof that backend detection already works natively).
  - **Gutter:** the icon is on the right line, an edit clears it, a click runs the command, and two actions on one line open a menu.
  - **Menus:** Tools entries show their enabled state and run from the menu bar; the Project view entry appears once its `when` holds and runs.
  - **Settings:** a value is saved and the plugin hears `settings-changed`; a page action applies first, then the plugin reports reading the value.
  - **Events and tasks:** `file-saved`, `input-mode-changed`, the `on-language:`/`on-file:` activation events, and cancelling a task from the popup.

## 4. Not built, assumptions, likely failures

**Not built:**

- **Debug:** the debugger drops the events that would tell a plugin its session ended, and `debug.attach` and `debug.pluginStart` aren't registered. A debug request therefore ends the run without a status and shows "“name” can't be debugged" / "Debugger unavailable". The webview fails silently here; I added the notification so the failure is visible.
- **Editor context menu:** there isn't one natively, so the Bazel submenu has nowhere to go. The items are built and tested; the menu itself is not.
- **Other menu slots:** no native host exists for the editor tab, Problems and Search Results menus or the `editor.title`, `mainToolbar` and `commit.message` slots. Their item builders exist.
- **Gutter:**
  - Icons are drawn only for the focused editor, as a layer over the window, because the editor's single overlay slot is taken by code insight.
  - They lag a scroll by one frame.
  - The Bazel gutter's actual run session can't be tested: no fixture's gutter action starts a run.
- **`input-mode-changed`:** Vim isn't wired natively, so no user can reach it. I built the carrier and tested it directly.
- **Run Anything:** not registered natively, so plugin detectors can't appear there.
- **Settings:** multiline string settings use a one-line field.

**Assumptions to check first:**

- `IdeEvent::DocDirty` reaches the window carrying its label.
- A `framework = "plugin"` test run shows its raw output in `RunView::lines`.
- The detector accepts the `debugger` key.
- The host lists menu contributions in manifest order.
- A deferred, occluding gutter icon gets the mouse press before the editor (so no caret move and no breakpoint).
- `defer_in` runs after the dialog's Apply event has been handled.

**Most likely to fail on first run:**

- The run-detector chooser test: "Dynamic tests" needs the plugin activated during the listing.
- The activation-event test: it assumes a granted plugin stays `Inactive` until an activation event fires.
- The Key Promoter test: it waits seconds of real time for the event batch.
- The oversized-edit settings test, if `simulate_input` emits one change per character.
- The gutter tests: the icon only appears after the editor has painted once.
