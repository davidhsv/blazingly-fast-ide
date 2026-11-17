# Agents › Coding Agents (`agents.list`): integration notes

Written without a compiler at HEAD `e123f0da5` (the brief says `f8d8ccb57`; `git diff --stat f8d8ccb57 HEAD` shows four files under `docs/gpui-port/notes/`, nothing else). Every existing file under `app-gpui/` was read with `git show HEAD:`. Statements not checked by reading code are marked "Hypothesis:".

## New files (untracked)

- `app-gpui/src/agents/agent_list/values.rs`: the launch values, New Tasks and custom-agent helpers, re-exported by `agent_list.rs`.
- `app-gpui/src/agents/agent_list.rs`: the list model `AgentList` (rows, sections, search, tags, muting, selection, details, confirmations, import report), the launch values (`parse_env`, `AgentWrite`, `agent_write`, `stored_value`), New Tasks (`is_per_model_option`, `default_options`, `chosen_defaults`, `defaults_with`, `new_task_options`), custom agents (`slugify_id`, `parse_args`, `import_report`). Declares `#[cfg(test)] mod proofs38_agent_list;`.
- `app-gpui/src/agents/agent_list/proofs38_agent_list.rs`: model tests.
- `app-gpui/src/settings_view/pages/agents_list.rs`: the page view `AgentListPageView`, the backend interface `AgentListBackend` (`Lister`, `RegistryCalls`, `CustomCalls`). Declares `mod custom_agent; mod details; mod master; #[cfg(test)] mod proofs38_agents_list_view;`.
- `app-gpui/src/settings_view/pages/agents_list/master.rs`: the master list, the confirmation panel, `overlay`.
- `app-gpui/src/settings_view/pages/agents_list/details.rs`: the details pane (header, progress, registry facts, Sign In, New Tasks, Launch).
- `app-gpui/src/settings_view/pages/agents_list/custom_agent.rs`: the Add Custom Agent dialog `CustomAgentModal` (its own view, events `Saved` / `Closed`).
- `app-gpui/src/settings_view/pages/agents_list/proofs38_agents_list_view.rs`: view tests.
- `app-gpui/src/workbench/agent_list_backend.rs`: `for_window(label) -> AgentListBackend`, the real calls, and in test builds the fail-closed registry seam with `RegistryStandIn`, `REFUSED`, `stand_in_calls`.
- `app-gpui/src/workbench/settings_agents_list.rs`: `Workbench::write_external_settings`, the Apply writer of the staged launch values (replaces `workbench/settings_agents.rs`).
- `app-gpui/src/workbench/tests/proofs38_settings_agents_list.rs`: end-to-end tests.

## (a) Hunks for existing files, in order

### Part A (list, details, Sign In, Launch, staging, Apply)

A1. `app-gpui/src/agents/mod.rs`. Anchor:

```rust
pub mod builtin_agents;
```

Add before it:

```rust
pub mod agent_list;
```

A2. `app-gpui/src/settings_view/pages/mod.rs`. Anchor:

```rust
pub mod appearance;
```

Add before it:

```rust
pub mod agents_list;
```

(`tools_agents` is not declared at HEAD, so there is nothing to remove here.)

A3. `app-gpui/src/settings_model/staging.rs`: which pages have staged external values (old draft's hunk 2, anchor verbatim at HEAD). Anchor (in `impl SettingsDialog`):

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

A4. `app-gpui/src/settings_view/dialog.rs`: a staged external value marks its page modified (old hunk 3; `SettingsDialog.tsx:239`). Anchor:

```rust
        let mut pages: BTreeSet<String> = dialog.staged().map(|change| change.page.clone()).collect();
        if dialog.has_staged_secrets() {
            pages.insert(SSH_SETTINGS_PAGE.to_owned());
        }
```

Add after it:

```rust
        pages.extend(dialog.staged_external_pages().map(str::to_owned));
```

A5. `app-gpui/src/settings_model/staging.rs`: the staged environment text never shows in `Debug`. Anchor:

```rust
/// A value kept outside the settings, staged by a page under its own key.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalChange {
    pub page: String,
    pub value: Value,
}
```

Replace with:

```rust
/// A value kept outside the settings, staged by a page under its own key. Its value can be an agent's
/// environment (API keys): `Debug` leaves it out.
#[derive(Clone, PartialEq)]
pub struct ExternalChange {
    pub page: String,
    pub value: Value,
}

impl fmt::Debug for ExternalChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExternalChange").field("page", &self.page).field("value", &"…").finish()
    }
}
```

This also covers `ApplyRun` and `ApplyStart` (they print `ExternalChange` through their derived `Debug`) and `SettingsDialog`'s `staged_external` line.

A6. `app-gpui/src/settings_model/staging.rs`, in `impl fmt::Debug for SettingsDialog`. Anchor:

```rust
            .field("external_applied", &self.external_applied)
```

Replace with:

```rust
            .field("external_applied", &self.external_applied.keys().collect::<Vec<_>>())
```

A7. `app-gpui/src/workbench/settings_window.rs` (old hunk 7, anchors verbatim at HEAD). 7a, anchor:

```rust
const NOT_CARRIED_OUT: &str = "This window cannot do this";
```

Replace with:

```rust
pub(super) const NOT_CARRIED_OUT: &str = "This window cannot do this";
```

7b, anchor (end of `apply_settings`):

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
        // The values kept outside the settings (the agents' launch values) are written on the backend's
        // runtime; that ends the Apply.
        self.write_external_settings(dialog, cx);
    }
```

A8. `app-gpui/src/workbench/mod.rs`: the child modules. Anchor:

```rust
mod agents;
```

Add after it:

```rust
pub(crate) mod agent_list_backend;
```

Anchor:

```rust
mod settings_feature_pages;
```

Add before it:

```rust
mod settings_agents_list;
```

A9. `app-gpui/src/workbench/mod.rs`, inside `mod tests`. Anchor:

```rust
    mod proofs35_settings_vim;
```

Add after it:

```rust
    mod proofs38_settings_agents_list;
```

A10. `app-gpui/src/settings_view/custom_controls.rs`: mount the page in `install()`. Anchor:

```rust
    slots.page_view(pages::build_maven::PAGE, &view, cx);
```

Add after it (the first one; the anchor is unique in `install()`):

```rust

    // Agents › Coding Agents: every agent, its launch values staged until Apply, installs, removals and
    // custom agents acting at once; the agents and the registry are read when the page is shown.
    let agents = binding(pages::agents_list::PAGE);
    let backend = crate::workbench::agent_list_backend::for_window(install.window_label);
    let view = cx.new(|cx| pages::agents_list::AgentListPageView::new(agents, backend, cx));
    slots.page_view(pages::agents_list::PAGE, &view, cx);
```

`slots.page_view` answers the page's `ControlEvent`s (Submit, Cancel, `Browse { directories: false }` for the executable). To land Parts A and B alone, mount with `AgentListBackend { registry: None, custom: None, ..crate::workbench::agent_list_backend::for_window(install.window_label) }`: the page then draws no registry rows, no registry line and no custom buttons. In that case `agent_list_backend.rs` still compiles whole (it is one file); Part A's tests that name `RegistryStandIn` still pass, since the stand-in is never called.

### Part B (New Tasks)

Part B has no switch of its own: the New Tasks section is drawn whenever the page is, so A and B land together. No hunk: `agents.defaults` is staged with `Binding::stage` like any setting. Hypothesis: `default_settings()` has an `agents.defaults` key or tolerates staging one (`SettingsDialog::stage` compares with `get_at(base, key)`, which is `None` for a missing key, so the edit is staged). Settle: run `stages_the_settings_new_tasks_start_with` (view) and check Apply writes `[agents.defaults.claude]` (no e2e test for the write yet, see (e)).

### Part C (custom agents)

No hunk beyond A10 (mounted with `custom: Some(..)`, which `for_window` gives).

### Part D (registry)

No hunk beyond A10 (mounted with `registry: Some(..)`). The app build calls `ide_host::ops::agent::registry_list`, `registry_install`, `registry_remove` and `Host::cancel_request` (`agent_list_backend.rs`, `#[cfg(not(test))] fn registry_calls`); a test build compiles only the stand-in path (`#[cfg(test)] fn registry_calls`), so it cannot reach them.

### Clean-up (with all four parts)

C1. Delete `app-gpui/src/workbench/settings_agents.rs`, `app-gpui/src/settings_view/pages/tools_agents.rs`, `app-gpui/src/settings_view/pages/tools_agents/proofs36_agents_view.rs`, `app-gpui/src/workbench/tests/proofs36_settings_agents.rs` (none is declared at HEAD). `settings_agents.rs` cannot be kept as it is: it imports `tools_agents::{self, AgentsPageView}` and `builtin_agents::{ExternalWrite, external_write}`; `settings_agents_list.rs` is the same writer over the new page and the new `agent_write`.

C2. `app-gpui/src/agents/builtin_agents.rs`: once the new tests stand, the page model `BuiltinAgents`, `AgentRow`, `Details`, `BuiltinEffect`, `shown`, `command_base`, `ExternalWrite`, `external_write`, `stored_value` and `parse_env` are used by nothing compiled (`git grep -n 'builtin_agents::' HEAD -- app-gpui/src` finds only the two old draft files). Keep `command_key`, `env_key`, `format_env`, `HintPart`, `login_hint` (used by `agent_list.rs`). Removing them takes `builtin_agents/tests.rs` with them; its 12 spec-named tests are replaced by the proofs38 tests below. If they are kept instead, note that `ExternalWrite` and `Details` derive `Debug` with environment values and `parse_env` puts the whole refused pair in its error. Their module doc still says "Settings › Tools › Agents › Built-in".

C3. `docs/gpui-port/parity.json`, key `action:agents.addAgent`: the proof text says "opens Settings on the tools.agents page"; the test it names already asserts `agents.list` (`workbench/tests/proofs_agents.rs:171`). Replace "tools.agents" with "agents.list" in the proof.

## (b) Parity rows

Prefixes: `M` = `agents::agent_list::proofs38_agent_list::`, `V` = `settings_view::pages::agents_list::proofs38_agents_list_view::`, `E` = `workbench::tests::proofs38_settings_agents_list::`, `R` = `agents::registry::tests::` (compiled, unchanged).

| key                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | expected                                                           | proof / what is missing                                                                                                                                                                                                                                                                                                                                                                                                               |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings:agents.list`                                                                                                                                                                                                                                                                                                                                                                                                                                                 | partial                                                            | `E` tests below drive the page in the real dialog: list, details, staging, Apply / OK / Reset / Cancel, custom agents, registry through the stand-in. Missing: a drawn spinner and progress bar role, the registry's listing over the real backend (fail-closed in tests by design), Browse's title, start folder and json filter, the Launch section's Browse error line, the mounted=true variant of navigation (see the unit row). |
| `unit:src/features/agents/agentRegistry.test.tsx`                                                                                                                                                                                                                                                                                                                                                                                                                      | partial, 31 of 33 through the view or the app, 2 on the model only | case by case below                                                                                                                                                                                                                                                                                                                                                                                                                    |
| Keys new to `parity.json` (a grep at HEAD finds none of them): `settings:agents.list`, `ipc:agent_set_command`, `ipc:agent_set_env`, `ipc:agent_custom_save`, `ipc:agent_custom_remove`, `ipc:agent_custom_import`, `ipc:agent_registry_list`, `ipc:agent_registry_install`, `ipc:agent_registry_remove`. Existing keys: `unit:src/features/agents/agentRegistry.test.tsx`, `ipc:agent_providers`, `ipc:fs_read_text`, `ipc:cancel_request`, `action:agents.addAgent`. |

| key                          | expected         | proof / what is missing                                                                                                                                                                                                                                                                                                                                                                   |
| ---------------------------- | ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ipc:agent_set_command`      | done             | `E::stages_a_binary_path_and_writes_it_only_on_apply_or_ok`: `<config>/agents/claude.toml` holds `command = "<typed path>"` after Apply, written only by `ops::agent::set_command` (adapter `src-tauri/src/commands/agent.rs:57-64` calls the same op). Also `E::clear_and_reset_…` and `E::retains_a_failed_command_…`.                                                                  |
| `ipc:agent_set_env`          | done             | `E::writes_the_agents_environment_variables_on_apply_and_shows_them_again`: the agent file names both variables with empty values, no agent file holds a value, the page shows them again after reopening (they come back from the saved secrets), and clearing removes the file. `ops::agent::set_env` as the adapter (`agent.rs:70-77`).                                                |
| `ipc:agent_custom_save`      | done             | `E::adds_a_custom_agent_through_the_modal_suggesting_the_id`: `<config>/agents/my-agent.toml` with the name, command, arguments and an empty-valued variable, written by `ops::agent::save_custom_agent` (adapter `agent.rs:83-86`).                                                                                                                                                      |
| `ipc:agent_custom_remove`    | done             | `E::removes_a_custom_agent_after_asking`: the file goes only after OK (`ops::agent::remove_custom_agent`, adapter `agent.rs:89-93`).                                                                                                                                                                                                                                                      |
| `ipc:agent_custom_import`    | done             | `E::imports_acp_json_and_reports_saved_and_skipped_entries`: the notice text from the backend's outcomes and `my-agent.toml` written (`ops::agent::import_acp_agents`, adapter `agent.rs:96-104`).                                                                                                                                                                                        |
| `ipc:fs_read_text`           | already done     | also used by Import acp.json (`ops::fs::read_text`, as `registry.ts:76`).                                                                                                                                                                                                                                                                                                                 |
| `ipc:agent_providers`        | already done     | every `E` test lists through `ops::agent::providers`.                                                                                                                                                                                                                                                                                                                                     |
| `ipc:agent_registry_list`    | partial          | The app build calls `ops::agent::registry_list` with the page's request id (read, not run). Tests answer through `RegistryStandIn`: `V::falls_back_…`, `E::lists_registry_agents_beside_the_built_ins_and_installs_one_through_the_registry`, and `E::without_a_stand_in_the_page_refuses_to_reach_the_registry` proves the refusal. No test can reach the op itself without the network. |
| `ipc:agent_registry_install` | partial          | Same: `V::installs_with_progress_then_reports_the_change`, `V::shows_why_an_install_failed_…`, `E::lists_registry_agents_…_installs_one_…`; the op is called in the app build only.                                                                                                                                                                                                       |
| `ipc:agent_registry_remove`  | partial          | `V::removes_an_installed_agent`; the op is called in the app build only.                                                                                                                                                                                                                                                                                                                  |
| `ipc:cancel_request`         | stays partial    | `V::shows_why_an_install_failed_cancels_one_on_cancel_…` (through the stand-in).                                                                                                                                                                                                                                                                                                          |
| `action:agents.addAgent`     | done (unchanged) | fix the proof text (C3).                                                                                                                                                                                                                                                                                                                                                                  |

The unit row, case by case (spec line, native test, through the view `V` / the app `E` or the model `M`/`R` only):

| spec case                                                                                               | native test                                                                                                                                                       | level                                                                                                                                                                                                                        |
| ------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| matches every word of the query against name, id, description and authors (`:117`)                      | `R::matches_every_word_of_the_query_against_name_id_description_and_authors`                                                                                      | model (the spec is a model test too)                                                                                                                                                                                         |
| offers install, update and remove by state (`:124`)                                                     | `R::offers_install_update_and_remove_by_state`                                                                                                                    | model (same)                                                                                                                                                                                                                 |
| lists installed agents, then well-known ones, then the rest by name (`:132`)                            | `R::lists_installed_agents_then_well_known_ones_then_the_rest_by_name`                                                                                            | model (same)                                                                                                                                                                                                                 |
| drops a removed agent this system cannot install again (`:150`)                                         | `R::drops_a_removed_agent_this_system_cannot_install_again`                                                                                                       | model (same)                                                                                                                                                                                                                 |
| shows registry versions and updates, then Add Agent… (`:176`)                                           | `R::shows_registry_versions_and_updates_then_add_agent`                                                                                                           | model (same)                                                                                                                                                                                                                 |
| shows the built-in Copilot name and icon in its registry row (`:222`)                                   | `M::shows_the_built_in_copilot_name_and_icon_in_its_registry_row`                                                                                                 | model only: the row's icon name `agent-copilot`, not the drawn SVG                                                                                                                                                           |
| lists registry agents, says they are third-party, and searches them (`:233`)                            | `V::lists_registry_agents_says_they_are_third_party_and_searches_them`                                                                                            | view                                                                                                                                                                                                                         |
| installs with progress, then reports the change (`:253`)                                                | `V::installs_with_progress_then_reports_the_change`; `E::lists_registry_agents_beside_the_built_ins_and_installs_one_through_the_registry`                        | view, app                                                                                                                                                                                                                    |
| asks before installing an agent whose download has no checksum (`:272`)                                 | `V::asks_before_installing_an_agent_whose_download_has_no_checksum`; `M::install_without_a_checksum_asks_and_only_install_goes_ahead`                             | view                                                                                                                                                                                                                         |
| installs an agent with a checksum without asking (`:292`)                                               | `V::installs_an_agent_with_a_checksum_without_asking`                                                                                                             | view                                                                                                                                                                                                                         |
| shows why an install failed, cancels one on Cancel, and leaves it running when the page closes (`:302`) | `V::shows_why_an_install_failed_cancels_one_on_cancel_and_leaves_it_running_when_the_page_closes`                                                                 | view (the page is hidden, not unmounted: one view per dialog)                                                                                                                                                                |
| follows an install started before the page opened until it ends (`:324`)                                | `V::follows_an_install_started_before_the_page_opened_until_it_ends`                                                                                              | view (clock advanced 1.6 s)                                                                                                                                                                                                  |
| removes an installed agent (`:345`)                                                                     | `V::removes_an_installed_agent`                                                                                                                                   | view                                                                                                                                                                                                                         |
| falls back to the saved list with a warning, and reports a registry it cannot load (`:358`)             | `V::falls_back_to_the_saved_list_with_a_warning_and_reports_a_registry_it_cannot_load`                                                                            | view                                                                                                                                                                                                                         |
| follows locale changes and renders login commands as inline code (`:442`)                               | `V::follows_locale_changes_and_renders_login_commands_as_inline_code`; `M::` same name                                                                            | view                                                                                                                                                                                                                         |
| reports when providers cannot be read (no trusted project) (`:475`)                                     | `E::reports_when_providers_cannot_be_read_no_trusted_project`; `V::` same name                                                                                    | app                                                                                                                                                                                                                          |
| stages a binary path and writes it only on %s (Apply, OK) (`:483`)                                      | `E::stages_a_binary_path_and_writes_it_only_on_apply_or_ok` (both buttons in one test)                                                                            | app                                                                                                                                                                                                                          |
| writes the agent's environment variables on Apply and shows them again (`:506`)                         | `E::writes_the_agents_environment_variables_on_apply_and_shows_them_again`                                                                                        | app                                                                                                                                                                                                                          |
| Cancel discards command edits without writing (`:523`)                                                  | `E::cancel_discards_command_edits_without_writing`                                                                                                                | app                                                                                                                                                                                                                          |
| %s stages automatic detection until Apply (clear, Reset) (`:546`)                                       | `E::clear_and_reset_stage_automatic_detection_until_apply`                                                                                                        | app                                                                                                                                                                                                                          |
| page Reset discards edits and returning to the saved value clears the modified state (`:563`)           | `E::page_reset_discards_edits_and_returning_to_the_saved_value_clears_the_modified_state` (needs A3, A4)                                                          | app                                                                                                                                                                                                                          |
| keeps edits across page navigation and applies with Agents mounted=%s (`:577`)                          | `E::keeps_edits_across_page_navigation_and_applies_with_agents_mounted`                                                                                           | app, mounted=false only (Apply from the other page); mounted=true (back to the page before Apply) MISSING                                                                                                                    |
| retains a failed command for retry and shows only the applied badge (`:601`)                            | `E::retains_a_failed_command_for_retry_and_shows_only_the_applied_badge`                                                                                          | app                                                                                                                                                                                                                          |
| lists built-ins and writes two independent edits on %s (Apply, OK) (`:618`)                             | `E::lists_built_ins_and_writes_two_independent_edits_on_ok`                                                                                                       | app, OK only; the Apply variant MISSING (its extra assertion, "Codex Custom" after Apply, is covered by `E::stages_a_binary_path_…` for Claude Code)                                                                         |
| keeps %s built-ins visible and read-only (packageRunner, registry) (`:652`)                             | `V::keeps_package_runner_built_ins_visible_and_read_only`, `V::keeps_registry_built_ins_visible_and_read_only`; `M::keeps_reason_built_ins_visible_and_read_only` | view                                                                                                                                                                                                                         |
| stages a browsed executable (`:678`)                                                                    | `E::stages_a_browsed_executable`; `V::` same name                                                                                                                 | app                                                                                                                                                                                                                          |
| lists registry agents beside the built-ins, updates one and removes it (`:725`)                         | `M::lists_registry_agents_beside_the_built_ins_updates_one_and_removes_it`                                                                                        | model only (the app test lists and installs a registry-only agent; no test updates a registry install in the view, because the backend lists a registry install only from its agent file, which the stand-in does not write) |
| lists every agent, installed first, and filters them (`:751`)                                           | `V::lists_every_agent_installed_first_and_filters_them`; `M::` same name                                                                                          | view                                                                                                                                                                                                                         |
| stages the settings new tasks start with (`:779`)                                                       | `V::stages_the_settings_new_tasks_start_with`; `M::` same name                                                                                                    | view                                                                                                                                                                                                                         |
| says when an agent's settings aren't known yet (`:790`)                                                 | `E::says_when_an_agents_settings_arent_known_yet`; `V::`, `M::` same name                                                                                         | app                                                                                                                                                                                                                          |
| adds a custom agent through the modal, suggesting the id (`:835`)                                       | `E::adds_a_custom_agent_through_the_modal_suggesting_the_id`; `V::`, `M::` same name                                                                              | app                                                                                                                                                                                                                          |
| removes a custom agent after asking (`:860`)                                                            | `E::removes_a_custom_agent_after_asking`; `V::`, `M::` same name                                                                                                  | app                                                                                                                                                                                                                          |
| imports acp.json and reports saved and skipped entries (`:873`)                                         | `E::imports_acp_json_and_reports_saved_and_skipped_entries`; `V::`, `M::` same name                                                                               | app                                                                                                                                                                                                                          |

Count: 33 cases as `inventory.mjs` counts them (an `it.each` is one case; `INVENTORY.md:3285` says 33). Covered by name: 33; through the view or the app: 31; model only: 2 (`:222`, `:725`); two `it.each` rows cover one of their two variants (`:577`, `:618`).

Tests not from a spec: `M::a_refused_environment_names_the_variable_never_its_value`, `M::debug_output_never_prints_an_agents_environment_values`, `M::staged_launch_values_are_written_as_their_commands_write_them`, `M::install_without_a_checksum_asks_and_only_install_goes_ahead`, `V::the_environment_typed_is_staged_and_never_listed`, `V::a_registry_agents_repository_opens_through_the_backend`, `E::an_environment_that_is_not_name_value_pairs_is_refused_by_its_place_and_stays_staged`, `E::without_a_stand_in_the_page_refuses_to_reach_the_registry`.

## (c) Behaviours of the webview page

`AgentList.tsx` unless named otherwise.

| behaviour                                                                                                                                                                                                                 | source                                    | state                                                                                                                                                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Page wrapper, no tabs                                                                                                                                                                                                     | `AgentRegistryPage.tsx:16-20`             | built (one view)                                                                                                                                                                                                                             |
| Providers listed on open; an error replaces the page                                                                                                                                                                      | `:150-159`, `:213-218`                    | built (`list_providers`, `settings-agents-error`)                                                                                                                                                                                            |
| Spinner while providers load                                                                                                                                                                                              | `:219`                                    | built as `…` (no spinner in the crate)                                                                                                                                                                                                       |
| Registry listed every time the page opens, cancelled when it closes                                                                                                                                                       | `:160-171`                                | built: cancelled when the dialog shows another page (`follow_page`) and when the dialog closes (`on_release` of the view)                                                                                                                    |
| Rows: providers in backend order, then registry-only agents sorted; Installed / Available headers; runnable rule                                                                                                          | `:61-64`, `:77-95`                        | built (`AgentList::rows`)                                                                                                                                                                                                                    |
| Search on name and id, or a registry agent's name, id, description, authors; clearable                                                                                                                                    | `:66-71`, `:342-349`, `registry.ts:87-92` | built (search field with a Clear button)                                                                                                                                                                                                     |
| "No agents match" only with a query                                                                                                                                                                                       | `:372-376`                                | built                                                                                                                                                                                                                                        |
| Row: icon, name, tag Update / Custom / Not found, muted name, spinner while installing                                                                                                                                    | `:98-126`, CSS `.agentMissing`            | built (a `…` mark replaces the spinner)                                                                                                                                                                                                      |
| Row heights 28 / 26, master width 240 (min 150), split min height 420                                                                                                                                                     | `:366-367`, CSS `.split`, `.master`       | built                                                                                                                                                                                                                                        |
| List keyboard (active row)                                                                                                                                                                                                | `:361-365` (`List`)                       | built: Up, Down, Home, End over the agents, headers skipped; Escape is the dialog's Cancel                                                                                                                                                   |
| Add Custom Agent…, Import acp.json / Importing…, disabled while busy                                                                                                                                                      | `:378-385`                                | built                                                                                                                                                                                                                                        |
| Notice line                                                                                                                                                                                                               | `:386-390`                                | built                                                                                                                                                                                                                                        |
| Registry state line: loading, warning, "Cannot load the ACP Registry: …"                                                                                                                                                  | `:391-408`                                | built                                                                                                                                                                                                                                        |
| Selection kept by id, else first row                                                                                                                                                                                      | `:328-330`                                | built                                                                                                                                                                                                                                        |
| Details header: 24 px icon, name, dot, facts (`providerStatus`, or version · distribution · authors · license)                                                                                                            | `:508-541`, `:794-799`                    | built                                                                                                                                                                                                                                        |
| Header buttons: Install, Update, Remove (registry), Remove (custom), Removing…, Cancel during own install, nothing for a background install                                                                               | `:543-574`                                | built                                                                                                                                                                                                                                        |
| Progress bar and message                                                                                                                                                                                                  | `:576-581`                                | built (bar drawn from the fraction, empty when unknown; no `progressbar` role)                                                                                                                                                               |
| "<version> is available"                                                                                                                                                                                                  | `:582`                                    | built                                                                                                                                                                                                                                        |
| Per-agent error                                                                                                                                                                                                           | `:583-587`, `:221-225`                    | built                                                                                                                                                                                                                                        |
| Description, no-checksum warning, Repository / Website links (http(s) only, Website when different)                                                                                                                       | `:588-601`, `:957-965`                    | built (links open through the backend seam)                                                                                                                                                                                                  |
| Third-party note                                                                                                                                                                                                          | `:603-610`                                | built                                                                                                                                                                                                                                        |
| Install with confirmation for no checksum, one at a time                                                                                                                                                                  | `:227-259`                                | built (in-page confirmation panel instead of the workbench message dialog; while it is open the list has the keyboard, Enter answers OK, Escape answers Cancel, and Enter / Escape in a field answer it instead of the dialog's OK / Cancel) |
| Remove registry install after asking                                                                                                                                                                                      | `:306-326`                                | built (the compiled `RegistryPage::remove` does not ask; the new model asks before calling it)                                                                                                                                               |
| Remove custom agent after asking                                                                                                                                                                                          | `:261-280`                                | built                                                                                                                                                                                                                                        |
| Import acp.json: chooser titled "Select acp.json" with a json filter, read with `fsReadText`, report                                                                                                                      | `:282-304`, `registry.ts:68-77`           | built; the chooser takes no title or filter (`PathPromptOptions` has `files`, `directories`, `multiple`, `prompt`)                                                                                                                           |
| Background poll every 1.5 s while installs started elsewhere run; one listing at a time                                                                                                                                   | `:180-206`                                | built (GPUI timer); the compiled `RegistryPage` follows the first installing agent only, the webview marks each installing agent (`:331-336`)                                                                                                |
| Sign In section with code spans, translated                                                                                                                                                                               | `:839-845`, `:949-955`                    | built                                                                                                                                                                                                                                        |
| New Tasks: one choice per option with more than one value, per-model options left out, the model's thinking levels added, "Agent's default (…)", values verbatim with details, hint; staging `agents.defaults`            | `:846-848`, `:967-1023`                   | built                                                                                                                                                                                                                                        |
| New Tasks empty text                                                                                                                                                                                                      | `:981-986`                                | built                                                                                                                                                                                                                                        |
| Launch: Command row with the command as tooltip                                                                                                                                                                           | `:849-855`                                | built                                                                                                                                                                                                                                        |
| Executable field, placeholder, hint, Browse… (title, start path), Reset                                                                                                                                                   | `:856-887`                                | built; the chooser takes no title or start folder                                                                                                                                                                                            |
| Browse failure shown under Launch                                                                                                                                                                                         | `:920-924`                                | left out: `ControlEvent::Browse` logs a chooser failure (`custom_controls.rs`, `answer`) and never answers                                                                                                                                   |
| Read-only note by reason                                                                                                                                                                                                  | `:888-897`                                | built                                                                                                                                                                                                                                        |
| Environment field (not for registry or custom agents), `NAME=value; …`                                                                                                                                                    | `:899-918`, `:930-947`                    | built; a refused pair is named by its variable or its place ("entry N"), not by its text: a deliberate difference                                                                                                                            |
| Fields disabled while an Apply runs                                                                                                                                                                                       | `:867`, `:912`                            | built (read-only)                                                                                                                                                                                                                            |
| Refresh after a previous instance applied                                                                                                                                                                                 | `:830-834`                                | built differently: one page view per dialog, and the Apply writer hands it the agents after each write                                                                                                                                       |
| `onChanged` after a change                                                                                                                                                                                                | `:175-178`                                | not needed: the registered page has no `onChanged` (`AgentRegistryPage` is mounted without props)                                                                                                                                            |
| Add Custom Agent dialog: Name (autofocus), ID slugified until edited, Command with Browse… (title "Select <name>"), Arguments split with quotes, Environment, Add disabled until name, id and command, error line, Cancel | `:615-792`                                | built (`CustomAgentModal`); Browse's title is not passed                                                                                                                                                                                     |

## (d) Hypotheses, each with how to settle it

1. Child modules (`master.rs`, `details.rs`, `custom_agent.rs`, the test files) read the parent's private fields, consts and `use` items through `super::`. Settle: compile.
2. Passing `cx: &mut Context<V>` where a stored `Rc<dyn Fn(.., &mut App)>` expects `&mut App` coerces by `DerefMut` (`(calls.list)(request, then, cx)` inside `cx.defer` gets `&mut App` anyway). Settle: compile.
3. `futures::channel::mpsc::UnboundedSender<InstallEvent>` is `Sync`, so the closure given to `ide_host::ops::Channel::new` (which needs `Fn + Send + Sync`) compiles. Settle: compile; else wrap the sender in a `Mutex`.
4. Dropping the modal's `Subscription` from inside its own callback (`add_custom`'s subscriber sets `view.modal = None`) is allowed by GPUI. Settle: run `V::adds_a_custom_agent_…`; else take the modal out in `cx.defer`.
5. Clicking a field's frame (`settings-agents-executable`, `settings-agents-custom-args`) gives the field the keyboard, as `settings-text-ssh.host` does. Settle: run the `E` tests.
6. `window.focus` in Add Custom Agent…'s click handler leaves the Name field focused, so `simulate_input` types the name. Settle: run `E::adds_a_custom_agent_…`.
7. A fresh temporary home has no cached model catalog, so `agent_providers` gives Claude Code an empty `config` (`crates/ide-agent/src/lib.rs:538-540`, `catalogs.cached` then `config_for_new_task`). Settle: run `E::says_when_an_agents_settings_arent_known_yet`.
8. Clearing the environment forgets the saved values (`forget_env`, `provider.rs`) so no file under the home holds them afterwards, the vault file included. Settle: run `E::writes_the_agents_environment_variables_…`; if the vault keeps tombstones, drop that last check, not the agent-file checks.
9. The dialog's footer shows `page|key: message` for a refused external write (the old draft's test asserted this prefix). Settle: run `E::retains_a_failed_command_…`.
10. `toml` writes a path as `command = "<path>"` matching `format!("{path:?}")` for temp paths, and `args = ["--acp", "two words"]` with `"two words"` quoted. Settle: print a failing file (paths only, never env).
11. `cmd-a` selects all in a `TextField` under test on macOS. Settle: run.
12. `cx.executor().advance_clock` fires the poll's `background_executor().timer` in a view test without `allow_parking`. Settle: run `V::follows_an_install_started_before_the_page_opened_until_it_ends`.
13. `PathPromptOptions { prompt: None }` with `files: true` lets a test answer through `simulate_path_prompt_response` from a `CustomCalls::pick_file` started inside `cx.defer`. Settle: run the three `E` tests that browse or import.
14. `Localization` is `Default`, so `update_default_global::<Localization, _>` works where the global is missing (view tests). The old draft relied on the same.
15. The bare button texts of the webview ("Install", "Update", "Remove", "Removing…", "Cancel", the dialog's "Add") are translated by `Button` (`src/ui/Button.tsx:32`, read) and, Hypothesis, by `Dialog`'s buttons (`src/ui/Dialog.tsx` not read); the native page passes all of them through `tx`.
16. The layout values beyond the list (`AgentList.module.css` lines 81-306: details padding, dot colours, the form grid, section rules) were not read; the native details use the crate's own spacing and the tokens `accent` / `success` / `fg-subtle` for the dot. Hypothesis: they match; settle by reading that CSS and comparing with `details.rs`.
17. `WeakEntity::read_with` with an `AsyncApp` returns `Result` (used by `settings_agents_list.rs`, as by the old writer). Settle: compile.
18. `agents.list|…` staged keys and `agents.defaults` share one dialog page id; `modified_pages` (A4) marks the page for either. Settle: `E::page_reset_…`.

## (e) Tests most likely to fail first, and why

- Every `E` test that applies, if A7 is missing (Apply then reports "This window cannot do this").
- `E::page_reset_…`, if A3 or A4 is missing (no Reset button).
- `E::writes_the_agents_environment_variables_…`: the final "no file under the home holds a value" check depends on how the vault forgets (hypothesis 8).
- `E::adds_a_custom_agent_…`: focus after the click that opens the dialog (hypothesis 6) and the chooser answered from a deferred call (13).
- `V::follows_an_install_started_before_the_page_opened_until_it_ends`: the timer and `advance_clock` (12).
- `V::lists_every_agent_installed_first_and_filters_them`: exact line text (`"  ~Codex (Not found)"`); `lines()` puts `> `/`  `, then `~` for muted.
- `V::shows_why_an_install_failed_…`: the stand-in ends a cancelled install with `Failure::Cancelled` synchronously inside the deferred cancel; a re-entrant view update is avoided by the defer, but if the page view is borrowed there the update fails silently and the last `[Install]` check fails.
- No e2e test writes `agents.defaults` through Apply yet; add one when a fixture with a cached model catalog exists (hypothesis 7 says a fresh home has none).
- `workbench::tests::proofs_agents::more_agents_opens_settings_on_the_agents_page` and `proofs14_agents_views::lists_the_agents_and_more_agents_in_the_new_task_agent_menu` now open a page that lists the registry without a stand-in: the call is refused (an error line on the page and a `tracing::error!`), nothing is fetched; they should still pass.

## (f) Old draft files deleted when these land

`app-gpui/src/settings_view/pages/tools_agents.rs`, `app-gpui/src/settings_view/pages/tools_agents/proofs36_agents_view.rs`, `app-gpui/src/workbench/settings_agents.rs`, `app-gpui/src/workbench/tests/proofs36_settings_agents.rs`, and the obsolete parts of `app-gpui/src/agents/builtin_agents.rs` with its `tests.rs` (C2). The old notes' hunks 2, 3 and 7 are A3, A4 and A7 here; hunks 1, 4, 5 and 6 are replaced by A2, A10, A8 and A9.

## Secrets: where the page keeps them

- In memory: the environment `TextField` text; `SettingsDialog`'s `staged_external` (`ExternalChange.value`) and `external_applied`; `AgentList.providers[*].env` (filled from the saved secrets by `agent_providers`); `ProviderSettings.env`; the Add Custom Agent dialog's Environment field and the `CustomAgentInput` handed to `agent_custom_save` (never formatted). `Debug` of `AgentList`, `AgentDetails`, `ProviderSettings`, `AgentWrite` prints names only (tested); A5 and A6 cover the dialog's types. `lines()` of the page and of the dialog never show a value (tested); refusals name the variable or the entry's place.
- On disk, through the backend: the agent file `<config>/agents/<id>.toml` keeps names with empty values; values go to `AgentSecrets` (`crates/ide-host/src/agent_secrets.rs`): `SecretFile` over `<data>/agent-secrets.json` (the OS credential store in the app; a file vault under a test home since `keep_secrets_in_files`, `backend.rs:201`), else `<data>/agent-secrets.local.json`.

## What a later look at the real window must check (never open it over the real home)

Opening the page in the real app lists the registry, which downloads it; listing the agents reads the credential store for every `agent/<id>/env/<NAME>` key and moves plaintext values out of old agent files (`provider.rs:577-594`). So a look at the window needs a temporary `HOME` and `OPENIDEA_GPUI_HOME`, and accepts that the registry is fetched (or runs offline to see the warning / error line). Check there: the split's widths against the dialog's content width, the dot and tag colours, the Installed / Available headers, the progress bar, the confirmation and Add Custom Agent panels over the page, the drop-down of a New Tasks choice over the Launch section, the zh labels, and that the environment field is the only place a value shows.
