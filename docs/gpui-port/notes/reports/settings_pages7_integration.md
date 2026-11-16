# Settings › Tools › Agents (`tools.agents`): integration notes

Written without a compiler. The Built-in tab is whole. The ACP Registry tab is NOT STARTED: the page draws no registry tab, so it has no network path, no installer, and no Remove. New files (untracked):

- `app-gpui/src/settings_view/pages/tools_agents.rs`: the page view `AgentsPageView`, plus the `AgentLister` seam (`AgentListing` global, `lister`, `backend_lister`).
- `app-gpui/src/settings_view/pages/tools_agents/proofs36_agents_view.rs`: view tests. The page file declares it itself (`#[cfg(test)] mod proofs36_agents_view;`).
- `app-gpui/src/workbench/settings_agents.rs`: `Workbench::write_external_settings`, which carries out Apply for values kept outside the settings.
- `app-gpui/src/workbench/tests/proofs36_settings_agents.rs`: end-to-end tests.

## (a) Hunks, in order (one group: the page lands whole)

### 1. `app-gpui/src/settings_view/pages/mod.rs`: the module

Anchor:

```rust
pub mod tools_actions_on_save;
```

Add after it:

```rust
pub mod tools_agents;
```

### 2. `app-gpui/src/settings_model/staging.rs`: which pages have staged external values

Anchor (in `impl SettingsDialog`):

```rust
    pub fn staged_external(&self, page: &str, key: &str) -> Option<&Value> {
        let id = external_key(page, key);
        self.staged_external.iter().find(|(staged, _)| *staged == id).map(|(_, change)| &change.value)
    }
```

Add after it:

```rust

    /// The pages with values kept outside the settings staged.
    pub fn staged_external_pages(&self) -> impl Iterator<Item = &str> {
        self.staged_external.iter().map(|(_, change)| change.page.as_str())
    }
```

### 3. `app-gpui/src/settings_view/dialog.rs`: a staged external value marks its page modified, so the dialog's Reset shows

This matches the webview (`src/features/settings/dialog/SettingsDialog.tsx:239` adds `stagedExternal` pages to the modified set). Without this hunk, `reset_visible` stays false on the Agents page and `page_reset_discards_edits_…` fails.

Anchor:

```rust
        let mut pages: BTreeSet<String> = dialog.staged().map(|change| change.page.clone()).collect();
        if dialog.has_staged_secrets() {
            pages.insert(SSH_SETTINGS_PAGE.to_owned());
        }
```

Add after it, before `pages`:

```rust
        pages.extend(dialog.staged_external_pages().map(str::to_owned));
```

### 4. `app-gpui/src/settings_view/custom_controls.rs`: mount the page in `install()`

Anchor:

```rust
    let view = pages::build_maven::page(binding(pages::build_maven::PAGE), toolchains.clone(), cx);
    slots.page_view(pages::build_maven::PAGE, &view, cx);
```

Add after it:

```rust

    // Tools › Agents: the built-in agents' executables and environments, staged until Apply; the
    // agents are read from the window's project when the page is shown.
    let agents_binding = binding(pages::tools_agents::PAGE);
    let lister = pages::tools_agents::lister(install.window_label, cx);
    let view = cx.new(|cx| pages::tools_agents::AgentsPageView::new(agents_binding, lister, cx));
    slots.page_view(pages::tools_agents::PAGE, &view, cx);
```

`slots.page_view` answers the page's `ControlEvent`s: Submit, Cancel, and `Browse { directories: false }` through the system file chooser.

### 5. `app-gpui/src/workbench/mod.rs`: the child module

Anchor:

```rust
mod services_window;
mod settings_feature_pages;
```

Replace with:

```rust
mod services_window;
mod settings_agents;
mod settings_feature_pages;
```

### 6. `app-gpui/src/workbench/mod.rs` (`mod tests`): the end-to-end tests

Anchor (the Vim page's integration added this line, which was in the working tree at writing time):

```rust
    mod proofs35_settings_vim;
```

Add after it (else after `    mod proofs32_settings_templates_fixtures;`):

```rust
    mod proofs36_settings_agents;
```

Note: while these notes were written, another agent's uncommitted edits touched `dialog.rs`, `settings_window.rs`, `pages/mod.rs` and `workbench/mod.rs`. Every anchor quoted here was re-checked against the working tree after those edits and is still present verbatim.

### 7. `app-gpui/src/workbench/settings_window.rs`: Apply writes the external values

7a. Anchor:

```rust
const NOT_CARRIED_OUT: &str = "This window cannot do this";
```

Replace with:

```rust
pub(super) const NOT_CARRIED_OUT: &str = "This window cannot do this";
```

7b. Anchor (end of `apply_settings`):

```rust
            while let Some(write) = view.apply_next_secret(keychain, cx) {
                let stored = store_ssh_secret(&write, cx);
                view.apply_secret_done(stored, cx);
            }
            // No page of this window stages a value kept outside the settings; one that did would
            // need its writer here, so it is refused rather than reported as saved.
            while let Some((id, _)) = view.apply_next_external(cx) {
                tracing::warn!("Settings: nothing in this window saves {id}");
                view.apply_external_done(Err(NOT_CARRIED_OUT.to_owned()), cx);
            }
            view.finish_apply(cx);
        });
    }
```

Replace with:

```rust
            while let Some(write) = view.apply_next_secret(keychain, cx) {
                let stored = store_ssh_secret(&write, cx);
                view.apply_secret_done(stored, cx);
            }
        });
        // The values kept outside the settings (the agents' executables) are written on the
        // backend's runtime; that ends the Apply.
        self.write_external_settings(dialog, cx);
    }
```

Also update the doc of `apply_settings` ("stores the staged SSH credentials and the values kept outside the settings") if wanted. It still reads true.

`write_external_settings` ends the Apply in the same turn when nothing external is pending, so every existing Apply keeps its timing. It refuses an id that no page of this window writes with `NOT_CARRIED_OUT`, as the removed loop did.

## (b) Parity rows

| key                                                                                  | expected          | proving tests / what is missing                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| ------------------------------------------------------------------------------------ | ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `settings:tools.agents` (new key, named like `settings:tools.agents.chat`)           | partial           | `workbench::tests::proofs36_settings_agents::stages_a_binary_path_and_writes_it_only_on_apply_or_ok`, `…::writes_the_agents_environment_variables_on_apply_and_shows_them_again`, `…::cancel_discards_command_edits_without_writing`, `…::clear_and_reset_stage_automatic_detection_until_apply`, `…::page_reset_discards_edits_and_returning_to_the_saved_value_clears_the_modified_state`, `…::keeps_edits_across_page_navigation_and_applies_with_agents_mounted`, `…::retains_a_failed_command_for_retry_and_shows_only_the_applied_badge`, `…::an_environment_that_is_not_name_value_pairs_is_refused_and_stays_staged`, `…::lists_built_ins_and_writes_two_independent_edits_on_ok`, `…::stages_a_browsed_executable`, `…::reports_when_providers_cannot_be_read_no_trusted_project`, `…::the_agents_page_lists_from_the_test_home_and_reaches_no_registry`. Missing: the whole ACP Registry tab (search, Install / Update / Remove, no-checksum confirmation, progress and Cancel, background poll, links). |
| `unit:src/features/agents/agentRegistry.test.tsx`                                    | partial, 25 of 26 | The previous 25 model cases stay (`agents::registry::tests::*` 13, `agents::builtin_agents::tests::*` 12). The view and e2e tests above add view-level proofs for the Built-in cases. Missing: "shows the built-in Copilot name and icon in its registry row" (registry tab not built). Model test names that differ from the spec's snake case: `reports_when_providers_cannot_be_read` (spec adds "(no trusted project)"; the view test `settings_view::pages::tools_agents::proofs36_agents_view::reports_when_providers_cannot_be_read_no_trusted_project` has the full name); `stages_a_binary_path_and_writes_it_only_on_button`, `action_stages_automatic_detection_until_apply`, `lists_built_ins_and_writes_two_independent_edits_on_button`, `keeps_reason_built_ins_visible_and_read_only` are the `it.each` names with the placeholder written out as a word. The integrator decides whether that counts.                                                                                              |
| `ipc:agent_set_command`                                                              | done              | `proofs36_settings_agents::stages_a_binary_path_and_writes_it_only_on_apply_or_ok`: `<config>/agents/claude.toml` holds `command = "<typed path>"` after Apply, a file only `ops::agent::set_command` (which `agent_set_command` calls with the window label, `src-tauri/src/commands/agent.rs:43-50`) writes. Also `clear_and_reset_…` (the file is removed for `None`) and `retains_a_failed_command_…` (the backend's refusal is shown).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `ipc:agent_set_env`                                                                  | done              | `proofs36_settings_agents::writes_the_agents_environment_variables_on_apply_and_shows_them_again`: the file has `[env]` with both pairs, then is removed when the field is emptied (`ops::agent::set_env`, adapter `src-tauri/src/commands/agent.rs:56-63`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `ipc:agent_providers`                                                                | already done      | Also exercised by every test above (the page lists through `ops::agent::providers`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `ipc:agent_registry_list`, `ipc:agent_registry_install`, `ipc:agent_registry_remove` | not started       | The Registry tab is not built.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |

The unit row, case by case. `R` = `agents::registry::tests::`, `B` = `agents::builtin_agents::tests::`, `V` = `settings_view::pages::tools_agents::proofs36_agents_view::`, `E` = `workbench::tests::proofs36_settings_agents::`.

| spec case                                                                                      | native test                                                                                                                                                    |
| ---------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| matches every word of the query against name, id, description and authors                      | R `matches_every_word_of_the_query_against_name_id_description_and_authors`                                                                                    |
| offers install, update and remove by state                                                     | R `offers_install_update_and_remove_by_state`                                                                                                                  |
| lists installed agents, then well-known ones, then the rest by name                            | R `lists_installed_agents_then_well_known_ones_then_the_rest_by_name`                                                                                          |
| drops a removed agent this system cannot install again                                         | R `drops_a_removed_agent_this_system_cannot_install_again`                                                                                                     |
| shows registry versions and updates, then Add Agent…                                           | R `shows_registry_versions_and_updates_then_add_agent`                                                                                                         |
| shows the built-in Copilot name and icon in its registry row                                   | MISSING (registry tab not built)                                                                                                                               |
| lists registry agents, says they are third-party, and searches them                            | R `lists_registry_agents_says_they_are_third_party_and_searches_them` (model only)                                                                             |
| installs with progress, then reports the change                                                | R `installs_with_progress_then_reports_the_change` (model only)                                                                                                |
| asks before installing an agent whose download has no checksum                                 | R `asks_before_installing_an_agent_whose_download_has_no_checksum` (model only)                                                                                |
| installs an agent with a checksum without asking                                               | R `installs_an_agent_with_a_checksum_without_asking` (model only)                                                                                              |
| shows why an install failed, cancels one on Cancel, and leaves it running when the page closes | R `shows_why_an_install_failed_cancels_one_on_cancel_and_leaves_it_running_when_the_page_closes` (model only)                                                  |
| follows an install started before the page opened until it ends                                | R `follows_an_install_started_before_the_page_opened_until_it_ends` (model only)                                                                               |
| removes an installed agent                                                                     | R `removes_an_installed_agent` (model only)                                                                                                                    |
| falls back to the saved list with a warning, and reports a registry it cannot load             | R `falls_back_to_the_saved_list_with_a_warning_and_reports_a_registry_it_cannot_load` (model only)                                                             |
| follows locale changes in tabs and renders login commands as inline code                       | B, V `follows_locale_changes_in_tabs_and_renders_login_commands_as_inline_code`                                                                                |
| reports when providers cannot be read (no trusted project)                                     | B `reports_when_providers_cannot_be_read`, V and E `reports_when_providers_cannot_be_read_no_trusted_project`                                                  |
| stages a binary path and writes it only on %s (Apply, OK)                                      | B `stages_a_binary_path_and_writes_it_only_on_button`, E `stages_a_binary_path_and_writes_it_only_on_apply_or_ok`                                              |
| writes the agent's environment variables on Apply and shows them again                         | B and E `writes_the_agents_environment_variables_on_apply_and_shows_them_again`                                                                                |
| Cancel discards command edits without writing                                                  | B and E `cancel_discards_command_edits_without_writing`                                                                                                        |
| %s stages automatic detection until Apply (clear, Reset)                                       | B `action_stages_automatic_detection_until_apply`, V `reset_stages_automatic_detection_until_apply`, E `clear_and_reset_stage_automatic_detection_until_apply` |
| page Reset discards edits and returning to the saved value clears the modified state           | B and E `page_reset_discards_edits_and_returning_to_the_saved_value_clears_the_modified_state`                                                                 |
| keeps edits across page navigation and applies with Agents mounted=%s                          | B and E `keeps_edits_across_page_navigation_and_applies_with_agents_mounted` (E covers mounted=true; B covers both)                                            |
| retains a failed command for retry and shows only the applied badge                            | B and E `retains_a_failed_command_for_retry_and_shows_only_the_applied_badge`                                                                                  |
| lists built-ins and writes two independent edits on %s                                         | B `lists_built_ins_and_writes_two_independent_edits_on_button`, E `lists_built_ins_and_writes_two_independent_edits_on_ok`                                     |
| keeps %s built-ins visible and read-only (packageRunner, registry)                             | B `keeps_reason_built_ins_visible_and_read_only`, V `keeps_package_runner_built_ins_visible_and_read_only`, `keeps_registry_built_ins_visible_and_read_only`   |
| stages a browsed executable                                                                    | B, V and E `stages_a_browsed_executable`                                                                                                                       |

`agentSetCommand` and `agentSetEnv` are called only from `src/features/agents/registry.ts:48-53` (grep of `src/`, `bindings.ts` aside), so the two `ipc:` rows are this page's alone.

View tests (`settings_view::pages::tools_agents::proofs36_agents_view::`): `follows_locale_changes_in_tabs_and_renders_login_commands_as_inline_code`, `reports_when_providers_cannot_be_read_no_trusted_project`, `an_executable_typed_is_staged_for_the_agent_selected`, `the_environment_typed_is_staged_and_never_listed`, `keeps_package_runner_built_ins_visible_and_read_only`, `keeps_registry_built_ins_visible_and_read_only`, `an_agent_is_selected_with_the_mouse_or_the_keyboard`, `reset_stages_automatic_detection_until_apply`, `stages_a_browsed_executable`, `a_file_chosen_after_another_agent_was_selected_is_dropped`, `the_agents_are_read_each_time_the_dialog_shows_the_page`, `enter_and_escape_in_a_field_ask_for_ok_and_cancel`.

## (c) Webview behaviours

| behaviour                                                                                                                           | source                                               | state                                                                                                                                                                                             |
| ----------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Two tabs, Built-in (default) and ACP Registry                                                                                       | `AgentRegistryPage.tsx:62-80`                        | Built-in built; Registry tab left out                                                                                                                                                             |
| Built-in lists providers on mount; keeps only those with `defaultProgram`                                                           | `BuiltinAgents.tsx:32-41`                            | built, model `builtin_agents.rs:185-193`; listed when the dialog shows the page (`follow_page`)                                                                                                   |
| Providers failure: only the error shows                                                                                             | `BuiltinAgents.tsx:46-51`                            | built (`settings-agents-error`)                                                                                                                                                                   |
| Spinner while loading                                                                                                               | `BuiltinAgents.tsx:52`                               | built as a muted `…` (the crate has no spinner)                                                                                                                                                   |
| List rows: icon, name, tag Custom / Not found, muted when missing                                                                   | `BuiltinAgents.tsx:13-26,56-70`, CSS `.agentMissing` | built (`settings-agents-row-N`), plus Up/Down/Home/End from the list control                                                                                                                      |
| Details: icon, name, status dot + text, launch command with tooltip                                                                 | `BuiltinAgents.tsx:112-134`                          | built                                                                                                                                                                                             |
| Executable field, placeholder `Auto-detect (…)`, staged                                                                             | `BuiltinAgents.tsx:89-95,141-148`                    | built                                                                                                                                                                                             |
| Browse… (title `Select <name>`, default path = value)                                                                               | `BuiltinAgents.tsx:149-162`, `registry.ts:44-47`     | built; the chooser takes no title or start folder (`PathPromptOptions` has neither), as on the Python page; a chooser failure is logged by `custom_controls::answer`, not shown under the details |
| Reset icon while the field has a value                                                                                              | `BuiltinAgents.tsx:163`                              | built (`settings-agents-reset`)                                                                                                                                                                   |
| Hints                                                                                                                               | `BuiltinAgents.tsx:165-167,195-197`                  | built                                                                                                                                                                                             |
| Read-only note by reason                                                                                                            | `BuiltinAgents.tsx:169-178`                          | built                                                                                                                                                                                             |
| Environment field (not for registry/custom agents), `NAME=value; …` parsed on Apply with "Expected NAME=value: …"                   | `BuiltinAgents.tsx:96-103,180-199,220-231`           | built                                                                                                                                                                                             |
| Sign in section with backtick code spans, translated                                                                                | `BuiltinAgents.tsx:200-209,239-244`                  | built (`hint_flow`)                                                                                                                                                                               |
| Fields disabled while saving                                                                                                        | `BuiltinAgents.tsx:145,190`                          | built (read-only during Apply)                                                                                                                                                                    |
| Refresh when a previous instance applied                                                                                            | `BuiltinAgents.tsx:105-111`                          | built differently: one instance per dialog, and the writer hands it the agents after each write                                                                                                   |
| Apply/OK write each staged value; failure retained with `id: message`                                                               | `staging.ts:458-474`                                 | built (`settings_agents.rs`)                                                                                                                                                                      |
| Reset (dialog) discards the page's staged externals; page marked modified                                                           | `staging.ts:340-348`, `SettingsDialog.tsx:239`       | built, needs hunks 2-3                                                                                                                                                                            |
| Cancel discards                                                                                                                     | staging close                                        | built (`SettingsDialog::close` clears `staged_external`)                                                                                                                                          |
| `onChanged` after a write                                                                                                           | `BuiltinAgents.tsx:93,101`                           | not needed: `index.ts:30-38` registers the component, and `SettingsDialog.tsx:188` renders it as `<Body />` with no props, so `onChanged` is undefined in the app                                 |
| Registry: search, notice, warning, load error, loading, empty texts                                                                 | `AgentRegistryPage.tsx:191-244`                      | left out                                                                                                                                                                                          |
| Registry rows: icon, version, distribution/no-checksum/update/installed badges, description, authors, license, links (http(s) only) | `AgentRegistryPage.tsx:248-342`                      | left out                                                                                                                                                                                          |
| Install with no-checksum confirm, progress, Cancel, one job at a time                                                               | `AgentRegistryPage.tsx:146-176,304-309`              | left out (model ready: `agents/registry.rs`)                                                                                                                                                      |
| Remove                                                                                                                              | `AgentRegistryPage.tsx:178-189`                      | left out                                                                                                                                                                                          |
| Poll every 1.5 s while installing in the background                                                                                 | `AgentRegistryPage.tsx:106-132`                      | left out                                                                                                                                                                                          |
| Registry list cancelled on close; install keeps running                                                                             | `AgentRegistryPage.tsx:100-103`                      | left out                                                                                                                                                                                          |

How the dialog treats each tab in the webview: Built-in values are staged externals (Apply/OK write, Reset/Cancel discard: `context.ts:52-67`, `staging.ts:329-348,458-474`). The Registry tab stages nothing and acts at once (`AgentRegistryPage.tsx:146-189`), so Apply, Reset and Cancel do not touch it.

## (d) Hypotheses (settle when compiling)

1. A glob `use super::*;` in `proofs36_agents_view.rs` brings in the parent's private `use` imports (`Rc`, `PathBuf`, `App`, `Then`, `TextField`, `Localization`, `AgentProvider`, `BuiltinAgents`, `Focusable`). Settle: compile; add explicit imports if not.
2. `cx.update_default_global::<Localization, _>` (trait `BorrowAppContext`) works on `&mut App` and on `&mut Context<AgentsPageView>`, and the closure may borrow `self` while it holds `cx`. Read in `gpui-pre-0.3.12/src/gpui.rs:301-330` (blanket impl over `BorrowMut<App>`). Settle: compile.
3. `AsyncApp::update` returns `R` directly (read at `async_context.rs:176`). `WeakEntity::update` / `read_with` return `Result` (read at `entity_map.rs:883-918`). Strong `Entity::update` with `AsyncApp` returns `R`.
4. `toml::to_string` writes `command = "<path>"` and `[env]` entries as `KEY = "value"`. The e2e tests compare with `format!("command = {path:?}")`, which holds for temp paths without backslashes or quotes. Settle: print a failing file.
5. An unreadable `claude.toml` (`command = [`) is skipped by `provider::load` (it warns) and makes `set_builtin_command` fail (`provider.rs:238-240` reads it with `CustomAgent::read`). This is read, not run. Settle: run `retains_a_failed_command_…`.
6. Removing a staged value's file when only built-in values remain (`provider.rs:254-258`, then removal) is assumed from `set_builtin_env_writes_and_clears_the_environment` (`provider.rs:607-621`), which asserts it.
7. `cmd-a` selects all in a `TextField` in tests on macOS (used by `proofs19_settings_misc::type_into`).
8. Clicking `settings-agents-executable` (a plain div around the field) gives the field the keyboard, as clicking `settings-text-ssh.host` does.
9. `run_action("file.settings", "tools.terminal")` on an open dialog switches pages (`settings_window.rs:197-207`, read).
10. The window without a project in `reports_when_providers_cannot_be_read_no_trusted_project`: `ops::agent::providers` returns `Err` there (`host.require_trust` → `host.project(window)?`, `ide-host/src/lib.rs:232`). Read, not run.
11. The zh catalog has `Built-in` → `内置` and `Not found` → `未找到`, as `agents::builtin_agents::tests::follows_locale_changes_…` asserts.

## (e) Tests most likely to fail first

- `page_reset_discards_edits_…`, if hunks 2-3 are missing (no `settings-reset`).
- Every e2e test, if hunk 7 is missing: Apply then reports `This window cannot do this`.
- `clear_field` / `clear()` (`cmd-a` + `backspace`): if a TextField takes backspace on a full selection differently, the value stays.
- `an_agent_is_selected_with_the_mouse_or_the_keyboard`: Up/End reach the list only if `claim_keys` on `list_focus` gets them after the row click focused it. The click focuses with `window.focus` inside `on_click`, after mouse-up, so the parent's `track_focus` should not take it back.
- `retains_a_failed_command_…`: the exact toml error text is not asserted, only the `tools.agents|agents.claude.command: ` prefix.
- `the_agents_page_lists_from_the_test_home_and_reaches_no_registry`: assumes opening the project writes no `<config>/agents` folder (hypothesis: only agent files live there, and nothing in a project open writes one). It asserts only `registry.json` and `tools/` under `<local data>/agents`, since the project's task store may live in `<data>/agents/<key>` (`ide-host/src/project.rs:687`).

## (f) The real window

Showing Tools › Agents in the real binary calls `agent_providers` for the window. That reads the agent files in `<config>/agents/*.toml` (`provider.rs:336`) and the saved registry index `<local data>/agents/registry.json` (`registry.rs:346`, for update marks). It also checks whether each built-in command is an executable file in a `PATH` directory (`provider.rs:386-398`), resolving through the cached login-shell PATH (`ide-jsonrpc/src/lib.rs:377`). It starts no program, opens no network connection and writes nothing. Typing stages only. Apply writes `<config>/agents/<id>.toml`. Browse… opens the system's file chooser. With no Registry tab, no view of this page reaches the network or installs anything.

It is NOT safe to screenshot over the user's real home. The page opens with Claude Code selected and draws its environment field in plain text, as the webview does (`BuiltinAgents.tsx:186-193`). That field is the `[env]` of the user's own `<config>/agents/claude.toml`, which the existing model treats as holding API keys (`builtin_agents.rs:157-158`). The values are stored as plaintext TOML in `<config>/agents/<id>.toml` (`provider.rs:228-230,323-331`), not in the keychain. A refused environment also echoes the offending pair into the dialog footer ("Expected NAME=value: …"). Look at the page, or capture it, only over a temporary home, or with no `[env]` in the user's agent files. The owner must decide between matching the webview (plain field, as written) and a masked field.

Check in the real window: the split's 640 px minimum width against the dialog's content width, the icon colours, and that the zh labels show.

## (g) What to share once all compile

The Agents page, SSH Configurations and Deployment share four things:

- A master list beside a details pane (SSH and Deployment draw their own list with a toolbar; Agents has no toolbar).
- A seam global that test builds replace (`SshPageChecks`, `ConnectionCheck`, `AgentListing`).
- Values kept outside the settings, written on Apply through the backend: SSH secrets through `next_secret`, agents through `next_external`.
- `field_box` versus `frame` (fixed width).

A shared "list + details" frame and one `Then`-based backend seam helper (`on_backend` in `ssh_backend.rs` versus `backend_lister`) are worth extracting. The external-value writer in `settings_agents.rs` should become the one place Apply carries out page-specific writes, keyed by page id.

## Findings in existing code

- These all print an agent's environment (its API keys) through `Debug`. `BuiltinAgents` hides it; these do not:
  - `ExternalWrite` derives `Debug` (`agents/builtin_agents.rs:81`).
  - `Details` derives `Debug` (`agents/builtin_agents.rs:126`).
  - `ExternalChange` derives `Debug` (`settings_model/staging.rs:99`).
  - `SettingsDialog`'s manual `Debug` prints `staged_external` and `external_applied` in full (`settings_model/staging.rs:251-252`).
  - `ApplyRun` derives `Debug` with its `external` list (`settings_model/staging.rs:604`).
- `modified_pages` ignores staged external values (`settings_view/dialog.rs:647-654`), unlike the webview (`SettingsDialog.tsx:239`). Fixed by hunks 2-3.
- `apply_settings` refused every external value (`settings_window.rs:333-338`), so the existing `BuiltinAgents` model could never be applied in a window. Fixed by hunk 7.
- The brief's GPUI path `gpui-pre-0.3.12` is right (`gpui-kit 0.9.1` depends on it).
