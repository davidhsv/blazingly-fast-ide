# Integrating the plugin runtime of a window (status items, tasks, notifications, context keys, questions, log tabs, workbench events)

Nothing here has been compiled or run. Every hunk quotes the line it anchors on; each anchor was checked to be unique on 2026-11-13. Match the quote, not a line number (`workbench/mod.rs` moves).

## 0. Files

- NEW `app-gpui/src/plugin_views/mod.rs`, `plugin_views/prompt_dialog.rs` (`PluginPrompt`, 7 tests), `plugin_views/log_tab.rs` (`PluginLogTab`, 6 tests).
- NEW `app-gpui/src/workbench/plugin_runtime.rs` (`PluginRuntime` slot, `impl Workbench`, `register_actions`, `build_plugin_log_tab`, 11 end-to-end tests).
- To change (compiled): `lib.rs`, `workbench/mod.rs`, `workbench/chrome.rs`, `workbench/tab_views.rs`.
- No model file is changed. `plugin_models/runtime_ui.rs`, `workspace_contains.rs`, `active_file.rs`, `store.rs` (`command_title`, `command_failure`) are used as they are.

Order relative to `plugins_welcome_integration.md`: either order works. No hunk below touches a line that note's hunks replace (its §3.3 replaces the multi-line tail of `Workbench::new` and the `if cx.has_global::<Backend>()` block of `on_release`; the anchors below are other lines).

## 1. `app-gpui/src/lib.rs`

After `pub mod plugin_models;` add:

```rust
pub mod plugin_views;
```

## 2. `app-gpui/src/workbench/mod.rs`

### 2.1 Module line

Before `mod project_files;` add:

```rust
mod plugin_runtime;
```

### 2.2 The field

Replace

```rust
    chrome: chrome::Chrome,
}
```

(the end of `pub struct Workbench`) with

```rust
    chrome: chrome::Chrome,
    /// What running plugin code shows in the window, and what the window tells the plugins.
    plugin_runtime: plugin_runtime::PluginRuntime,
}
```

### 2.3 Its initialiser and start (in `Workbench::new`)

Replace

```rust
            welcome: None,
        };
```

with

```rust
            welcome: None,
            plugin_runtime: Default::default(),
        };
        workbench.start_plugin_runtime(window, cx);
```

### 2.4 Its end (in the `cx.on_release` of `Workbench::new`)

Before `            input::window_blurred(id, cx);` add:

```rust
            workbench.stop_plugin_runtime(cx);
```

### 2.5 Actions

After `    tab_views::register_actions(registry);` (in `register_actions`) add:

```rust
    plugin_runtime::register_actions(registry);
```

### 2.6 Tabs changed (in `fn tabs_changed`)

After `        self.note_active_tab(cx);` add:

```rust
        self.plugin_tabs_changed(cx);
```

### 2.7 Notification buttons (in `fn run_notification_action`)

Before `            EntryAction::Own(own) if own.command.starts_with("agents.openTask:") => {` add:

```rust
            // A plugin's notification: its button runs the publishing plugin's command.
            EntryAction::Own(own)
                if own.command.starts_with(crate::plugin_models::runtime_ui::NOTIFICATION_COMMAND_PREFIX) =>
            {
                self.run_plugin_notification_button(&own.command, cx);
            }
```

### 2.8 The questions' layer (in `Render for Workbench`)

After `            .children(self.vcs_dialog_layer())` add:

```rust
            .children(self.plugin_prompt_layer())
```

## 3. `app-gpui/src/workbench/chrome.rs` (in `fn status_bar_event`)

Before `            StatusBarEvent::RunAction { id, arg, .. } => run_later(id, arg, window, cx),` add:

```rust
            // A plugin's item runs its plugin's command, declared or not, as the plugin set it.
            StatusBarEvent::RunAction { item, .. } if item.starts_with(super::plugin_runtime::PLUGIN_STATUS_PREFIX) => {
                self.plugin_status_item_clicked(&item, cx)
            }
```

## 4. `app-gpui/src/workbench/tab_views.rs` (`TAB_KINDS`)

After the `BINARY_KIND` entry (its last line is `        build: build_hex,` then `    },`), add a third entry:

```rust
    TabKind {
        id: crate::plugin_views::log_tab::PLUGIN_LOG_KIND,
        restorable: false,
        active_key: None,
        focus_key: None,
        title: crate::plugin_views::log_tab::log_tab_title,
        icon: super::plugin_runtime::log_tab_icon,
        build: super::plugin_runtime::build_plugin_log_tab,
    },
```

`every_kind_is_registered_once` (tab_views tests) keeps holding.

## 5. Menus

None. Plugin menus (`pluginTools`, editor and project view context menus) are the later draft.

## 6. Actions registered (all run by an end-to-end test)

| id                           | what                                                                 | test                                                                         | IPC                    |
| ---------------------------- | -------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------- |
| `plugins.log.clear`          | empties a log tab (arg: tab key, else the active tab)                | `starts_a_tab_opened_by_plugin_code_by_its_key_with_the_toggles_that_are_on` | none                   |
| `plugins.log.wrap`           | soft wrap on or off in a log tab (same addressing)                   | same                                                                         | none                   |
| `plugins.log.toggleArgument` | `{target?, id, checked}`: restarts the tab's program with the toggle | same                                                                         | `plugin_log_tab_spawn` |

`plugins.log.clear` and `plugins.log.wrap` are not in `assets/gen/actions.json` (the webview registers them from a template), so they are registered with `registry.register(Action::new(meta, …))`, hidden, category Plugins. `plugins.log.find` is not registered (Find in Logs is not built). Plugin commands stay registered by `plugins.rs`; this draft only makes their `workspaceContains` conditions hold.

## 7. IPC commands each path stands for (the parity rows)

`plugin_subscribe` (the `Plugins::listen` per window), `plugin_status_items` (`status_snapshot`), `plugin_context_keys`, `plugin_event_kinds`, `plugin_post_events` (`post_window_events`), `plugin_set_active_file`, `plugin_cancel_task`, `plugin_answer_prompt`, `plugin_answer_command`, `plugin_answer_commit`, `plugin_run_command` (notification buttons, status item clicks, the host's command requests), `plugin_log_tab_spawn` (`spawn_log_tab_pty`), `plugin_finish_operation`, `fs_read_dir` (`ops::fs::read_dir` for `workspaceContains`), `cancel_task` (`ide_core::task::cancel`, for backend tasks). Window lifecycle: `reopen_client` at window creation, `forget_client` at release (Tauri does both in `windows.rs:592` / `lib.rs:246`).

Every call is first passed through `Host::authorize_command(label, "<ipc name>")`, as Tauri's invoke handler does (`src-tauri/src/lib.rs:170`, `crates/ide-host/src/trust.rs:429`): without an open, trusted project only `plugin_set_active_file` (and the other restricted-allowed commands) go through. That is why a window without a trusted project lists no plugin status items and asks no event kinds, exactly as in the webview.

## 8. What each runtime event kind does (complete list of `PluginEvent`)

Built: `StatusItemChanged`, `WindowStatusItemChanged` (status bar; declared `[[contributes.statusBar]]` items shown until code sets its own), `TaskStarted`/`TaskProgress`/`TaskEnded` (task list, Cancel through `cancel_background_task`), `ContextChanged` (refetch, `set_context`), `Notification` (buttons run the plugin command), `CopyToClipboard`, `OpenUrl` (http/https only, the model's check), `ExecuteCommand` (action or plugin command, answered), `EditorOpen` and `OpenLocation` (caret at the position), `EditorRanges` (selections / scroll in open editors), `OpenProject` (`file.openProjectPath` / `file.openInNewWindow`), `PromptRequested`/`PromptClosed`, `OpenLogTab`, `OutputChanged`/`RevealOutput` (an output tab), `SubscriptionsChanged` and `CommandsChanged` (event kinds asked again; the listing is `Plugins::pump`'s), `UpdatesAvailable` (notification with Update All / Show Plugins, commands `plugins.updateAll` / `plugins.settings`, run when registered).

Answered as a window without the service answers (so no plugin call hangs): `WindowRequest::Run` (`finish_operation(op, None, false)`, then `Failed("Run service unavailable")` when it has a request id), `WindowRequest::Terminal` / `DatabaseConnect` / `ContentTab` (`Failed(...)`, the webview's texts), `CommitRequested` (`CommitAnswer::NoRepository`: the native Commit view does not hand out its selection yet).

Logged and not shown, with why: `OpenTerminal` (no native `terminal.runCommand` action yet), `DecorationsChanged`, `ViewChanged`, `ViewMessage`, `RevealView`, `ContentTabReply` (plugin views, web views and content tabs: later draft), `RunTargetsChanged` (run targets: later draft), `DocumentIndexesChanged`, `EndpointsChanged` (navigation, endpoints: later draft), `ConsentRequested` (handled by `plugins_window.rs`, `plugin_host_event`), `ThemeChanged` and `LanguagesChanged` (already handled by `Plugins::pump` and the sink in `plugins.rs`).

Window events posted (500 ms batches, only kinds in `event_kinds`): `editor-focused` (through `set_active_file`, deduplicated, reported again when the project opens), `file-opened`, `file-closed` (from the file tabs), `vcs-changed` (backend `VcsChanged`), `settings-changed` (backend `SettingsChanged`, and once when the kinds include it), `action-performed` (registry `on_action_executed`, deferred). Not posted: `file-saved` (no single save-completed point in the workbench; `Editor::save` callers are many) and `input-mode-changed` (Vim mode changes are not events natively). `files-changed` is the backend's own (`project.rs`). Activation events `on-language:` / `on-file:` (bridge.ts `trackActivationEvents`) are not sent: `ActivationEvent` is in `ide-extension`, which `app-gpui` does not depend on (add `ide-extension = { path = "../crates/ide-extension" }`, already in the graph, and call `host.activation_event` from `plugin_tabs_changed`).

## 9. Safety

- Grants: the native side calls only `PluginHost`'s public API; every host function checks the plugin's grant itself (log tab spawn: `pty_options` checks `process:<program>`; clipboard, `project.read`, `events` subscriptions are checked by the host). The window adds the per-window trust gate above. Nothing here grants anything.
- Secrets: no code here reads, paints or logs a secret. `PluginRuntime` has no `Debug`. The keychain (`KeychainSecrets`, set by `Plugins::init` on macOS) is reached only by `set_secret`, `secrets`, `uninstall` of a plugin that declares secrets, a plugin's `secret_*` calls and `fetch` with `{secret:…}` headers. The tests replace it right after `Plugins::init` with `FileSecretStore { dir: <temp home>/secrets }` and run no secret command.
- Folders: every test uses `AppDirs::under(tempdir)`, links a copy of each fixture made in the temp home (the repository is only read), and points `set_program_path` at a temp `bin` (or `/bin` for `bash`). Nothing writes under `~/Library/Application Support/ai.l0ng.openidea*`.

## 10. Not built, with why

- Find in Logs (`plugins.log.find`, the log tab's search button): the native find bar works on `Editor` entities only.
- Plugin text catalogs (`usePluginT`) for running status item texts: shown as the plugin wrote them.
- A log tab's soft wrap starts on; the webview also reads `editor.softWrap` and the soft-wrap file patterns for `stream.log`.
- A log tab whose input has no program closes (tab_views closes tabs it cannot build) instead of showing "Invalid log stream configuration". The webview's UI-side `logTabData` validation is left to the host's `pty_options`, which refuses the same programs.
- The folder question (`pickDirectory`) opens the system dialog without its title or initial folder (`PathPromptOptions` has neither).
- `EditorRanges` with `reveal` scrolls the first range to the top; the webview centres it.
- **Cancel is not reachable by a user yet.** The tasks status item's click runs `view.backgroundTasks` (`status_items.rs:193`), which nothing registers natively, so `reachable()` drops the click; there is no Background Tasks popup. `Workbench::cancel_background_task(&TaskId, cx)` is the carrier its Cancel button needs; the task test calls it directly.
- `file-saved` is not posted (see §8); `checkstyle` and `license-header` activate on it, so it is the first gap to close.
- Pick lists have no filter field: the webview's `PickDialog` has none either (the assignment's "filtering" is not webview behaviour).

## 11. Assumptions to check first

- `IdeEvent::FsChanged`, `VcsChanged`, `SettingsChanged`, `TerminalExited` reach `Backend::listen` with `window` set to the window's label (or `None`).
- `observe_in(&cx.entity(), …)` on the workbench itself is allowed by GPUI and does not loop (the handler notifies only when the project changed).
- `TextField` emits `Submitted` on Enter when focused through `window.focus` in tests; `TextArea::submit` emits `Submitted`.
- The settings key path `plugins.api14.enabled` is accepted by `platform::settings::set` (staging supports `plugins` map entries per the assessment).
- `host.set_enabled` may be called on the main thread in a test (it withdraws prompts first).

## 12. Tests most likely to fail first

- `a_changed_plugin_setting_reaches_the_plugin_as_settings_changed`: depends on the settings key path and on `changed_settings` diffing the plugin's settings for the window.
- `the_focused_editor_and_a_checkout_reach_the_plugin_which_shows_the_branch`: the plugin is idle, so events wait `IDLE_BATCH` (2 s real time) before activation; the watcher must report the checkout as `VcsChanged` for this window.
- `a_plugins_task_is_listed_while_it_runs_and_cancel_stops_it`: needs the fake `checkstyle` approved at grant time and the task event addressed to `w1`.
- `starts_a_tab_opened_by_plugin_code_by_its_key_with_the_toggles_that_are_on`: a real PTY running `bash`; output arrives with `\r\n`, colour codes are stripped.
- `re_evaluates_when_the_file_is_created_and_deleted`: needs the watcher's `FsChanged` for the project root.
- Every test needs §2.3 (the runtime starts in `Workbench::new`) and the `Plugins` global initialised before the window.
