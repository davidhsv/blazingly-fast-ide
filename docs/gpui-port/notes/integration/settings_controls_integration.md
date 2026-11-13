# Integrating the Settings pages' own controls

New files, not compiled yet (drafted without a compiler; run `cargo fmt` after adding them: several lines run past 120 columns):

- `app-gpui/src/settings_view/custom_controls.rs` (737 lines): `install(dialog, Install { window_label, has_project }, window, cx) -> Vec<Subscription>`, the answers to the views' requests, and 13 end-to-end tests (one per page, plus `on_every_page_reset_and_cancel_discard_an_edit_of_an_own_control`, which runs Reset and Cancel on one own control of each of the 12 pages). Every e2e test also asserts, on opening the page, that no line of `control_lines` says "This page is not available yet" (so a slot label typo fails).
- `app-gpui/src/settings_view/pages/mod.rs` (1324): `Binding`, `ControlEvent`, drawing helpers, the declarative `FormView` (`Part`: toggle, toggle+number, field, radio row, select with a checkbox beside, two columns, page link), `StackView`, `test_support`, 4 tests.
- `pages/list_editor.rs` (525), `pages/font_picker.rs` (449): shared list editor and font picker (`FontCatalog` global = the font list seam).
- One file per page: `appearance.rs` (396), `general.rs` (81), `notifications.rs` (186), `console.rs` (63), `data_views.rs` (45), `deployment_options.rs` (42), `terminal.rs` (317), `vcs_commit.rs` (83), `vcs_confirmation.rs` (62), `vcs_git.rs` (389), `editor_font.rs` (292), `code_style.rs` (184).

No existing file was edited.

## 1. `app-gpui/src/settings_view/mod.rs`

After `pub mod controls;` add:

```rust
pub mod custom_controls;
```

and after `pub mod keymap;` add:

```rust
pub mod pages;
```

## 2. `app-gpui/src/workbench/settings_window.rs`: install the controls

In `pub fn open_settings`, replace

```rust
        subscriptions.extend(self.mount_plugin_settings_pages(&dialog, has_project, window, cx));
```

with

```rust
        subscriptions.extend(self.mount_plugin_settings_pages(&dialog, has_project, window, cx));
        let install = crate::settings_view::custom_controls::Install { window_label: &self.label, has_project };
        subscriptions.extend(crate::settings_view::custom_controls::install(&dialog, install, window, cx));
```

`self.label` is borrowed while `cx` (a `Context<Workbench>`, which derefs to `App`) is passed mutably; if the borrow checker refuses, take `let label = self.label.clone();` first and pass `&label`.

No other edit is needed: the views stage into the dialog's model, so the existing `apply_settings` writes them; nothing goes through `apply_next_external`.

## 3. Assumptions (not confirmed by compiling)

1. A child `AnyView` renders after its parent's `render` returned, so a control reading `SettingsDialogView::scope` from its own render (through `Binding`'s `ScopeOf`) does not hit a leased entity. If it panics ("already being updated"), replace the `ScopeOf` closure in `install` by one that reads only the model: `model.scopes.get(scope_key)` + the spec's `default_scope`, as `SettingsDialogView::page_scope` does (needs the spec, which `install` can take from `settings_registry`).
2. `App::spawn(async move |cx: &mut AsyncApp| …)`, `cx.background_executor()` on `AsyncApp`, `WeakEntity::update(&mut AsyncApp, …)` returning a `Result`, and `App::subscribe(&entity, |entity, event, cx| …)` have the shapes used; `App::new` needs `AppContext` in scope (imported).
3. `theme::tokens::Color` has public `r, g, b, a: f32` fields (as `theme::color` reads them).
4. `Pixels` converts with `f32::from` (gpui `impl From<Pixels> for f32`), `TextSystem::advance` returns `Result<Size<Pixels>>`.
5. `crate::editor::actions::register`, `crate::workbench::register_actions`, `crate::workbench::menu::init`, `crate::backend::AppDirs::under`, `crate::terminal::test_support::isolate_shells`, `Workbench::{new, open_project, project_tree, settings_dialog, notify, notifications}` are reachable from `settings_view` (all read as `pub`).
6. Default `editor.codeStyle.hardWrapAt` is not 100 (view test of the Scheme copy).
7. The dialog's root `on_mouse_down` closes only its own lists; each own drop-down closes on a choice, a second click on its box, or a press elsewhere inside its own control. Escape in the search does not close them.

## 4. Compile risks

- Long lines (rustfmt) and `clippy::too_many_lines` are not checked. `Part` derives `Clone` only (it holds `Rc` closures).
- `FormView::sync` borrows `self.fields` mutably while reading `self.binding` (disjoint fields: fine in a method body).
- `ListEditor::start_edit` builds claims in a `.map` closure that captures `cx` mutably while `fields` is borrowed: if refused, use a `for` loop.
- `GitCredentialsView::render` and `NotificationGroupsView::render` call `cx.listener` inside iterator closures (the pattern of `controls.rs`).
- `vcs_git::enter_escape` and `terminal::submit_or_cancel` are the same function twice; merge into `pages/mod.rs` if clippy or review asks.
- `install` enumerates the system fonts (`all_font_names` + `resolve_font` + two `advance`s per family) on the main thread the first time Settings opens, when no `FontCatalog` global exists yet. With hundreds of families this can be a visible pause; moving it behind the first open of a font list (or to start-up) is the fix. The tests set an empty `FontCatalog` in `start()`, so they enumerate nothing.

## 5. Tests most likely to fail first

- `custom_controls::tests::vcs_git_test_protected_branches_and_update_method`: waits for `settings-git-ok` (the error line has `settings-git-error`); runs the real `git --version` through `ide_host::ops::vcs::vcs_git_version`, which needs the window's project to be trusted (the test opens a trusted project). It depends on `git` on the PATH, like the existing Git tests.
- `terminal_environment_cursor_and_shell_apply`: the "+" row of the environment table takes the keyboard on its first field; if `window.focus` inside the click does not stick (a parent `track_focus` takes it), mark the press handled in `ListEditor`'s add button (`window.prevent_default()` in an `on_mouse_down`).
- `editor_font_family_and_weight_apply_and_cancel_discards`: the font picker opens its list on mouse down; Enter takes the typed name (no suggestion is marked).
- `type_into` clicks a field frame and relies on `TextField` taking focus on mouse down (as the smoke test does for standard fields).
- Clicks below the page's visible height (about 590 px, a scrolled container) may miss: the tests use controls near the top of each page; the Reset/Cancel table test is the first place to look if a `… is drawn` panic or a wrong value shows.
- `appearance_accent_zoom_and_ui_options_apply` waits for `theme::appearance::apply` to follow `appearance.accent` (it listens to `SettingsChanged`).

## 6. Not built, and why

| What                                                                                                           | Why                                                                                                                                                                                                                                                                                                                                                                                                                               |
| -------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| MCP Server › Endpoint (Status, Endpoint, Copy HTTP / Stdio Config)                                             | The native app runs no MCP server: `mcp_server_status` is built from `McpService` in `src-tauri`, no `ide_host::ops` function exists and `app-gpui` does not depend on `ide-mcp`. The slot keeps "This page is not available yet". When it is built: `McpServerStatus` derives `Debug` with the token in it, and "Copy HTTP Config" copies the token to the clipboard, which the no-secret-copied rule forbids: needs a decision. |
| Editor › Font preview (the page's footer)                                                                      | The native spec has no slot for a footer.                                                                                                                                                                                                                                                                                                                                                                                         |
| Appearance › Theme with Sync with OS, preferred light / dark themes, plugin themes, disabled while synced      | Theme is a standard `Select` in `builtin_pages/behavior.rs`, not a slot: needs a spec change (`custom("Theme", …)`).                                                                                                                                                                                                                                                                                                              |
| Code Style › General / Formatter tabs                                                                          | The native spec flattens them into sections.                                                                                                                                                                                                                                                                                                                                                                                      |
| File icon theme choices from plugins                                                                           | No plugin host lists contributed icon themes to the dialog; only a stored theme is offered (as "… (Plugin not enabled)"), else the row is hidden as on the webview.                                                                                                                                                                                                                                                               |
| Fallback-font hints ("Used for characters the main font lacks.", "For symbols not supported by the main font") | `FontPicker` draws its status line, not a hint.                                                                                                                                                                                                                                                                                                                                                                                   |
| Blur commits an inline list edit                                                                               | Commits happen on Enter, a click on another row, or a toolbar button; Tab / Shift-Tab move between the two cells of a table row.                                                                                                                                                                                                                                                                                                  |
| Visible effect of Editor › Font                                                                                | The editor does not follow `editor.fontFamily`: `Editor::code_font` and `metrics` read the theme's `font-mono` / `fs-code` tokens only. The e2e test asserts the stored setting.                                                                                                                                                                                                                                                  |
| Git Passwords Forget end to end                                                                                | `GitCredentials::forget` goes through the keychain vault; the in-memory store is behind ide-ssh's `testing` feature, which `app-gpui` does not enable. Covered at view level; e2e checks the empty state. To cover it: `ide-ssh = { path = "../crates/ide-ssh", features = ["testing"] }` under `[dev-dependencies]` and `ide_ssh::secrets::testing::use_memory_store()` in the test.                                             |

Backend calls (IPC command → native call): `vcsGitVersion` → `ide_host::ops::vcs::vcs_git_version(&host, window_label, path)`; `vcsDetectedGit` → `ide_host::ops::vcs::vcs_detected_git(&host, "")`; `gitCredentials` → `ide_ssh::git_creds::get().list()`; `gitCredentialForget` → `ide_ssh::git_creds::get().forget(key)`; `termDefaultShell` → `ide_terminal::default_shell_path()`; `termAvailableShells` → `ide_terminal::available_shells()`. `vcs_detected_git` and `default_shell_path` run on the main thread at install (a PATH lookup and an env read); the rest run on the backend runtime or the background executor. No test opens a socket; nothing reads the user's home or keychain; the font list comes from the `FontCatalog` global, which the font test fills.

## 7. Pages

| Page id                    | Own controls built                                                                                                                                                                                                                                        | End-to-end test (`settings_view::custom_controls::tests::…`)         | parity key                                              | Status                                                                                         |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- | ------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `appearance`               | Editor color scheme, File icon theme (stored one only), Accent colour, Different tool window background, Zoom (+ shortcut hint), Use custom font (UI font, fallback, size), Main menu (Windows/Linux only), Compact mode grid, Show tool window bars grid | `appearance_accent_zoom_and_ui_options_apply`                        | `settings:appearance`                                   | partial: Theme's Sync with OS / preferred themes / plugin themes (no slot); plugin icon themes |
| `general`                  | Open project in, Default directory (+ Browse…), Save files if idle                                                                                                                                                                                        | `system_settings_open_in_directory_and_idle_save_apply`              | `settings:general`                                      | done                                                                                           |
| `notifications`            | Popup type table (popup type + Show in tool window per group)                                                                                                                                                                                             | `notifications_no_popup_applies_and_the_groups_balloon_is_not_shown` | `settings:notifications`                                | done                                                                                           |
| `build.console`            | Override console cycle buffer size, Fold console lines that contain                                                                                                                                                                                       | `console_buffer_and_fold_patterns_apply_and_cancel_discards`         | `settings:build.console`                                | done (list commits on Enter, not on blur)                                                      |
| `build.debugger.dataViews` | Value tooltip delay (ms) with "Value tooltips are off"                                                                                                                                                                                                    | `data_views_delay_steps_with_the_arrows_and_reset_restores_it`       | `settings:build.debugger.dataViews`                     | done                                                                                           |
| `build.deployment.options` | Exclude items by name                                                                                                                                                                                                                                     | `deployment_excludes_are_written_to_the_project`                     | `settings:build.deployment.options`                     | done                                                                                           |
| `tools.terminal`           | Start directory, Environment variables, Shell path (installed shells, Browse…), Audible bell grid (+ Configure terminal keybindings link), Cursor shape + Blinking cursor, Use editor font (font, size, line height, fallback, column width)              | `terminal_environment_cursor_and_shell_apply`                        | `settings:tools.terminal`                               | done (fallback hint missing)                                                                   |
| `tools.mcpServer`          | none                                                                                                                                                                                                                                                      | none                                                                 | `settings:tools.mcpServer`                              | partial: Endpoint not built (no native MCP server)                                             |
| `vcs.commit`               | Limit subject / body line, Show right margin, Wrap at margin, commit checks grid                                                                                                                                                                          | `vcs_commit_limits_and_checks_apply`                                 | `settings:vcs.commit`                                   | done                                                                                           |
| `vcs.confirmation`         | When files are created, When files are deleted, Limit history to                                                                                                                                                                                          | `vcs_confirmation_radio_buttons_and_history_limit_apply`             | `settings:vcs.confirmation`                             | done                                                                                           |
| `vcs.git`                  | Path to Git executable + Test, large-file warning, Push dialog pair, Protected branches, Update method, incoming-commit checks, Passwords                                                                                                                 | `vcs_git_test_protected_branches_and_update_method`                  | `settings:vcs.git`                                      | done (Forget only view-tested)                                                                 |
| `editor.font`              | Font (monospaced filter, Refresh), Size + Line height, Main / Bold font weight (+ Custom), Fallback font                                                                                                                                                  | `editor_font_family_and_weight_apply_and_cancel_discards`            | none in `parity.json` (add `settings:editor.font`)      | partial: Preview; the editor does not use the font                                             |
| `editor.codeStyle`         | Scheme (Default / Project, Copy to Project / IDE), Visual guides, Do not format                                                                                                                                                                           | `code_style_visual_guides_and_do_not_format_apply`                   | none in `parity.json` (add `settings:editor.codeStyle`) | partial: General / Formatter tabs                                                              |

## 8. Differences of the existing native standard controls from the webview

- Appearance › Theme: a plain select of built-in themes. No Sync with OS, preferred themes, plugin themes or preview on hover.
- Code Style: the General / Formatter tabs are one page of sections.
- System Settings › Open project in: a stored legacy `"attach"` shows no radio selected; the webview loads it as Ask.
- Hypothesis (from reading, not run): the standard number fields stage integers (`number_json`) even for `f32` settings, and `SettingsDialog::stage` compares with the base by JSON equality, so typing back the stored `13.0` as `13` may leave a staged no-op and the page marked modified. Check: stage `json!(13)` on a base holding `13.0` and assert `has_staged()`. The own controls compare numbers by value (`pages::same_value`) only for their field sync; they stage through the same `stage`.
