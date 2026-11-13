I wrote three view and module files, the workbench child module and the integration notes. Nothing has been compiled or run, so every claim about behaviour below is a hypothesis until the integrator builds and runs the tests. One finding matters most: **a user cannot cancel a task yet.** The tasks status item's click runs `view.backgroundTasks`, which nothing registers natively, so the click is dropped. `cancel_background_task` is the code a future Cancel button needs; my test calls it directly.

## 1. Files written

- `app-gpui/src/plugin_views/mod.rs` (5 lines), a new folder; add `pub mod plugin_views;` to `lib.rs`.
- `app-gpui/src/plugin_views/prompt_dialog.rs` (740 lines, 7 tests). `PluginPrompt` handles text input (one line or several), pick-one, pick-many and yes/no.
  - Constructors: `input(prompt, value, multiline, window, cx)`, `pick(title, items, multiple, cx)`, `confirm_question(title, message, cx)`.
  - Event: `PluginPromptEvent::Answered(Option<Vec<String>>)`.
  - Accessors: `title`, `text`, `is_multiline`, `field`, `area`, `rows`, `active`, `checked`, `message`, `can_confirm`, `confirm`, `dismiss`.
- `app-gpui/src/plugin_views/log_tab.rs` (608 lines, 6 tests). `PluginLogTab::new(data, wrap, cx)`.
  - Events: `PluginLogEvent::{Start{on}, Stop}`.
  - Methods: `start`, `append`, `exited`, `failed`, `stop`, `set_output`, `clear`, `toggle_wrap`, `set_toggle`.
  - Accessors: `lines`, `text`, `toggles`, `wraps`, `is_running`, `is_stopped`, `data`.
  - Helpers: `code_log_input`, `output_input`, `log_tab_title`, `bounded_log`, `clean_output`.
- `app-gpui/src/workbench/plugin_runtime.rs` (2,024 lines, 11 end-to-end tests): the `PluginRuntime` field, `impl Workbench`, `register_actions` and `build_plugin_log_tab`.
- Notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/plugin_runtime_integration.md`. They hold the hunks for `lib.rs`, `mod.rs` (8 hunks), `chrome.rs` (status item click) and `tab_views.rs` (the `pluginLog` tab kind). None of these anchors overlaps a line that `plugins_welcome_integration.md` replaces.

## 2. Actions registered

All three are tested by `starts_a_tab_opened_by_plugin_code_by_its_key_with_the_toggles_that_are_on`.

| Action                       | What it does                               | IPC command            |
| ---------------------------- | ------------------------------------------ | ---------------------- |
| `plugins.log.clear`          | empties the log tab                        | none                   |
| `plugins.log.wrap`           | turns soft wrap on or off                  | none                   |
| `plugins.log.toggleArgument` | restarts the tab's program with the toggle | `plugin_log_tab_spawn` |

- `plugins.log.find` is not registered because Find in Logs is not built.
- Plugin commands stay registered by `plugins.rs`; their `workspaceContains` conditions now hold.
- Every host call first passes `Host::authorize_command(label, "<ipc name>")`, the same gate Tauri applies, so nothing works in an untrusted project that the webview would refuse.
- Each path's IPC command is listed in notes §7.

## 3. Spec cases covered

- `workspaceContains.test.ts`: 12/12. Eleven are already covered by the model's tests; I added an end-to-end test for "re-evaluates when the file is created and deleted". "Stops listening when disposed" is built (`stop_plugin_runtime`) but not tested.
- `plugins.test.tsx` plugin prompts: 4/4.
- `plugins.test.tsx` bridge events (runtime cases): 5/5. Two are end-to-end (the window status override and notification buttons); the rest are covered by the model.
- Plugin context keys: 4/4, model only.
- `logEditor.test.tsx`: 4 of 7.
  - "Renders partial output…" is partial: Find is not built, and my test name drops "and finds".
  - Not tested: the UI-side validation of malformed requests (the host refuses the same programs), killing a spawn that resolves after its tab closes (built), and the transient tab that stays mounted.
- "Reports actions to action-performed plugins" is built but has no test.

## 4. Not built, assumptions, likely failures

**Not built:**

- `file-saved` events are not posted; `checkstyle` and `license-header` activate on them, so this is the first gap to close.
- `input-mode-changed` events are not posted.
- `on-language:` and `on-file:` activation events are not sent, because `ActivationEvent` lives in `ide-extension`, which `app-gpui` does not depend on.
- Find in Logs.
- Plugin text catalogs for status item text (shown as the plugin wrote it).
- The folder dialog's title and starting folder.
- `OpenTerminal`, views and web views, decorations, run targets, endpoints. The run, terminal, database and content-tab requests, and commit requests, are answered as a window without those services would answer, so no plugin call hangs.
- Pick lists have no filter field: the webview's pick dialog has none, so the "filtering" in your list is not webview behaviour.

**Driven through the host, not the registry:** `jobs`' input, pick, copy and open-url commands are undeclared, and its file open happens only from a view button. Those tests call `host.run_command` or `view_event` with the window's label.

**Keychain:** `Plugins::init` installs the macOS keychain store. It is reached only by `set_secret`, `secrets`, uninstalling a plugin that declares secrets, a plugin's own secret calls, and `fetch` with secret headers. The tests swap in `FileSecretStore` under the temp home right after init and run none of those. All fixtures are copied into the temp home before linking; nothing writes under the installed app's folders.

**Assumptions to check first:**

- Backend events (`FsChanged`, `VcsChanged`, `SettingsChanged`, `TerminalExited`) carry the window's label.
- Observing the workbench itself with `observe_in` is allowed and does not loop.
- `platform::settings::set` accepts the key `plugins.api14.enabled`.

**Most likely to fail on first run:**

- The api14 settings-changed test: it depends on that settings key.
- The git-toolbox branch test: the plugin is idle, so events wait 2 s of real time, and the checkout must arrive as `VcsChanged`.
- The checkstyle cancel test: the fake sleeping `checkstyle` must be approved when the grant is made.
- The real-PTY log tab test.
