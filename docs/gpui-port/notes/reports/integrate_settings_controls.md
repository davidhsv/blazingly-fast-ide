Both parts are in and green. The full suite passed five times in a row on the final tree with 6699 passed, 0 failed, 0 ignored, and clippy `--all-targets -D warnings` is clean. Nothing is committed. Some pages are still partial against the webview; details in §3 and §4.

## 1. Suite, clippy, scope, slow tests

- **Runs ×5:** 91.2 s, 90.9 s, 90.7 s, 90.8 s, 90.8 s. That is up from about 87 s; the new Part 0 tests run one at a time under the plugin-host lock (see the times below).
- **Compile errors:**
  - Part 1 drafts: 5 in total. 3 were `.ok()` called on a `()` result, 1 a missing lifetime in a test helper, 1 an unused import.
  - Clippy on the drafts: 2 (a complex type, an unneeded `vec!`).
  - Part 0: none.
- **Scope:** I only edited files under `app-gpui/src`, and only those files went through `rustfmt`. No git command changed anything.
- **The ten slowest tests:**
  - `plugin_java_debug::stop_ends_the_process_and_the_session` 16.7 s
  - `plugin_runtime::a_changed_plugin_setting_reaches_the_plugin_as_settings_changed` 15.6 s
  - `plugin_java_debug::an_attach_the_adapter_refuses_is_reported_with_its_message` 15.5 s
  - `plugin_runtime::a_declared_status_item_shows_and_a_click_runs_its_plugin_command` 15.4 s
  - `plugin_runtime::a_plugin_opens_a_file_at_a_position_copies_text_and_opens_a_url` 15.3 s
  - `plugin_runtime::a_plugins_task_is_listed_while_it_runs_and_cancel_stops_it` 14.3 s
  - `plugin_java_debug::an_announced_host_is_ignored_and_the_debugger_attaches_on_loopback` 14.1 s
  - `vcs_diff::diff_options::falls_back_to_transferable_chunks_on_initial_comparison_and_either_side_edits` 12.7 s
  - `plugin_runtime::a_plugins_text_question_is_answered_and_the_command_gets_the_text` 12.6 s
  - `plugin_java_debug::a_process_that_exits_before_announcing_a_port_leaves_no_debug_session` 12.5 s
- **Reading those times:** measured with `RUSTC_BOOTSTRAP=1 … --report-time` on an earlier tree (that run had 6699 passed). The plugin tests' times include waiting on the process-wide plugin-host lock. My new Part 0 tests measure 2.7–8.8 s each.

## 2. Part 0: a project's own plugins

**Webview:**

- `src/features/plugins/index.ts:167` lists the plugins again whenever the project root or its trust changes, and at start (`:170`).
- `plugin_list` (`src-tauri/src/commands/plugin.rs:178-190`) calls `sync_project(root)`, then `list(Some(root))`.
- Revoking trust or removing a trusted location calls `restrict_project` (`src-tauri/src/commands/app.rs:336, :354`).
- Closing a project does not unload anything (`state.rs:173` only detaches). The plugins simply stop being listed.
- A freshly loaded project plugin waits for consent, which is given on the Plugins page (`PluginsPage.tsx:163, :476`).

**Native before the fix (measured):** with the refresh made to skip project roots, the three trusted-project tests timed out waiting for "the project's plugins are loaded" (0/3 passed). `Plugins::refresh` never called `sync_project` and listed with `None`.

**Fix:**

- `plugins.rs`: `refresh` loads the own plugins of every open trusted project and merges them into the listing. Projects that are not trusted are left alone, so restricting them again cannot cause a listing loop.
- `plugins.rs`: new `release_project` unloads a project's plugins when no other window still has it open. A `listings()` counter backs the loop bound in the tests.
- Trigger: `plugin_runtime.rs::plugin_project_changed` refreshes on a root or trust change.
- Close: `Workbench::close_project` calls `release_project`.
- Two failure checks now find a plugin by id with `info_of` (`plugins.rs` `run_command`, `plugin_runtime.rs`), so project plugins are no longer missed.

**Tests** (`app-gpui/src/workbench/plugins_window.rs`; the tests grant consent with `host.grant`, standing in for agreeing on the Plugins page):

- `a_trusted_projects_own_plugins_load_when_it_opens_and_their_command_runs`: the action runs through the registry; menu entry, status item, tool window and settings page are present.
- `an_undecided_projects_own_plugins_do_not_load_and_the_listing_does_not_repeat`: at most 1 extra listing per quiet second, both without and with the Plugins page open.
- `trusting_an_open_project_loads_its_own_plugins`
- `revoking_trust_unloads_the_projects_own_plugins_and_their_contributions`
- `closing_a_project_unloads_its_own_plugins`

**Parity keys they would prove:** `ipc:plugin_list`, `ipc:project_trust_set`, `ipc:close_project`, `feature:plugins`.

## 3. Part 1: the Settings pages' own controls

Every page below opens through `file.settings` with no row saying "This page is not available yet". All tests are under `settings_view::custom_controls::tests::`.

| Page                                                | e2e test                                                                                                                                                                                                             | Status, and what is missing                                                                     |
| --------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `settings:appearance`                               | `appearance_accent_zoom_and_ui_options_apply` (the accent shows in the theme)                                                                                                                                        | partial: Theme lacks Sync with OS and preferred themes; no plugin icon themes; no colour picker |
| `settings:general`                                  | `system_settings_open_in_directory_and_idle_save_apply`, `a_stored_attach_for_open_project_in_shows_ask`                                                                                                             | partial: the chooser has no title or start directory                                            |
| `settings:notifications`                            | `notifications_no_popup_applies_and_the_groups_balloon_is_not_shown`                                                                                                                                                 | done                                                                                            |
| `settings:build.console`                            | `console_buffer_and_fold_patterns_apply_and_cancel_discards`                                                                                                                                                         | done                                                                                            |
| `settings:build.debugger.dataViews`                 | `data_views_delay_steps_with_the_arrows_and_reset_restores_it`                                                                                                                                                       | done                                                                                            |
| `settings:build.deployment.options`                 | `deployment_excludes_are_written_to_the_project` (project scope)                                                                                                                                                     | done                                                                                            |
| `settings:tools.terminal`                           | `terminal_environment_cursor_and_shell_apply`, `the_login_shell_is_read_after_the_dialog_opens`                                                                                                                      | done                                                                                            |
| `settings:vcs.commit`                               | `vcs_commit_limits_and_checks_apply`                                                                                                                                                                                 | done                                                                                            |
| `settings:vcs.confirmation`                         | `vcs_confirmation_radio_buttons_and_history_limit_apply`                                                                                                                                                             | done                                                                                            |
| `settings:vcs.git`                                  | `vcs_git_test_protected_branches_and_update_method`, `git_test_is_refused_in_restricted_mode`, `git_is_detected_after_the_dialog_opens_and_test_runs_it`, `passwords_lists_a_saved_credential_and_forget_removes_it` | done                                                                                            |
| `editor.font` (no `settings:` key in `parity.json`) | `editor_font_family_and_weight_apply_and_cancel_discards`, `the_editor_draws_with_the_font_applied_on_the_font_page`                                                                                                 | partial: no Preview; bold weight not applied                                                    |
| `editor.codeStyle` (no `settings:` key)             | `code_style_visual_guides_and_do_not_format_apply`                                                                                                                                                                   | partial: no General / Formatter tabs; formatter marker rows are not gated                       |

Also on every page:

- `on_every_page_reset_and_cancel_discard_an_edit_of_an_own_control` covers Reset and Cancel.
- `the_search_and_the_modified_marker_treat_own_controls_like_standard_ones` covers search and the modified marker.

**Limits on "done":**

- Each page's e2e test covers 2–4 of its own controls, not every one.
- The remaining controls were compared with the webview by reading only (keys, scopes, limits, texts). That reading found matches except where the table says otherwise.
- Hypothesis: every number field is partial. An invalid value is flagged "(invalid)" but has no "Enter a value between {min} and {max}" tooltip (`Form.tsx:149`). Grepping `app-gpui` for that string would confirm.

**Editor font:** the native editor now follows `editor.fontFamily`, `fallbackFont`, `fontSize`, `lineHeight`, `ligatures` (off by default, as in the webview) and `fontWeight`.

- The family is the first installed of the named family, the fallback, then the theme's mono list.
- Settings changes repaint it. The changes are in `editor/language_facts.rs` (`EditorFont`) and `editor/view.rs` (`code_font`, `text_style`).
- The test seeds `InstalledFontNames` by hand.
- Still on theme tokens, although the webview gives them the editor font: diff views (`vcs/cm.ts:434`), plugin and service log editors, and LSP popups (`editor_lsp/*`).

**Fixes on the way, each tested:**

- **Fonts:** never read when Settings opens. The first page with a font picker reads them on the background executor, once even with two pickers on the page. Test builds read none (`the_installed_fonts_are_read_once_a_font_picker_is_shown`).
- **Git and shell:** finding git on the PATH and the login shell now run off the window's thread (the two detection tests above).
- **Legacy `attach`:** loads as Ask. Any radio row whose stored value is not one of its choices shows the default, as the webview's `oneOf` does.
- **f32 hypothesis:** false. `schema.rs::same` compares numbers by value; `typing_the_stored_size_as_a_whole_number_leaves_the_page_unmodified` passes.
- **Git › Test:** refused in Restricted Mode with the backend's message.
- **Forget:** works end to end through the file store under the temp home. The test proves the secret was on disk before Forget, never in a file afterwards, and that the system keychain was never set up.
- **Secrets:** no secret is drawn or logged (checked by capture). No credential type is `Debug`-printed in native code.

## 4. Other changes, test edits, notes, hypotheses

**Product changes beyond the notes:**

- `backend.rs` keeps its own Git credential store, installed under a lock, so parallel tests cannot swap it.
- Font pickers get a hint line: "For symbols not supported by the main font" (`font.tsx:58`) and "Used for characters the main font lacks." (`tools.tsx:291`).
- Terminal › Size is no longer disabled under "Use editor font" (`tools.tsx:271`).
- Escape in a list edit gives the keyboard back to the list.
- New small accessors: `TextField::set_placeholder` and `placeholder`, and `SettingsDialogView::custom_control`.

**Drafted tests I changed (the tests were wrong, not the product):**

- Staged keys are compared as `scope:key`; the helper produced `scope|key`.
- `console.overrideBuffer` defaults to true, so the console and form tests now start from a stored `false`.
- Typing "#ff00" one character at a time stages "#ff0", as the webview's `HEX_COLOR` does.

**Notes that turned out false:**

- Note assumption 6 (the default hard wrap is not 100).
- The claims that Forget is untestable and that the editor font has no visible effect.

**Left out:** no drafted test was left out. The MCP Endpoint was left out, as instructed.

**Hypotheses (unmeasured):**

- **Multi-window:** the merged listing may show project A's plugin UI in a window of project B; the webview lists per window. A two-window test would settle it.
- **Window close:** closing a non-last window may not release its project's plugins. Reading `request_close` in `app_shell.rs` would settle it.
- **Webview loop:** with a restricted project open, the webview may itself re-list every 30 ms, because `restrict_project` always announces a change (`bridge.ts:300` → `scheduleRefresh`).
- **Font enumeration:** the editor's and the diff view's `all_font_names` call on every paint predates this work. No test exercises the real font-reading path.
