# Integrating the application actions into the workbench

New files, not compiled yet:

- `app-gpui/src/workbench/app_actions.rs`: the `impl Workbench` blocks, the registration, the globals (saved layouts, Distraction Free Mode, waiting trust questions, the `Relaunch` seam) and the end-to-end tests.
- `app-gpui/src/app_views/mod.rs`: shared dialog pieces (frame, title row, footer, button, checkbox, field frame, path line, `tx`, `t`).
- `app-gpui/src/app_views/trust_dialog.rs`, `about_dialog.rs`, `new_project_dialog.rs`, `invalidate_caches_dialog.rs`: the four dialogs, each with its own `#[gpui_kit::test]`s.

No existing file was edited. Every anchor below is quoted from the file as it is now. Line numbers are left out because `mod.rs` is moving. Sections 1 to 9 are all required: `-D warnings` fails on dead code without 5, 7 and 8, and the tests fail without 4, 6 and 9. Some pasted lines run past 120 columns, so run `cargo fmt` after pasting.

## 1. `app-gpui/src/lib.rs`

After `pub mod app_models;` add:

```rust
pub mod app_views;
```

## 2. `app-gpui/src/platform/storage.rs`

Export Settings… lists the stored entries, and `UiStorage` cannot list them yet. After

```rust
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
```

add:

```rust

    /// Every stored entry, in key order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values.iter().map(|(key, value)| (key.as_str(), value.as_str()))
    }
```

## 3. `app-gpui/src/workbench/mod.rs`

### 3a. The module

Replace

```rust
mod agents;
mod app_shell;
```

with

```rust
mod agents;
mod app_actions;
mod app_shell;
```

### 3b. The actions

In `pub fn register_actions(cx: &mut App)`, after `settings_window::register_actions(registry);` add:

```rust
    app_actions::register_actions(registry);
```

### 3c. The context keys

In `fn publish_context(&mut self, window: &mut Window, cx: &mut App)`, after `self.publish_merge_context(window, cx);` add:

```rust
        self.publish_app_context(window, cx);
```

This publishes `toolWindowLayoutsSaved` (the `when` of `layout.rename` / `layout.delete`) and `distractionFree`.

### 3d. The Restricted Mode banner redraws when the trust changes

Replace

```rust
            IdeEvent::ProjectTrustChanged { trusted, .. } => {
                self.agents_trust_changed(*trusted, cx);
                return;
            }
```

with

```rust
            IdeEvent::ProjectTrustChanged { trusted, .. } => {
                self.agents_trust_changed(*trusted, cx);
                // The Restricted Mode banner comes or goes.
                cx.notify();
                return;
            }
```

### 3e. Distraction Free Mode hides the status bar

Replace

```rust
        (self.editor().is_some() || self.task_status().is_some())
            && crate::platform::settings::global(cx).appearance.show_status_bar
```

with

```rust
        (self.editor().is_some() || self.task_status().is_some())
            && crate::platform::settings::global(cx).appearance.show_status_bar
            && !self.distraction_free(cx)
```

### 3f. Distraction Free Mode hides the stripes

In `fn render`, replace

```rust
                let bars = crate::platform::settings::global(cx).appearance.show_tool_window_bars;
```

with

```rust
                let bars = self.shows_tool_window_bars(cx);
```

Keep the comment line above it (`// "Tool Window Bars" off hides the stripes; …`) and extend it with: `Distraction Free Mode hides them too.`

### 3g. The banner over a project in Restricted Mode

In `fn render`, replace

```rust
            .child(self.title_bar().clone())
            .child(div().flex_1().min_h_0().w_full().child(body))
```

with

```rust
            .child(self.title_bar().clone())
            .children(self.restricted_banner(cx))
            .child(div().flex_1().min_h_0().w_full().child(body))
```

### 3h. A project opened in Distraction Free Mode hides its tool windows

In `pub fn open_project`, after

```rust
                        layout.load(&scope, stored.as_deref());
```

add:

```rust
                        // In Distraction Free Mode the project's tool windows are hidden like the others'.
                        if app_actions::zen_layout_loaded(&mut layout, cx) {
                            workbench.show_distraction_free_hint(cx);
                        }
```

### 3i. A test that now meets the trust question

`projects_open_in_this_window_a_new_one_or_the_one_that_has_them` opens three undecided folders through `file.openProjectPath`. With section 5b each opening first asks "Trust and Open Project?", as the webview does, so the test would wait on the dialog. After

```rust
        for root in &roots {
            std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        }
```

add:

```rust
        // Decided folders open without the trust question.
        for root in &roots {
            cx.update(|cx| cx.global::<Backend>().host.trust.decide(root, true, false)).expect("trusted");
        }
```

## 4. `app-gpui/src/workbench/chrome.rs`: Distraction Free Mode makes the title bar compact

In `fn sync_title_bar`, replace

```rust
                (TitleBarKind::Project, title_bar_mode(false, show_main_toolbar), None, Some(project))
```

with

```rust
                (TitleBarKind::Project, title_bar_mode(self.distraction_free(cx), show_main_toolbar), None, Some(project))
```

## 5. `app-gpui/src/workbench/app_shell.rs`

### 5a. Restart IDE quits the way Quit does, then relaunches

Replace

```rust
pub(super) fn quit(window: &mut Window, cx: &mut App) {
    if quitting(cx) {
        return;
    }
```

with

```rust
pub(super) fn quit(window: &mut Window, cx: &mut App) {
    quit_then(window, cx, |cx| cx.quit());
}

/// [`quit`], ending with `end` once every window agreed to close: quitting, or quitting and
/// starting again (Restart IDE).
pub(super) fn quit_then(window: &mut Window, cx: &mut App, end: fn(&mut App)) {
    if quitting(cx) {
        return;
    }
```

and, at the end of that function, replace

```rust
        cx.update(|cx| cx.quit());
```

with

```rust
        cx.update(end);
```

### 5b. "Trust and Open Project?" before an undecided project opens

In `async fn carry_out_open`, replace

```rust
            OpenStep::RequestWindow { path, new_window, files } => {
                let outcome = cx.update(|window, cx| request_window(&path, new_window, files, window, cx))?;
```

with

```rust
            OpenStep::RequestWindow { path, new_window, files } => {
                // A folder nobody decided about is asked about first; Don't Open ends the opening.
                if !super::app_actions::confirm_project_trust(this, cx, Path::new(&path)).await? {
                    return Ok(());
                }
                let outcome = cx.update(|window, cx| request_window(&path, new_window, files, window, cx))?;
```

The question is asked before a window is chosen, so it also covers a project that opens in a new window. Nothing has been prepared for a close at that step, so returning leaves nothing to release. Assumption: every opening by the user passes `RequestWindow` (Open, Open Recent, the Welcome screen, Open in New Window, New Project). The command-line folder in `main.rs` (`Target::Project`) does not go through this flow and opens without the question.

## 6. `app-gpui/src/workbench/menu.rs`: Window › Tool Window Layouts lists the saved layouts

Replace

```rust
    DYNAMIC_STATUS_BAR_WIDGETS, DYNAMIC_TOOL_WINDOWS, DynamicItem, Entry, Native, OPEN_RECENT_ACTION, Os, RecentItem,
```

with

```rust
    DYNAMIC_LAYOUTS, DYNAMIC_STATUS_BAR_WIDGETS, DYNAMIC_TOOL_WINDOWS, DynamicItem, Entry, Native, OPEN_RECENT_ACTION,
    Os, RecentItem,
```

and replace

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
        (DYNAMIC_LAYOUTS.to_owned(), super::app_actions::layout_menu_items(cx)),
    ]);
```

Each item runs `layout.restore` with the layout's name as its argument, checked for the active layout. `menu_spec.rs` already has every other entry: About, New Project…, Invalidate Caches…, Export / Import Settings…, Power Save Mode, View › Appearance › Enter Distraction Free Mode, and Window › Tool Window Layouts › Save / Restore Default / Rename / Delete. No context menu entry is needed.

## 7. Menu label of Distraction Free Mode

The registry's title of `editor.toggleZen` flips between Enter and Exit, so Find Action and the keymap show the current one. The static label in `menu_spec.rs` (`act("editor.toggleZen", "Enter Distraction Free Mode")`) stays "Enter". Not wired: `menu_spec::Entry` labels are fixed strings.

## 8. Required by dead-code analysis

`confirm_project_trust` is used only by 5b outside the tests. `layout_menu_items` is used only by 6. `shows_tool_window_bars`, `distraction_free`, `restricted_banner`, `zen_layout_loaded`, `show_distraction_free_hint` and `publish_app_context` are used only by 3 and 4.

## 9. IPC commands each action stands for

- `project.trust`, `project.previewRestricted`: `project_trust_set(path, trusted, folder)` → `Host::set_project_trust`. The question: `project_trust_status(path)` → `TrustStore::status`.
- `project.dontOpen`: none (answers the question).
- `project.revokeTrust`: `project_trust_set(path, false, false)`.
- `project.removeTrustedLocation`: `project_trust_remove_location(path)` → `TrustStore::remove_location` + `Host::refresh_project_trust`. The desktop shell also calls `plugins.restrict_project` for every untrusted project, and the native app has no equivalent of that plugin manager call here.
- `layout.*`, `editor.toggleZen`: none (UI storage `openidea.toolWindowLayouts.v1`, `openidea.distractionFree`, and the project layout key).
- `app.about`: `app_build_info`, `app_info` (version from `CARGO_PKG_VERSION`, build from `OPENIDEA_BUILD`, the configuration folder from `Backend::dirs`).
- `file.newProject`: `fs_read_dir`, `fs_create_dir`, `vcs_init`, `window_open_project`.
- `file.invalidateCaches`, `app.restart`: `caches_schedule_invalidation(options | null)` → `ide_workspace::caches::schedule` / `cancel` in `Backend::dirs.data`, then `app_restart`. The desktop shell sets a restart flag and runs `app_quit` (every window's close check), then Tauri relaunches. Natively, `app_shell::quit_then(…, relaunch)` runs the same close checks; the production `relaunch` is `App::restart`, where the platform waits for this process to exit, opens the app bundle again and quits. Tests install `Relaunch` instead.
- `file.exportSettings`: `settings_export(path, storage)` → `SettingsStore::export_bundle`.
- `file.importSettings`: `settings_import(path)` → `SettingsStore::import_bundle`. Natively it also reloads each project's settings and file defaults, reconfigures its language servers, re-applies the SSH configurations and the proxy, and emits `SettingsChanged { key: "" }` to every window. The desktop shell's MCP re-apply and update-checker wake have no native counterpart.
- `file.togglePowerSave`: `settings_set("global", "general.powerSaveMode", …)`.

## Not wired, and why

- The trust question's "an attached workspace member lost its trust" path (the banner naming the member and Trust Project asking about it): the Project view shows the primary folder only, and workspace attachment is not built (`app_shell` drops Attach).
- Trust Settings page (trusted projects and locations with Revoke / Remove / Add folder…): it belongs to the Settings dialog's pages. The actions it runs are registered.
- Layout chooser: the webview's list dialog is a popup menu of the saved names here (no title row). Rename with a taken name shows "A layout with this name exists" as a message after OK instead of under the field: `FieldRule` has no rule for it.
- About: no logo (no app-logo icon in `icons.json`). The Runtime row says `GPUI` without a version. Keymap Reference and Show Log links appear only when those actions are registered (they are not).
- New Project Browse… cannot start in the typed location: `PathPromptOptions` has no starting folder.
- Import Settings… of a JetBrains `.zip`: it reads as an OpenIDEA bundle and fails with the backend's "not an OpenIDEA settings file". The file dialogs have no type filter (`PathPromptOptions` has none).
- The Power Save Mode banner above editors: the editor banner slot is not in this module.

## Assumptions to check first

1. `TestAppContext::simulate_new_path_selection` and `simulate_path_prompt_response` are reachable on `VisualTestContext` (the second one already is, at `run.editConfigs`'s test).
2. In the layouts test, `simulate_keystrokes("enter")` chooses the first row of the popup menu that `show_menu` opens: the first enabled row is selected on open (`PopupMenu::new`).
3. `TextField::set_text` leaves the caret at the end, so the test's backspaces clear the prompt's initial name.
4. `TitleBar::mode()` follows `set_mode` during the next draw (section 4).
5. `App::restart` exists in the pinned GPUI (`gpui-pre-0.3.12/src/app.rs`) and quits after spawning the relauncher.

## Compile risks

- `subscribe_in` handlers that match on `*event` (`TrustDialogEvent::Run(action)`): the field is `&'static str`, so it copies.
- `AsyncApp::update(end)` with `end: fn(&mut App)`.
- The async block in `import_settings` ends with `ide_core::IdeResult::Ok(storage)` so that `?` infers the error type.
- `app_views::footer` takes `Option<Stateful<Div>>` and `Vec<Stateful<Div>>`.

## Tests most likely to fail on first run

- `saves_and_restores_visible_windows_and_sizes_by_name`: it depends on assumptions 2 and 3 and on the popup taking the keyboard.
- `exports_templates_macros_layouts_and_view_options_not_per_project_state`: `import_bundle` writes the global settings file (in the temp home) and `apply_proxy` runs in the test process.
- `invalidate_and_restart_schedules_the_chosen_caches_and_restarts` and `just_restart_…`: they depend on 5a. The quit loop closes the project window with `confirmExit` off; a leftover run session would ask first.
- `hides_tool_windows_and_restores_the_previous_layout_on_exit`: it depends on 3e, 3f and 4.
