# Integrating plugin contributions of a window (run requests, gutter actions, menus, settings pages, editor and activation events, Background Tasks)

Nothing here has been compiled or run. Apply **after** `plugin_runtime_integration.md` (several anchors are lines that note adds). Every hunk quotes the text it anchors on; each anchor was checked to be unique on 2026-11-13 in the files as committed (the `plugin_runtime.rs` anchors in the draft as committed). Match the quote, not a line number. Run `cargo fmt` after applying: about 40 lines of the new files are over 120 columns inside closures and rustfmt wraps them.

## 0. Files

- NEW `app-gpui/src/workbench/plugin_contributions.rs` (`PluginContributions` slot, `PluginMenu`, `SlotItem`, `plugin_tool_items`, `plugin_slot_items`, `plugin_menu_items`, `plugin_settings_pages`, `register_actions`, `impl Workbench`, 15 end-to-end tests).
- NEW `app-gpui/src/workbench/background_tasks_view.rs` (`BackgroundTasksPopup`, 3 view tests).
- NEW `app-gpui/src/plugin_views/settings_page.rs` (`PluginSettingsPage`, `SettingsDecl`, `settings_pages`, 2 model tests + 7 view tests).
- To change: `app-gpui/Cargo.toml`, `plugin_views/mod.rs` (draft), `workbench/plugin_runtime.rs` (draft), `workbench/mod.rs`, `workbench/menu.rs`, `workbench/project_files.rs`, `workbench/settings_window.rs`.
- No model file changes. Used as they are: `plugin_models/language_visuals.rs`, `marker_icons.rs`, `contribution.rs`, `run/store.rs` (`SessionStatus`), `vcs_ops/menu.rs`, `settings_model/staging.rs`, `settings_view/controls.rs` helpers, `tasks.rs`.

## 1. `app-gpui/Cargo.toml`

After `ide-docker = { path = "../crates/ide-docker" }` add:

```toml
ide-extension = { path = "../crates/ide-extension" }
```

(`ActivationEvent` for `PluginHost::activation_event`; `ide_host` uses it privately and does not re-export it. The crate is already in the graph through `ide-host`.)

## 2. `app-gpui/src/plugin_views/mod.rs` (draft)

After `pub mod prompt_dialog;` add:

```rust
pub mod settings_page;
```

and change its doc's first sentence to "Views of what plugins show: the questions a waiting command asks, the log tabs plugin code opens and their pages in Settings."

## 3. `app-gpui/src/workbench/plugin_runtime.rs` (draft)

### 3.1 Visibility (four helpers the sibling calls)

- Replace `fn plugin_host(cx: &App) -> Option<Arc<PluginHost>> {` with `pub(super) fn plugin_host(cx: &App) -> Option<Arc<PluginHost>> {`
- Replace `    fn plugin_call_allowed(&self, command: &str, cx: &App) -> Result<(), String> {` with `    pub(super) fn plugin_call_allowed(&self, command: &str, cx: &App) -> Result<(), String> {`
- Replace `    fn post_plugin_event(&mut self, event: WorkbenchEvent, cx: &mut Context<Self>) {` with `    pub(super) fn post_plugin_event(&mut self, event: WorkbenchEvent, cx: &mut Context<Self>) {`
- Replace `fn answer_plugin_command(label: &str, request_id: &str, answer: CommandAnswer, cx: &App) {` with `pub(super) fn answer_plugin_command(label: &str, request_id: &str, answer: CommandAnswer, cx: &App) {`

### 3.2 Run requests go to the Run tool window (in `fn plugin_runtime_event`)

Replace

```rust
            PluginEvent::WindowRequest { request_id, request, .. } => {
                self.refuse_window_request(request_id, request, cx);
            }
```

with

```rust
            PluginEvent::WindowRequest { request_id, request: WindowRequest::Run { operation, config, debug }, .. } => {
                self.plugin_run_request(request_id, operation, config, *debug, cx);
            }
            PluginEvent::WindowRequest { request_id, request, .. } => {
                self.refuse_window_request(request_id, request, cx);
            }
```

`refuse_window_request`'s `WindowRequest::Run` arm stays (it is still a complete match); its doc's "(runs, terminals, …)" may drop "runs". In the draft's module doc and in `plugin_runtime_integration.md` §8, `WindowRequest::Run` moves from "answered as a window without the service" to "built".

### 3.3 `file-saved` (in `fn plugin_backend_event`)

Before `            IdeEvent::TerminalExited { terminal, code } => self.plugin_terminal_exited(*terminal, *code, cx),` add:

```rust
            // The backend clears a document's dirty flag only when it saved it.
            IdeEvent::DocDirty { doc, dirty: false } => self.plugin_document_saved(*doc, cx),
```

### 3.4 Activation events and the gutter of the focused editor (end of `fn plugin_tabs_changed`)

Replace

```rust
        if let Some(report) = self.plugin_runtime.active_file.focused(active.as_deref()) {
            self.report_active_file(report, cx);
        }
    }
```

with

```rust
        if let Some(report) = self.plugin_runtime.active_file.focused(active.as_deref()) {
            self.report_active_file(report, cx);
        }
        self.plugin_activation_events(cx);
        self.plugin_gutter_shown(cx);
    }
```

## 4. `app-gpui/src/workbench/mod.rs`

### 4.1 Module lines

Before `mod bookmarks_window;` add `pub mod background_tasks_view;`. Before `mod plugin_runtime;` (added by the runtime note) add:

```rust
pub mod plugin_contributions;
```

(`pub`: its `pub` items — `plugin_menu_items`, `plugin_slot_items`, `PluginMenu`, `SlotItem`, `Workbench::plugin_input_mode_changed` — have no caller outside tests until the editor's context menu, the toolbar slots and Vim exist; exported, they are not dead code.)

### 4.2 Field

Replace

```rust
    plugin_runtime: plugin_runtime::PluginRuntime,
}
```

with

```rust
    plugin_runtime: plugin_runtime::PluginRuntime,
    /// Plugins' gutter actions, activation events and what else they contribute to the window.
    plugin_contributions: plugin_contributions::PluginContributions,
}
```

### 4.3 Initialiser (in `Workbench::new`)

Replace `            plugin_runtime: Default::default(),` with

```rust
            plugin_runtime: Default::default(),
            plugin_contributions: Default::default(),
```

### 4.4 Actions

After `    plugin_runtime::register_actions(registry);` add:

```rust
    plugin_contributions::register_actions(registry);
```

### 4.5 Every project editor's gutter is followed (in the file-opening task)

Replace

```rust
                        if still_open {
                            workbench.watch_debug_editor(&editor, window, cx);
                        }
```

with

```rust
                        if still_open {
                            workbench.watch_debug_editor(&editor, window, cx);
                            workbench.watch_plugin_gutter(&editor, window, cx);
                        }
```

### 4.6 The gutter icons (in `Render for Workbench`)

Before `            .child(notification_balloons::layer(&self.balloons, cx))` add:

```rust
            .children(self.plugin_gutter_layer(cx))
```

## 5. `app-gpui/src/workbench/menu.rs` (Tools › plugin entries)

In the `use super::menu_spec::{` list replace `DYNAMIC_STATUS_BAR_WIDGETS, DYNAMIC_TOOL_WINDOWS,` with `DYNAMIC_PLUGIN_TOOLS, DYNAMIC_STATUS_BAR_WIDGETS, DYNAMIC_TOOL_WINDOWS,`. Then replace

```rust
    let dynamic = HashMap::from([
        (DYNAMIC_TOOL_WINDOWS.to_owned(), tool_windows),
        (DYNAMIC_STATUS_BAR_WIDGETS.to_owned(), status_widgets),
    ]);
```

with

```rust
    let dynamic = HashMap::from([
        (DYNAMIC_TOOL_WINDOWS.to_owned(), tool_windows),
        (DYNAMIC_STATUS_BAR_WIDGETS.to_owned(), status_widgets),
        (DYNAMIC_PLUGIN_TOOLS.to_owned(), super::plugin_contributions::plugin_tool_items(cx)),
    ]);
```

The disabled set below already covers the dynamic items' actions, so each entry is greyed out while its command's `when` fails, as the webview's menu bridge does.

## 6. `app-gpui/src/workbench/project_files.rs` (Project view › plugin group)

### 6.1 `with_menu_context` takes the contributed groups

Replace

```rust
    fn with_menu_context<R>(&self, cx: &App, build: impl FnOnce(&MenuContext<'_>, &App) -> R) -> R {
```

with

```rust
    fn with_menu_context<R>(
        &self,
        contributed: Contributed,
        cx: &App,
        build: impl FnOnce(&MenuContext<'_>, &App) -> R,
    ) -> R {
```

and delete the line `        let contributed = Contributed::default();` in its body.

### 6.2 Its callers

Replace `        let items = self.with_menu_context(cx, |ctx, _| new_menu_items(ctx, Some(&folder), outside_tree));` with `        let items = self.with_menu_context(Contributed::default(), cx, |ctx, _| new_menu_items(ctx, Some(&folder), outside_tree));`.

In `tree_context_menu`, replace

```rust
        let items = cx.update_global::<Localization, _>(|localization, cx| {
            self.with_menu_context(cx, |ctx, _| {
```

with

```rust
        let selected = crate::project_view::context_menu::menu_group_context(&node_refs(&nodes));
        let project_view = self.plugin_project_view_items(&selected.paths, window, cx);
        let contributed = Contributed { project_view, ..Contributed::default() };
        let items = cx.update_global::<Localization, _>(|localization, cx| {
            self.with_menu_context(contributed, cx, |ctx, _| {
```

(`node_refs` is `crate::project_view::operations::node_refs`, already used in this function.)

## 7. `app-gpui/src/workbench/settings_window.rs` (a page per plugin)

These anchors are not touched by `plugins_welcome_integration.md` §6.

Replace

```rust
        let set = SettingsPageSet { specs: settings_registry(updates), pages: code_style_language_pages(&languages) };
```

with

```rust
        let mut pages = code_style_language_pages(&languages);
        pages.extend(super::plugin_contributions::plugin_settings_pages(cx));
        let set = SettingsPageSet { specs: settings_registry(updates), pages };
```

After `        let mut subscriptions = vec![cx.subscribe_in(&dialog, window, Self::settings_dialog_event)];` add:

```rust
        subscriptions.extend(self.mount_plugin_settings_pages(&dialog, has_project, window, cx));
```

## 8. Menu entries

Nothing to add to `menu_spec.rs`: Tools already ends with `Entry::Dynamic(DYNAMIC_PLUGIN_TOOLS)`. View › Background Tasks is not a menu item in the webview either (Find Action and the status item reach it).

## 9. Actions registered

| id                     | what                                                                                  | test                                                                 | IPC                                           |
| ---------------------- | ------------------------------------------------------------------------------------- | -------------------------------------------------------------------- | --------------------------------------------- |
| `view.backgroundTasks` | opens the Background Tasks popup over the status bar's right end, closes it when open | `cancelling_a_plugins_task_from_the_background_tasks_popup_stops_it` | `plugin_cancel_task` / `cancel_task` (Cancel) |

No shortcut is bound (the webview binds none for it). Plugin commands stay registered by `plugins.rs`. `run.pluginStart` / `debug.pluginStart` are not registered: the webview's actions exist only to carry a callback between two features; here the run request goes straight to the run driver (§3.2).

## 10. IPC commands each path stands for

- Run requests: `plugin_finish_operation` (a run that never started, a debug request); the backend's `run_operation_sink` reports the exit of a started one (`crates/ide-host/src/plugins/execution.rs`), as `run_start`/`ops::run::start` do for the webview; `plugin_answer_command` when the request has an id.
- Gutter: `plugin_language_visuals` (`ops::lsp::plugin_language_visuals`), `lsp_execute_command` (`openidea.plugin-language:<plugin>:<command>`).
- Menus: none (projection of `plugin_list` contributions; the entry runs `plugin_run_command` through the plugin's action).
- Settings: the dialog's settings writes (`settings_set`), then `plugin_run_command` with `settings_scope` (`Invocation.global_settings`).
- Events: `plugin_post_events` (`file-saved`, `input-mode-changed`), `plugin_activation_event`.
- Background Tasks: `plugin_cancel_task` / `cancel_task` through the draft's `cancel_background_task`.

Every call passes `Host::authorize_command(label, "<ipc name>")` first (the draft's `plugin_call_allowed`), and activation events also return early for a project in Restricted Mode, as `plugin_activation_event` does in `src-tauri/src/commands/plugin.rs:276-298`.

## 11. Not built, with why

- **Debugging a plugin's configuration.** `debug.attach` and `debug.pluginStart` are not in `debug.rs`'s `DEBUG_ACTIONS`, `run/java_debug.rs`'s `AgentDebug` has no caller, and `DebugDriver::pump` drops the debugger model's `PluginEvent`s ("no plugin starts sessions in this window"), so a plugin could not hear its session end. A debug request therefore finishes the operation without a status and raises "“<name>” can't be debugged" / "Debugger unavailable". The webview's catch is silent (`runtimeUi.tsx:689-692`); the notification is added so the failure is visible. To build: the debug driver hands out `take_plugin_events` per session, then route `debug` requests to `debug.startConfig` (JDWP via `jdwp_launch`, which already handles `kind == "plugin"` with `debugArgs`) or `debug.pluginStart`.
- **The editor's context menu** does not exist natively (no right-button handler in `editor/`), so `editor.context` entries (the Bazel submenu) have no host. `plugin_menu_items(PluginMenu::Editor, …)` builds them; the test asserts the Bazel-shaped submenu.
- **Editor tab, Problems and Search Results context menus, and the `editor.title`, `mainToolbar`, `commit.message` slots**: no native slot takes plugin items yet. `plugin_menu_items` (`EditorTab`, `Problems`, `SearchResults`) and `plugin_slot_items` give them with their `when`. `tab_menu` could append them before "Configure Editor Tabs…".
- **Gutter icons in split panes**: only the focused editor's icons are drawn (the layer reads the editor's last painted layout, and a background tab's layout is stale). Icons are drawn by the workbench over the editor (the editor's one overlay slot is code insight's, `editor/intelligence.rs:123`), so they lag one frame behind a scroll and sit over any floating tool window above the editor.
- **A gutter action's failure** is only logged, as the webview's `console.warn` (`languageVisuals.ts:84`).
- **Run Anything** is not registered natively (`runAnything.show` has no handler), so detectors cannot appear there. The gutter's run producers do not list plugin detectors in the webview either.
- **Multiline string settings** use the one-line field (the webview draws a textarea).
- **A settings page opened before a plugin changes** keeps the old schema until the dialog is opened again.
- **`file-saved` of a clean document**: the webview posts it on every save; the backend emits `DocDirty { dirty: false }` only when a dirty document was written.
- **`input-mode-changed`** has no native producer: Vim is not wired (`VimModeState` is used only in `status_items.rs`). `Workbench::plugin_input_mode_changed` is the carrier its producer calls; the test calls it directly.

## 12. Assumptions to check first

- `IdeEvent::DocDirty` reaches `Backend::listen` with the window's label (or none).
- `RunDriver::start` of a `framework = "plugin"` test configuration shows the program's raw output in `RunView::lines` (no plugin test parser is wired natively; `run/store.rs:1893` skips the launch finalizer for it).
- The execution fixture's detector accepts `debugger = "java-debug"` (the Bazel manifest uses that key).
- The host lists a manifest's `[[contributes.menus]]` in declaration order (the menu tests assert it).
- A deferred, occluding icon over the editor's gutter gets the press before the editor's own mouse-down (so no caret move and no breakpoint toggle).
- `cx.defer_in` after `SettingsDialogView::apply` runs after the dialog's `Apply` event was handled (effects are flushed in order).
- `Window::viewport_size` is the window's content size in the test platform.

## 13. Tests most likely to fail first

- `a_plugins_run_detectors_are_listed_in_the_configuration_chooser`: "Dynamic tests" comes from the plugin's `runConfigurations` export, which needs the plugin activated by `refresh_run_configurations`.
- `focusing_an_editor_fires_the_language_and_file_activation_events`: assumes a linked, granted plugin stays `Inactive` until an activation event; the language id of `.txt` must be `plaintext`.
- `reports_actions_to_action_performed_plugins`: needs `navigate.gotoFile` to have a shortcut label and Key Promoter's first tip on its first count; events wait the idle batch (seconds of real time).
- `the_tools_menu_lists_plugin_entries_with_their_state_and_runs_the_chosen_one`: `MenuCommand` dispatch needs the test window active; Insert Date needs `editor.write` (required, granted by default consent).
- `renders_owned_gutter_commands_and_drops_answers_to_superseded_buffers` / `puts_the_actions_of_one_line_…`: the icon is drawn only after the editor painted once (the layer reads `position_of`); the click goes through `debug_bounds`.
- `a_plugins_settings_page_saves_its_value_and_the_plugin_hears_the_change`: depends on `settings-changed` carrying `["enabled"]` for a `plugins` write (the draft's own test asserts the same).
- `a_saved_file_reaches_the_plugin_that_watches_its_saves`: `subscribe_paths` globs are project-relative; the count must not include an `editor-focused` event.
