## Integration report: plugins (Part 0, stages 1–3)

All three stages are in and green. Part 0 was never run on its own: I applied stage 1 before the first build, so it was checked together with stage 1. Nothing is committed.

### 1. Suite, clippy, scope

- **Suite ×5 (final code):** `cargo test --lib` gave 6386 passed, 0 failed, 0 ignored each time (48.7 s, 48.7 s, 50.4 s, 51.7 s, 49.0 s).
- **Clippy:** `cargo clippy --all-targets -- -D warnings` is clean.
- **Errors per stage:**
  - Stage 1: 0 compile errors, 2 clippy (`needless_borrows_for_generic_args`).
  - Stage 2: 0 compile errors, 1 unused-import warning, 1 clippy (`needless_borrow`).
  - Stage 3: 1 compile error (E0382, `registered` moved by `map_or_else`), 1 clippy (`explicit_auto_deref`).
- **Scope:**
  - Changes are limited to `app-gpui/` (27 files, including `Cargo.toml` and `Cargo.lock`, which only gained `ide-extension`), with no change under `crates/`, `docs/`, `src/` or `src-tauri/`.
  - `src/bindings.ts` was already modified when I started and is clean now; I did not touch it.
  - I never ran `cargo fmt` and never ran a git command that changes anything.
  - My per-file formatter (`$S/fmt.sh`, rustfmt via stdin) writes temp files under `/tmp`. One recursive `rustfmt` call was `--check` only.

### 2. Actions (full test paths start `openidea_gpui::`… as `--list` prints them)

| action                                          | test                                                                                                                                                                                                                        | IPC → backend                                                                                    | state                                                                              |
| ----------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------- |
| `plugins.installFromDisk`                       | `workbench::plugins_window::tests::install_from_disk_takes_a_plugin_folder_by_its_manifest_and_makes_its_commands_actions`, `…::installing_from_disk_asks_for_consent_with_high_risk_first_and_installs_only_when_accepted` | `plugin_preview_install`/`plugin_install_dir` → `preview_install`/`install_from_dir` (same call) | partial: the picker has no title                                                   |
| `plugins.reloadAll`                             | `workbench::plugins_window::tests::reload_plugins_lists_a_plugin_whose_manifest_no_longer_parses_as_failed`                                                                                                                 | `plugin_reload` → `host.reload` (same)                                                           | done; the "Could not reload" branch has no test                                    |
| `plugins.log.clear` / `wrap` / `toggleArgument` | `workbench::plugin_runtime::tests::starts_a_tab_opened_by_plugin_code_by_its_key_with_the_toggles_that_are_on`                                                                                                              | `plugin_log_tab_spawn`                                                                           | wrap: partial (soft wrap starts on)                                                |
| `view.backgroundTasks`                          | `workbench::plugin_contributions::tests::cancelling_a_plugins_task_from_the_background_tasks_popup_stops_it`                                                                                                                | `plugin_cancel_task` → `cancel_task` (same)                                                      | done                                                                               |
| `debug.pluginStart`                             | `…::a_plugins_debug_request_runs_under_the_debugger_and_the_plugin_hears_the_end`, `…::debug_plugin_configuration_runs_only_while_a_project_is_open`                                                                        | `plugin_prepare_debug_run` → `prepare_debug_run`                                                 | partial: no JDWP branch; native runs it inside an async task, not `spawn_blocking` |
| `plugin.<id>.<cmd>`                             | the status-item, notification, Tools-menu and context-menu tests                                                                                                                                                            | `plugin_run_command` → `host.run_command` (now on the blocking pool, as the adapter)             | done; editor and tab menus have no native host                                     |

- **Not registered:** `plugins.updateAll`, `plugins.log.find`, `run.pluginStart`, the IntelliJ imports.
- **Context keys:** `projectOpen` is read both ON and OFF in `debug_plugin_configuration_runs_only_while_a_project_is_open`.
- **Events:** `settings-changed`, `file-saved`, `input-mode-changed`, `action-performed`, the activation events, `editor-focused` and `vcs-changed` are each covered by a named test.
- **Extension-point rows (`point.registerToolWindow:…` and the like):** not checked; the old map file in the scratch folder predates this work. Plugin views and tool windows are measured as not handled (they reach the log-only arm).
- **Spec counts:** about 28 tests cite `plugins.test.tsx` (60 cases) and 10 cite `settings.test.tsx` (12 cases). These are rough counts, and the citations include older `plugins_page.rs` tests.

### 3. Proofs (the task's a–f)

- **a. Keychain.**
  - Code: `KeychainSecrets` and its `set_secret_store` call are `#[cfg(all(target_os = "macos", not(test)))]` in `app-gpui/src/plugins.rs`.
  - Measured with `nm -C` on the final test binary: `KeychainSecrets` 0 symbols, `security_framework::passwords` 0.
  - Test: `the_app_plugin_host_keeps_secrets_in_a_file_under_the_temporary_home`.
  - The binary still imports `_SecKeychain*GenericPassword` and `_SecItem*`. Hypothesis: they come from `ide-ssh`'s keyring (`apple_native_keyring_store`: 30 symbols). Check with `nm -m <bin> | grep SecKeychainAdd` plus a symbol-to-crate map.
  - By grep, `store_ssh_secret` and `stage_ssh_secret` are reached only from model tests in `staging.rs`, never from a test window.
- **b. Secrets.**
  - The `secret_field` tests pass as written (painted text is bullets, and the clipboard sentinel survives Copy and Cut).
  - Grep: no `dbg!`, `println!` or `eprintln!` in the new files. The only derived `Debug` types in them hold no secret; `SecretField` and `PluginsPageEvent` have hand-written ones.
  - I fixed `plugin_runtime.rs`: it logged `{other:?}` of a whole `PluginEvent`; it now logs the kind only.
  - The webview's prompts and settings pages have no password inputs (grep).
- **c. Isolation.**
  - Grep of the tests: no `$HOME`, `home_dir` or `/Users` paths; the only absolute path is `/bin` for `bash`.
  - The Wasm compile cache is `<plugins_dir>/.cache` inside the temp home. `intellij.rs` uses `home_dir`, but nothing calls it.
  - Two compiled `plugins.rs` tests linked the repository folders directly; they now link copies.
  - `OPENIDEA_GPUI_TEST_PLUGIN` is unset in my shell.
- **d. Debug requests:** `a_plugins_debug_request_runs_under_the_debugger_and_the_plugin_hears_the_end`, on the existing fake adapter.
- **e. Conditions and focus.**
  - Only `debug.pluginStart` has a `when` (`projectOpen`, from `actions.json`); ON and OFF are tested.
  - Keys without focus: `prompt_dialog::tests::the_prompt_keys_do_nothing_while_the_prompt_has_no_keyboard` and the new `background_tasks_view::tests::escape_does_nothing_while_the_popup_has_no_keyboard`.
- **f. Gutter clicks:** new `a_gutter_icon_click_neither_moves_the_caret_nor_toggles_a_breakpoint`.
  - It uses the `fakedap` language, so the gutter really takes breakpoints; a press beside another line sets one first, as a control.
  - Mutation check: with `occlude`, `prevent_default` and `stop_propagation` removed, it fails with an extra breakpoint at 0..5.

### 4. Product fixes beyond the notes

- **Test hosts locked (`plugins.rs` `lock_test_hosts`, `test_support::PluginHome`):** the host's editor providers, run detection and syntax are process-wide, so parallel tests answered each other's windows.
- **`plugins.rs` `host_blocking`:** plugin commands now run on the backend's `spawn_blocking`, as the adapter does (`src-tauri/src/commands/plugin.rs:354`), in all three command paths. On the GPUI test executor a 30-second command blocked the window from hearing `TaskStarted`.
- **`settings_page.rs` `sync_fields`:** a field's text is kept while it still stands for the shown value, so `7.` and `7.9` can be typed into a number field (`settings.tsx:100`). Only the staged value (7) is proven; what the field shows afterwards is not checked against the webview.
- **`git_forms.rs`:** redacted `Debug` for the form types, with test `the_debug_output_of_a_form_never_holds_a_typed_url_or_its_credentials`.
- **Notes that were false:**
  - `reload` of a broken manifest returns Ok and the plugin is listed as failed, so the test was rewritten.
  - The notes say to run `cargo fmt`; the task forbids it.
  - `hello` needs engine `^1.3` for `submenu`.
  - The host keeps the first settings it is told as each plugin's baseline; the two settings tests now wait for that before changing a setting.
  - `simulate_input` types one character at a time; the oversized-value test now pastes in one edit.
  - The cancel tests could pass when their 30-second sleep simply ran out; it is now 600 s.

### 5. Left out, and what a Bazel-plugin user would still miss

- **Drafted tests left out:** none.
- **Next integrator:** the undeclared `context_menus.rs` imports `start_with_plugins`, which now returns `PluginHome`.
- **Bazel plugin.** Read from the code, not by loading the Bazel plugin:
  - Works natively: commands as actions, `workspaceContains:` activation, run configurations, and the settings page.
  - Measured, missing: plugin tool windows and views (`ViewChanged`, `RevealView` and `contributes.views` have no native handler, by grep), `OpenTerminal`, run targets, Run Anything, and the JDWP debug branch.
  - Hypothesis: a `file-saved` activation fires only after a dirty save. Confirm by saving a clean file in the a_saved_file test.
