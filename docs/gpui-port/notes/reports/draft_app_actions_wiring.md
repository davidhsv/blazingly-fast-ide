I drafted the application- and window-level actions: one workbench module, four dialog views and the integration notes. Nothing has been compiled or run (cargo was off-limits here), so every test below is written but unrun, and the integrator's `cargo fmt` / clippy / test pass is the first check.

### 1. Files written

| File                                                                                                                                                                | Lines |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----- |
| `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/app_actions.rs`                              | 1484  |
| `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/app_views/mod.rs` (shared dialog parts)                | 142   |
| `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/app_views/trust_dialog.rs`                             | 222   |
| `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/app_views/about_dialog.rs`                             | 318   |
| `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/app_views/new_project_dialog.rs`                       | 313   |
| `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/app_views/invalidate_caches_dialog.rs`                 | 239   |
| `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/app_actions_integration.md` | notes |

The views:

- **`TrustDialog::new(path)`**
  - Event: `Run(&'static str)`, the action of the pressed button.
  - Accessors: `path()`, `parent()`, `folder()`.
- **`AboutDialog::new(build, config_dir, actions)`**
  - Events: `Closed`, `RunAction`, `RevealConfig`, `OpenUrl`.
  - Accessors: `product()`, `build_line()`, `facts()`, `info_text()`, `copied()`.
  - The version comes from `this_build()`.
- **`NewProjectDialog::new(location)`**
  - Events: `Create{path, git}`, `Browse`, `Cancelled`.
  - Accessors: `name_field()`, `location_field()`, `git()`, `error()`, `path()`, `hint()`, `can_create()`.
  - Setters: `set_busy()`, `set_location()`.
- **`InvalidateCachesDialog::new`**
  - Events: `Run([CachesEffect; 2])`, `Cancelled`.
  - Accessors: `choices()`, `warns()`.

The dialogs reuse the dialog slot Git operations use, so `mod.rs` gets no new fields.

### 2. Actions registered (all 20 tested end to end, through the registry or a click)

| Action                                                                                     | Test                                                                                                                                                  | IPC commands it stands for                                        |
| ------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| `project.trust`                                                                            | `updates_the_running_projects_banner_after_a_backend_trust_transition` (banner gone, project code allowed to run), `saves_the_parent_folder_choice_…` | `project_trust_set`, `project_trust_status`                       |
| `project.previewRestricted`, `project.dontOpen`                                            | `offers_three_choices_cancels_without_a_trust_decision_and_supports_restricted_preview`                                                               | `project_trust_set`                                               |
| `project.revokeTrust`                                                                      | `revoking_trust_or_removing_its_location_restricts_the_project`                                                                                       | `project_trust_set`                                               |
| `project.removeTrustedLocation`                                                            | `removing_a_trusted_location_restricts_the_projects_inside_it`                                                                                        | `project_trust_remove_location`                                   |
| `layout.save`, `layout.restore`, `layout.restoreDefault`, `layout.rename`, `layout.delete` | `saves_and_restores_visible_windows_and_sizes_by_name` (Run and Project windows checked after Save, change, Restore)                                  | none (UI storage)                                                 |
| `editor.toggleZen`                                                                         | `hides_tool_windows_and_restores_the_previous_layout_on_exit` (status bar, stripes, compact title bar, Enter/Exit title, hint)                        | none                                                              |
| `app.about`                                                                                | `about_shows_the_version_of_the_build`                                                                                                                | `app_build_info`, `app_info`                                      |
| `file.newProject`                                                                          | `new_project_creates_the_folder_and_opens_it` (temp folder created and opened, "not empty" error shown)                                               | `fs_read_dir`, `fs_create_dir`, `vcs_init`, `window_open_project` |
| `file.invalidateCaches`                                                                    | `invalidate_and_restart_schedules_the_chosen_caches_and_restarts`                                                                                     | `caches_schedule_invalidation`, `app_restart`                     |
| `app.restart`                                                                              | `just_restart_cancels_a_pending_invalidation_cancel_does_nothing`                                                                                     | `caches_schedule_invalidation(null)`, `app_restart`               |
| `file.exportSettings`, `file.importSettings`                                               | `exports_templates_macros_layouts_and_view_options_not_per_project_state`                                                                             | `settings_export`, `settings_import`                              |
| `file.togglePowerSave`                                                                     | `power_save_mode_toggles_the_global_setting`                                                                                                          | `settings_set`                                                    |

How the safety limits are met:

- **Restart:** it runs the same close checks as Quit, then calls a `Relaunch` seam. In production the seam is `App::restart`: the platform waits for this process to exit, reopens the app and quits. Tests replace it with a counter, so nothing quits or relaunches.
- **Caches:** the invalidation is written only to the temp home's data folder.

**Not registered:**

- `app.checkForUpdates`: the native app has no updater.
- `app.installCli`, `app.uninstallCli`: the native binary has neither the `--cli` client mode nor the socket server that `src-tauri/src/cli.rs` installs a link to.
- `macro.*` (5 actions): the editor has no hook that reports typed text or editor keys to record.
- `navigate.nextOccurrence`, `navigate.prevOccurrence`: no result tool window registers an occurrence navigator.
- `edit.*` gaps: undo, copy and the rest for text fields and lists need focus routing outside this module.
- `perf.*` (4 actions): the memory status item and the overlay view are not fed.
- `toolwindow.closeContent`: no strip of closable tool window tabs is registered.
- `toolwindow.hideActive`: already registered and tested elsewhere.
- `help.keymapReference`, `help.showLog`: no Keymap Reference view, and the native app writes no log file.
- `find.field.*`, `window.*Tab*`, `workspace.*`, `project.attach`, `project.detach`, `file.renameProject`, `file.openPaths`, `lightEdit.openProject`, `view.backgroundTasks`, `view.quickSwitchScheme`, `workbench.stopBackgroundProcesses`: out of this pass.

### 3. Spec files covered

| Spec                        | Covered                                                                                                                                               |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `trust.test.tsx`            | 3 of 7. The symlink case is covered implicitly. Not built: the settings page "add folder", the local-scope opening and the attached workspace member. |
| `layouts.test.ts`           | 4 of 5. "Persists and ignores malformed entries" is the model's own test.                                                                             |
| `invalidateCaches.test.tsx` | 2 of 2                                                                                                                                                |
| `newProject.test.ts`        | 2 of 3. Path joining is the model's own test.                                                                                                         |
| `distractionFree.test.tsx`  | 3 of about 12                                                                                                                                         |

### 4. Gaps, things to check first, likely first failures

**Not built:**

- The attached-member trust path and the Trust settings page.
- The layout chooser is a popup menu, not a list dialog. A taken name on rename is reported after OK, not under the field.
- About has no logo, and the Runtime row has no version.
- Browse… cannot start in the typed folder.
- Importing a JetBrains `.zip` fails with the backend's error.
- No Power Save banner, and the Distraction Free menu label stays "Enter".
- MCP re-apply on import and plugin restriction on remove-location have no native counterpart.

**Check first:**

- The hunks in the notes are required: `app_shell` gets `quit_then` and the trust question before a window is chosen; `chrome.rs` gets the compact title bar; `menu.rs` gets the saved-layouts items; `UiStorage` gets `entries()`. `mod.rs` gets the module line, the registration, context publishing, a redraw on trust changes, the status bar and stripes checks, the banner and the layout-load hook.
- The trust question now appears on every undecided opening. One existing test, `projects_open_in_this_window_a_new_one_or_the_one_that_has_them`, would wait on it, so the notes include a hunk that trusts its folders. Other tests opening undecided folders through the open flow would need the same.
- The New Project test leaves "Create Git repository" unchecked, because the backend's git would read this machine's configuration.

**Likely first-run failures:**

- The layouts test: it relies on Enter choosing the popup's first row and on backspaces clearing the prompt.
- Export/import: import runs `apply_proxy` in the test process.
- The Distraction Free and restart tests: they fail until their hunks are in.
