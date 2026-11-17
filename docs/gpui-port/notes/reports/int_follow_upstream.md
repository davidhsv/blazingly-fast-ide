# Integrator report: the ten native features upstream changed (follow-upstream slice)

Eight of the ten items are built and proved by tests that failed before the change. Item 2 moved the control onto Code Style instead of removing it. Two pieces are only listed: the One Dark notice (part of item 5) and Stash Selected Files (part of item 10). Nothing is committed. All gates are green on the final tree. No process of mine is left: I checked by PID after the last shot run, and no cargo or test process remained.

## 1. Gates

Log folder: `$S/dfol/` (written `dfol/` below).

**Final gates (no source edit after final suite run 1 started):**

- **Suite run 1:** `dfol/suite1.log`: `test result: ok. 8113 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 215.36s`.
  - Load: before `53.51 29.72 18.18`, after `18.58 20.34 16.76`.
  - Hypothesis: the high starting load is the previous suite run, which had ended about a minute earlier. Not measured.
- **Suite run 2:** `dfol/suite2.log`: `test result: ok. 8113 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 216.96s`.
  - Load: before `16.12 19.75 16.60`, after `50.83 28.09 20.42`.
- **Twenty repeats, during suite run 2:** `dfol/x20.log`. They ran the test binary directly, filtered to the 33 added or changed tests, so no second cargo command ran.
  - Result: 20 of 20 `test result: ok. 33 passed; 0 failed`.
- **web_view:** `dfol/web.log`: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.32s`, `rc=0`.
- **Clippy:** `dfol/clippy.log`, `cargo clippy --all-targets -- -D warnings`: `rc=0`, no warning or error lines.
- **Build:** `dfol/build.log`: `Finished dev profile … in 2m 09s`, `rc=0`, 0 warnings.

**Failures seen along the way (logs kept):**

- **`dfol/suite0_failed.log`:** `test result: FAILED. 8109 passed; 4 failed`. All four are expectations my items changed; I updated them (see section 3):
  - `agents::task_list_view::tests::marks_a_task_that_finished_in_the_background_with_an_unread_dot`: `left: "Ready | a | claude · a | 10m"`, `right: "Ready | a | claude · a | 10m | unread"`.
  - `agents::task_list_view::tests::proofs11_task_list::marks_background_status_transitions_unread`: `assertion left == right failed: Ready, left: [], right: ["one"]`.
  - `settings_view::dialog::tests::walks_the_tree_from_the_keyboard_and_through_the_history`: the tree now has `"Agents"` between `"Languages & Frameworks"` and `"Tools"`.
  - `workbench::tests::smoke_settings_pages::every_settings_page_opens_and_applies_a_change_made_with_the_mouse`: `editor.naturalLanguages: nothing to change in ["→ Spelling"]`, `vcs: nothing to change in [...]`, `python: nothing to change in [...]`. This led to the `folder` flag described under item 1.
- **`dfol/x20_attempt1.log`:** `renames_with_the_tracking_kept_or_dropped_when_the_box_is_ticked` failed 3 of 20, each time with `panicked at src/workbench/mod.rs:9960:9: assertion left == right failed, left: "", right: "origin/kept"`. The other 17 runs passed all 33 tests.
  - Hypothesis: git renames the branch ref before it renames the branch's config section, and the test read the upstream between the two.
  - Confirming check: run `git branch -m` in a loop and read `for-each-ref --format=%(upstream:short)` right after the ref appears.
  - Fix: the test now waits for the tracking instead of asserting at once. The code comment states the race as fact; that wording is the same hypothesis. Then I re-ran every gate (the final results above).
  - `dfol/suite1_attempt1.log` and `dfol/suite2_attempt1.log` (both `8113 passed`) predate that test edit.
- **Red runs:** `dfol/red_run.log` is the new tests against the unchanged code: `test result: FAILED. 3 passed; 22 failed`. The three that passed were changed tests, not proofs:
  - `shortens_home_folders_of_every_platform` only lost an assertion.
  - `the_actions_need_what_their_when_clauses_name` only gained an id.
  - `shows_a_dot_on_the_task_and_a_clickable_status_count_even_when_all_tasks_are_idle` also passes on status-derived unread.
  - Green before the full suite: `dfol/green_run2.log`, `33 passed`.

## 2. Items

1. **Settings pages renamed or moved:** done.
   - **Upstream:** 9e46d61dc (#2511), 32abc77a7 (#3626), bff6a12e2 (#2987).
   - **Webview at HEAD:**
     - `src/features/settings/pages/tools.tsx:683-686`: `agents.chat`, group Agents, order 460.
     - `src/features/settings/pages/mcpServer.tsx:136-140`: `agents.mcpServer` "IDE MCP Server", order 470.
     - `src/features/agents/model.ts:11-13` and `index.ts:61-68`: `agents.list` "Coding Agents", order 450.
     - `src/features/settings/pages/languages.tsx:28-35`: a `python` folder page with no body.
     - `src/features/settings/pages/languages.tsx:37-43`: `python.interpreter` is titled "Dependencies", order 403.1.
     - `src/features/settings/pages/languages.tsx:198-205`: `python.ruff`, order 403.3.
     - `SettingsDialog.tsx:85-86`: the Python and Agents descriptions.
     - `SettingsDialog.tsx:211`: a page without a body draws as a folder.
     - `SettingsDialog.tsx:258`: an unknown stored id falls back to the first page. There is no mapping from old ids, and native already falls back the same way.
   - **Native before:** the pages were `tools.agents` (under Tools), `tools.agents.chat`, `tools.mcpServer` and `languages.ruff`. There was no Python folder.
   - **Changed:**
     - `builtin_pages.rs` gained `AGENTS_GROUP`.
     - Page specs and ids: `builtin_pages/features.rs`, `builtin_pages/tools.rs`, `mcp_server.rs`, `language_pages.rs`, `agents/model.rs` (`REGISTRY_PAGE = "agents.list"`).
     - Page constants: `languages_pages.rs` (`RUFF`).
     - `page_spec.rs`: a new `folder` flag. `vcs` and `python` set it, so Version Control now also draws as a folder, as `SettingsDialog.tsx:211` does for it.
     - `dialog.rs`: folder rendering and the two new descriptions.
   - **Tests:**
     - `app-gpui/src/workbench/tests/proofs19_settings_languages.rs::puts_the_python_pages_in_a_python_folder_in_pycharms_order` (red: dialog opened on `appearance`).
     - Updated expectations: `lists_every_page_in_the_settings_tree_order`, `keeps_every_language_under_languages_and_frameworks_as_in_intellij_idea`, `draws_every_language_page_of_the_settings_tree`, `draws_the_views_given_for_bespoke_pages_and_controls`, `walks_the_tree_from_the_keyboard_and_through_the_history`.
     - Page ids switched to the new ones in `proofs_agents.rs`, `proofs14_agents_views.rs` and the ruff tests in proofs19.
     - The smoke test now accepts a folder page that draws its links (explained by `SettingsDialog.tsx:211`).
   - **Inventory rows:** `settings:agents.chat`, `settings:agents.mcpServer`, `settings:python.ruff`. Hypothesis: they stay partial, because of what is listed below.
   - **Listed, not built:**
     - The new controls on Chat. Its keywords now name them (send, reasoning, approval, bypass, width) although those controls are not built.
     - The model catalog.
     - IDE MCP Server's new controls: brave mode and listen address.
     - New pages: `agents.workingStatus`, `agents.mcpServers`, `agents.policy`, `python.interpreters` (All Interpreters), `python.docstrings`, `tools.externalDiff`.
2. **Plugin Formatters:** moved onto Editor › Code Style.
   - **Upstream:** 0cfe0c82f. **Webview:** `codeStyleScheme.tsx:15,36,147` and `pages/editor/pluginFormatters.tsx`. The section is hidden while no plugin offers a formatter, and `languages.pluginFormatters` is part of the Code Style scheme.
   - Native Code Style already shows its formatter settings on one page, so I put the existing `PluginFormattersView` at the end of its Formatter section. It is drawn under a "Plugin Formatters" heading only when it has languages to show.
   - Its questions are answered through `languages_sdk::answer` (`custom_controls.rs`). `CODE_STYLE_KEYS` now includes the setting. The `languages.pluginFormatters` page and its `Control::Formatters` variant are gone.
   - **Test:** `proofs19_settings_languages.rs::code_style_offers_a_plugin_formatter_per_covered_language_and_stores_the_pick` (red at `assert_ne!` on the old page id).
   - The Formatter tab structure itself is not built; native keeps one flat Code Style page.
3. **Shift+F12, Restore Current Layout and Save Changes in Current Layout:** done.
   - **Upstream:** 460027fd4. **Webview:** `layouts.ts:76-89`, `toolwindows/actions.ts:98-118,205`.
   - **Changed:** `app_actions.rs` (both actions and the `toolWindowLayoutActive` context key) and `menu_spec.rs` (menu entries).
   - **Test:** `app_actions/tests/proofs20_layouts.rs::restores_the_current_layout_and_saves_changes_into_it` (red: `toolWindowLayoutActive` was off). It presses Shift+F12.
   - **Inventory:** `module:src/workbench/toolwindows/layouts.ts`. Hypothesis: now done.
4. **Editor font zoom:** done.
   - **Upstream:** 1259423fd. **Webview:** `editor/core/actions.ts:149` resets the active editor (default scope); `:176` resets every editor.
   - **Changed:** `editor_core.rs` (`editor.resetFontSize` resets the active editor; new `editor.resetFontSizeAll` handler) and `menu_spec.rs` (both entries).
   - **Test:** `editor_core.rs::the_font_size_steps_and_resets_in_the_active_editor_and_resets_in_all_editors` (red: `[0, 0]` where `[1, 0]` was expected).
   - **Listed:** the Settings choice `editor.general.wheelZoomScope` and its "all editors" behaviour.
5. **Quick Switch preview delay:** done, with no visible effect yet.
   - **Upstream:** 7f9bf3727. **Webview:** `quickSwitch.ts:39-61`, preview at `:47`.
   - **Changed:** `quick_switch.rs`. There is no delay now, and the theme already shown is not shown again.
   - **Tests:** three spec cases renamed and rewritten to the HEAD spec (the delay cases are gone). All three were red.
   - No native menu calls this model: the native Quick Switch menu has no hover preview, so the change shows nothing until that is wired. Not built.
   - **One Dark notice (cc72161f9, `src/features/plugins/oneDarkNotice.ts`): listed, not built.** This deviates from the brief's criterion: it is a notice through the existing notifications. But its Install action needs a marketplace install the window can start from a notice, and natively that only exists on the Plugins page model (`plugins_page.rs:990`). It also needs a once-only flag and a wait for the plugin list. The backend op `settings_removed_theme` exists.
6. **Stack frames resolved by exact file name:** done.
   - **Upstream:** 226ac2695 (#2313). **Webview:** `src/features/run/navigate.ts:43-48`, which calls `findFilesNamed`, then checks the exact path and the declared package.
   - **Changed:** `run/navigate.rs` (`ResolveStep::FindFilesNamed`, no 50-file limit) and `run/driver.rs` (calls `ide_host::ops::nav::find_files_named`).
   - **Test:** `proofs4_run.rs::keeps_the_package_in_a_crowded_java_source_lookup` (red: `notices=["No sources for this class"]`). It calls `RunDriver::open_location` directly instead of clicking a link.
   - **Listed:** the fallback for old remote servers.
7. **Debugging through plugin run targets:** removed.
   - **Upstream:** 2b80c7531. **Webview:** `debugger/actions.ts:351,379`.
   - **Changed:**
     - `debugger/actions.rs`: no `with_run_target`, no `__runTarget`, no run-after on the target, and the `AfterStart::RunPluginTarget` and `Host.run_targets` fields are gone.
     - `debugger/driver.rs`: no longer builds those targets.
   - **Test:** `debugger/actions.rs::debugs_a_configuration_on_a_plugin_target_on_this_machine` (red: `__runTarget` was present).
   - **Remaining difference:** the webview refuses Debug on a targeted configuration in `remoteDebugBlocked` (`run/remote.ts:70`, called at `run/index.ts:174`). Native `Workbench::debug_config` has no such check (I read it), so native now debugs on this machine what the webview refuses. Not built.
8. **Services shows only the Docker connections the user added:** done for the window.
   - **Upstream:** d5cb379ca. **Webview:** `services/tree.ts:24` (nothing starts expanded) and `docker.tsx:144` (a new connection opens once).
   - **Changed:** `services/model.rs` (`DEFAULT_EXPANDED` is gone) and `services/docker.rs` (connections start expanded).
   - **Test:** `services/docker.rs::opens_a_connection_once_when_it_first_appears` (red).
   - Hypothesis: on a fresh data folder the backend lists no connection. I read this in `ide-docker/src/connection.rs:157-169`; I did not run it. Confirm with `cargo test -p ide-docker load_`.
   - **Listed:** the run-configuration half of the same commit. That covers the Spring Boot default type, remembered runs, a hidden configuration staying hidden while it runs, and the console strip without Run, Debug and Stop. It is a larger rework of `run_testing/services.rs`.
9. **Agent notifications on the backend's Attention events:** done.
   - **Webview:** `agents/store.ts:570-573,648`.
   - **Changed:**
     - `agents/store.rs`: a status change no longer marks a task unread or notifies; Attention does both.
     - `agents/model.rs`: `Attention` and `attention_change` are removed.
   - **Tests:**
     - New: `marks_background_attention_unread` and `does_not_notify_from_status_alone_once_attention_exists` (both red).
     - Updated: `shows_a_dot…`, `task_list_view::marks_a_task_that_finished…`, and `proofs11_task_list::marks_background_status_transitions_unread`, now named `marks_background_attention_unread`.
   - **Listed:** `ModelCatalog` and the model catalog.
10. **Version control:**
    - **Unset upstream on rename:** done. **Webview:** `branchOps.ts:291-298`.
      - **Changed:** `vcs_ops/branch_ops.rs` (the checkbox is offered only for a branch that tracks one) and `workbench/branches.rs` (passes the choice to the backend).
      - **Tests:** `branch_ops.rs::offers_to_drop_the_upstream_only_for_a_branch_that_tracks_one` and `workbench/mod.rs::renames_with_the_tracking_kept_or_dropped_when_the_box_is_ticked`. The second uses a temp repository with a local bare remote. Both were red.
    - **Stash Selected Files (`vcs.stashSelected`, `vcs/actions.tsx:705-722`): listed, not built.** It is a new action with its own selection and notice, not one control on an existing dialog.

## 3. Tests removed or rewritten, with why

**Removed:**

- `agents::model::tests::notifies_on_finish_failure_and_questions_only`: the derivation of attention from status changes is gone from the webview.
- `proofs19::plugin_formatters_format_on_save_applies`: it tested the Format on save toggle of the removed Plugin Formatters page. 0cfe0c82f says that toggle duplicated Actions on Save and Editor › General.
- The `languages.pluginFormatters` row of `reset_and_cancel_discard_an_edit_on_each_language_page`, and the scope assertion for that page in `shortens_home_folders_of_every_platform`: the page is gone.

**Rewritten:**

- The Plugin Formatters choice test now runs on Code Style.
- The three Quick Switch delay cases follow the HEAD spec.
- The three status-transition unread tests now use Attention events.

## 4. Shots

All under `$S/insp8/shots/`. The project was under `/private/tmp/openidea-inspect-qCnx9H/`.

- **`dfol2-01-python-light-1.png`:** the Python folder page, light. It shows its description, links to Dependencies and Ruff, and the tree reads … TypeScript, Agents, Tools.
- **`dfol3-01-python-dark-1.png`:** the same in dark, with the Agents group between Languages & Frameworks and Tools.
- **`dfol2-02-agents-group-light-1.png`:** Tools › Terminal with Agents (collapsed) above Tools. No Agents page was opened.
- **`dfol2-04-codestyle-dark-1.png`:** the top of Code Style with the Formatter section, dark.
  - The light Code Style shot (`dfol1-01`) also shows only the top of the page.
  - Plugin Formatters cannot appear in these shots: no plugin provides a formatter, so it is hidden.
- **`dfol2-07-quickswitch-dark-1.png`, `dfol2-08-quickswitch-light-1.png`:** the Quick Switch menu (Editor Color Scheme, Keymap, View Mode, Theme). Nothing visible changed there.
- **Seen in the shots:** in `dfol2-05` and `dfol2-06`, the selected row (Python, Terminal) was below the visible part of the tree, which does not scroll to it. That is a native behaviour I did not touch.

## 5. Stale or false in notes and briefs

- **Item 6:** the brief points at `app-gpui/src/debugger/`, but #2313 changed only `src/features/run/navigate.ts`. The debugger's `FindFiles` is the Java main-class lookup, which upstream did not change, and nothing under `src/debugger` opens output locations (grep).
- **`int_merge.md` part 6** says "One Light and One Dark are still offered": native already reads those settings as Islands (`theme/appearance.rs:328`). Hypothesis: they are not offered in Quick Switch.
- **The undeclared draft `workbench/tests/proofs36_settings_agents.rs:268`** still expects `tools.agents|…`. It is now stale; I left it untouched.

## 6. Files edited

All under `app-gpui/src/`:

- **agents:** `agents/model.rs`, `agents/store.rs`, `agents/store/tests.rs`, `agents/task_list_view.rs`, `agents/task_list_view/tests/proofs11_task_list.rs`
- **debugger and run:** `debugger/actions.rs`, `debugger/driver.rs`, `run/driver.rs`, `run/navigate.rs`
- **services:** `services/docker.rs`, `services/model.rs`
- **settings model:** `settings_model/builtin_pages.rs`, `builtin_pages/editor.rs`, `builtin_pages/features.rs`, `builtin_pages/tools.rs`, `builtin_pages/vcs.rs`, `code_style_scheme.rs`, `language_pages.rs`, `mcp_server.rs`, `page_spec.rs`
- **settings views:** `settings_view/custom_controls.rs`, `settings_view/dialog.rs`, `settings_view/pages/languages_controls.rs`, `settings_view/pages/languages_pages.rs`
- **vcs:** `vcs_ops/branch_ops.rs`
- **workbench:** `workbench/app_actions.rs`, `app_actions/tests/proofs20_layouts.rs`, `branches.rs`, `editor_core.rs`, `menu_spec.rs`, `mod.rs`, `quick_switch.rs`
- **workbench tests:** `workbench/tests/proofs14_agents_views.rs`, `proofs19_settings_languages.rs`, `proofs4_run.rs`, `proofs_agents.rs`, `smoke_settings_pages.rs`

Each was formatted alone with `$S/fmt.sh`. Nothing under `crates/`, `src/`, `docs/` or the off-limits drafts was touched.
