# Integration report: Services actions on folder nodes, `servicesViewFocus`, a table-editor cell being edited, and the stripe-button crash

All eight Services rows can be raised, and so can `context:dbTableEditorChanges`. `context:servicesViewFocus` now has its test but keeps one clause. The stripe-button crash is reproduced, fixed and proven. Every gate passed. Nothing is committed.

The brief's premise was false: the native Services tree had no folder nodes at all, so the folder grouping had to be built before any action could act on a folder.

## 1. Gates

All measured. Logs are in `$S/dsvc/gates/`.

The gates ran on a tree whose `app-gpui/` matches 8c752215c. HEAD moved to d10ea8919 while I worked, but `git diff --stat 8c752215c HEAD -- app-gpui` is empty, so the newer commits touch other files only.

| Gate                                        | Result                                                     | Load (uptime before → after) |
| ------------------------------------------- | ---------------------------------------------------------- | ---------------------------- |
| Whole suite, run 1                          | ok, 8436 passed, 0 failed, 235.03 s                        | 5.86 → 121.83                |
| Whole suite, run 2                          | ok, 8436 passed, 0 failed, 234.11 s                        | 121.83 → 104.01              |
| Added or changed tests, 20 runs by filter   | 20 of 20 passes, 58 tests each, no test failed at any time | —                            |
| `cargo test --test web_view`                | ok, 1 passed                                               | —                            |
| `cargo clippy --all-targets -- -D warnings` | exit 0                                                     | —                            |
| `cargo build`                               | exit 0, 0 warnings                                         | —                            |

- **Twenty-run filter:** `proofs47 table_editors_cell run_testing::services::tests`. It covers every test I added, the stripe test and all of `run_testing::services`, whose calls I changed.
- **Suite size:** 8,436 tests now (8,422 at HEAD, plus 14 added).
- **Failures:** none in any whole-suite or repeated run. The only failures seen were the intended ones in the first run and the mutation runs.
- **Cleanup:** no stand-in process was left running (`pgrep -fl 'sleep 300'` is empty after the gates).

## 2. Services: Run All, Restart All, Stop All, Open in Browser on a folder node

### The webview's rule (`src/features/run/services.tsx`)

**Folder nodes**

- A type's group lists its configurations' folders first. Folders are ordered by the first configuration that belongs to them, and each holds its configurations in display order. Configurations in no folder come after the folders (`:323-380`).
- A folder node's key is `run:folder:<kind>:<name>`, split at the first colon (`:72-82`). It has the `folder` icon, is expanded, and says "N running".
- An empty folder name counts as no folder.

**What each action targets**

- **A folder node:** `resolve()` gives the kind, the folder, and the sessions of that kind whose configuration's folder matches (`:449-457`). `itemActions` then gives the folder's Run All, Restart All, Stop All and Open in Browser (`:577-603`).
  - Run All and Restart All start `groupConfigs(kind, folder)`, which is the folder's children in the tree (`:493-517`).
  - Restart All first stops only the live sessions of that folder (`:519-532`).
  - Stop All stops the folder's live sessions.
  - Open in Browser collects the folder's live ports. One URL opens directly; several URLs show a chooser (`:545-569`).
- **A type node:** the same buttons over every configuration, folders included (`configItems`).
- **A configuration:** Run, Debug, browser, Stop and so on.
- **Nothing selected:** no node, so no buttons.

**What is disabled when**

- Run All: when every non-debug-only entry already runs.
- Restart All: when the folder or group has no entries.
- Stop All: when nothing runs.
- Open in Browser: when no live port is known.

**The two id families**

- The toolbar and menu ids are `run-all`, `restart-all`, `stop-all` and `open-browser`.
- Each toolbar entry runs the registered action `run.services.<x>` with the node's key (`:691-703`).
- The registered actions accept keys with the type, folder or configuration prefix and run the same `itemActions` (`:648-660`).
- Both families therefore resolve the same way.

**Other folder rules**

- A folder's menu is its toolbar buttons only. Add Service and Remove from Services are offered only when `r.folder === undefined` (`:711`).
- A folder's detail pane is titled with the folder's name (`:766`).

### The first run, before any change (`$S/dsvc/run0_head.log`)

8 failed and 3 passed.

- **The four toolbar tests** failed with "the Infra folder's row did not happen within thirty seconds": the native tree drew no folder node.
- **The registry Run All, Restart All and Open in Browser tests** timed out ("the folder's two configurations to run", "…to run again", "the page opens"): the registered handler drops any `run:folder:` key.
- **The registry Stop All test** got past its wait only because the 30-second stand-ins ended on their own. It then failed with "the configuration outside the folder still runs" (left `None`).
  - I then changed the tests: the stand-ins now sleep 300 seconds, and the wait also requires the folder's sessions to have been asked to stop.
  - M2 and M4 below reproduce the HEAD behaviour on the final tests.
- **The focus test and the two Database tests passed.** That was expected: their code was already there.

### The change

The resolution happens in one place, `Services::resolve`, and both families reach it through `invoke`.

**`app-gpui/src/run_testing/services.rs`**

- **New key helpers:** `FOLDER_PREFIX`, `folder_key`, `parse_folder_key` and `folder_of`, where an empty folder name counts as none.
- **Folders-first order:** `folders_first` sets the order, and `kind_groups` applies it. Type-wide `group_configs` and `moved()` therefore follow the order the tree shows, like `configItems` in the webview.
- **Tree:** `run_service_items` emits the folder nodes, then the loose configurations, with "N running" on each folder.
- **Targets:**
  - `Resolved` gains `folder`, and `resolve` has a folder branch.
  - `group_configs(kind, folder)` narrows to the folder.
  - `item_commands` uses the folder, and `ServiceCommand::StartAll` carries it.
  - `invoke` accepts `FOLDER_PREFIX`.
- **Restart All:** `StartAll::start(services, kind, folder, restart)` stops only the folder's live sessions on restart.
- **Menu and detail:** a folder's menu has no Add Service or Remove, and its detail is titled with the folder's name.

**`app-gpui/src/workbench/services_window.rs`**

- The registered handler for the four actions accepts `FOLDER_PREFIX`.
- `StartAll` receives the folder.

No existing assertion was changed or weakened. Only call signatures changed, in these tests of `run_testing::services::tests`:

- `run_batch` (the helper)
- `waits_for_process_exit_before_restarting_any_configuration_and_continues_after_a_launch_failure` (now `StartAll::start(…, None, true)`)
- the test that calls `group_configs("npm", None)` (at about line 2049)

### Tests

All window tests are in `app-gpui/src/workbench/tests/proofs47_services_folders.rs`, module `workbench::tests::proofs47_services_folders`. The fixture is three Program configurations:

- `edge`: a server reporting port 8124, in the folder Edge, first in the type's order;
- `a1`: only waits, in the folder Infra;
- `a2`: a server reporting port 8123, in the folder Infra.

The stand-ins are `/bin/sh` and `/bin/sleep`. The browser opener is the test's recorder (`cx.opened_url()`). Each wait covers exactly the next assertion.

The toolbar tests select the Infra row and click `services-action:<id>`. The registry tests run `run.services.*` with `run:folder:program:Infra`.

Model tests in `run_testing::services::tests`:

- `groups_configurations_by_their_folders_first_as_run_debug_configurations_does` (the spec case at `services.test.ts:199`)
- `a_folder_node_acts_on_its_configurations` (Run All, Stop All and Restart All targets, the menu, and the detail title)

### Mutations

Each mutation ran alone in `$S/dsvc/mutate.sh`, with the file restored by an EXIT trap and its hash compared afterwards. All six were restored identically.

| Mutation | What it does                                    | What failed                                                           |
| -------- | ----------------------------------------------- | --------------------------------------------------------------------- |
| **M1**   | `resolve` treats a folder key as its whole type | all 8 window tests and `a_folder_node_acts_on_its_configurations`     |
| **M2**   | the registered handler drops `FOLDER_PREFIX`    | exactly the 4 registry tests                                          |
| **M3**   | Restart All drops its folder filter             | both Restart All tests and `a_folder_node_acts_on_its_configurations` |
| **M4**   | the tree draws no folder node                   | the 4 toolbar tests and the grouping spec test                        |

What the failing assertions said:

- **M1:** "the configuration outside the folder is not started", "…still runs", "…keeps its run", and "the page opens" timed out, because two URLs give a chooser.
- **M2:** the registry tests timed out.
- **M3:** the outside configuration's run was `None` after the restart.
- **M4:** the toolbar tests timed out waiting for the Infra row.

| Row                               | Raised | Tests                                                                              | Mutations  | Clause left |
| --------------------------------- | ------ | ---------------------------------------------------------------------------------- | ---------- | ----------- |
| `action:run-all`                  | yes    | `…::run_all_on_a_folders_toolbar_starts_only_the_folders_configurations`           | M1, M4     | none        |
| `action:stop-all`                 | yes    | `…::stop_all_on_a_folders_toolbar_stops_only_the_folders_configurations`           | M1, M4     | none        |
| `action:restart-all`              | yes    | `…::restart_all_on_a_folders_toolbar_restarts_only_the_folders_configurations`     | M1, M3, M4 | none        |
| `action:open-browser`             | yes    | `…::open_in_browser_on_a_folders_toolbar_opens_the_folders_server`                 | M1, M4     | none        |
| `action:run.services.runAll`      | yes    | `…::run_services_run_all_on_a_folder_starts_only_the_folders_configurations`       | M1, M2     | none        |
| `action:run.services.stopAll`     | yes    | `…::run_services_stop_all_on_a_folder_stops_only_the_folders_configurations`       | M1, M2     | none        |
| `action:run.services.restartAll`  | yes    | `…::run_services_restart_all_on_a_folder_restarts_only_the_folders_configurations` | M1, M2, M3 | none        |
| `action:run.services.openBrowser` | yes    | `…::run_services_open_browser_on_a_folder_opens_the_folders_server`                | M1, M2     | none        |

`…` stands for `workbench::tests::proofs47_services_folders`. Each of the eight rows had only the folder clause.

## 3. `context:servicesViewFocus`

- **Test:** `workbench::tests::proofs47_services_folders::add_services_key_works_in_the_services_tree_and_not_in_the_editor`.
  - With a configuration row clicked, ⌘N runs exactly `["services.addService"]`, the context was on, and the Add Service menu opens.
  - With the editor focused, the context is off, ⌘N does not reach Add Service, and no menu opens.
- **Mutation M6** sets the context to `false` at `app-gpui/src/workbench/mod.rs:2409`. The test failed on the key: left `["project.newFile"]`, right `["services.addService"]`. So the key is shown to depend on the context.
- **Not raised to done.** The clause that remains: the webview sets the context for the tree only (`src/features/services/ServicesTree.tsx:227`). Natively it holds anywhere in the Services view, because there is one focus handle for the whole view (`app-gpui/src/services/view.rs:107`, `:704`).
  - Hypothesis: clicking the detail pane leaves the context on natively. Confirm with a window test that clicks `services-detail` and reads `servicesViewFocus`.

## 4. The extra: `context:dbTableEditorChanges`

- **Tests**, in `app-gpui/src/workbench/tests/proofs43_database_results.rs`, built on the Edit Data harness:
  - `workbench::tests::proofs43_database_results::revert_drops_the_table_editors_cell_being_edited`
  - `workbench::tests::proofs43_database_results::go_to_column_confirms_the_table_editors_cell_being_edited_first`
  - Both use the helper `edit_a_table_cell`: F2 on `users.name`, `ann` typed over as `amy`, nothing staged, and both context keys on.
  - Revert runs, which proves it is enabled, since `run_action` panics on a disabled action. It closes the field, nothing is dirty, the cell shows `ann`, the context goes off, and the database still holds `ann`.
  - Go to Column closes the field, stages `amy` and opens the column list. The database still holds `ann`.
- **Mutation M5:** `sync_database_context`'s table-editor branch, `app-gpui/src/workbench/database.rs:2024`, reduced to `editor.is_dirty()`. Both tests failed at the context assertion; `edit_data_shows_the_rows_submit_writes_an_edit_and_revert_discards_one` still passed.
- **Raised.** No clause is left of the one named.

## 5. Priority defect: a click on a tool window stripe button aborts the app

- **First run, before the fix** (`$S/dsvc/stripe0_head.log`): the test panicked with "cannot update openidea_gpui::workbench::Workbench while it is already being updated", from `gpui-pre-0.3.12/src/app/entity_map.rs:192:36`.
- **Fix** (`app-gpui/src/workbench/mod.rs`, the stripe button's `on_click`): the `toolwindow.<id>` action now runs in `window.defer`, the way the Trust Project banner button does.
- **Test:** `workbench::tests::proofs47_stripe_clicks::a_click_on_a_stripe_button_hides_and_shows_its_tool_window`, in `app-gpui/src/workbench/tests/proofs47_stripe_clicks.rs`. It passes after the fix.
  - A click on `stripe:project` hides the focused Project window, and the next click shows it.
  - A click on `stripe:problems` shows Problems, at the bottom, and the next click hides it.
  - The first run is the fail-first proof; the fix is the mutation.

**Sweep.** I scanned every `cx.listener(` and every call of `execute_action`, `execute_action_from` and `execute_action_or_throw` in production code under `app-gpui/src`, leaving out tests and undeclared drafts.

- **Listeners that run an action:**
  - `workbench/mod.rs:4298` (the stripe button): deferred, the fix.
  - `workbench/app_actions.rs:344`: already deferred.
  - `settings_view/pages/general_updates.rs:108`: left as it is. The listener holds the settings page and runs `app.checkForUpdates`, but no native handler is registered for that action, and the button is drawn only when it is. Hypothesis: a future handler that updates that page would panic. Confirm with `git grep -n 'handle("app.checkForUpdates"' -- app-gpui/src`, which has no hit today.
  - `plugin_views/settings_page.rs:876`, `features_b/viewers/image_view.rs:338` and `features_b/viewers/preview_host.rs:175`: safe. These call the view's own `run_action` method, not the action registry.
- **Event handlers and helpers that run inside a workbench update:** all already use `window.defer`. These are `agents.rs:353,1020`, `agents_views.rs:132`, `app_actions.rs:379,736`, `bookmarks_window.rs:68`, `branches.rs:384,1375`, `chrome.rs:132`, `code_insight.rs:186`, `context_menus.rs:908,943`, `debug.rs:287,438,532`, `deployment.rs:553,1516`, `editor_find.rs:142`, `editor_intelligence.rs:56`, `editor_templates.rs:399`, `postfix_action.rs:41`, `empty_editor.rs:97`, `menu.rs:592`, `mod.rs:1433,1446,2767,2791,2799,3033,3338,3405,3638,3858,3866,3874`, `plugin_contributions.rs:438`, `plugin_runtime.rs:499,582`, `plugins_window.rs:492`, `project_files.rs:695,1037`, `run_configs.rs:220,225,248,364`, `run_context.rs:675`, `services_window.rs:750`, `todo_window.rs:513`, and `editor/intelligence.rs:975`.
- **Outside any entity update:**
  - `context_menus.rs:1159`: a plain `on_click` closure, not a listener.
  - `editor_core.rs:101` and `vcs_text_actions.rs:88`: inside action handlers, after the workbench update has ended.
  - `inspect.rs:278` and `run/driver.rs:898`: run through `AnyWindowHandle::update`, which leases the window, not the workbench entity.
  - `platform/input.rs:260`: the keyboard dispatch.
  - `idle_save.rs:25`, `template_files.rs:437` and `editor_language.rs:714`: async `cx.update`.
  - `debug.rs:1373,1781,2086` and `debug_extras.rs:423`: test helpers.

Only the stripe button needed deferring.

## 6. Other rows whose state changed

- **`unit:src/features/run/services.test.ts`:** the case "groups configurations by their folders first, as Run/Debug Configurations does" is now covered by `run_testing::services::tests::groups_configurations_by_their_folders_first_as_run_debug_configurations_does`. That makes 42 of 46 cases; the row stays partial.
- **`point.registerServiceProvider:runConfigurations`:** the clause "a type's configurations are grouped by their folders (services.tsx:323)" is closed by the same model test and by the toolbar folder tests. The row's other clauses remain.

## 7. False or stale in the rows and the brief

- All eight rows, and the brief's step 3, assume a native configuration folder node exists. None did: grepping `folder` in `app-gpui/src/run_testing/services.rs` at HEAD had no hit.
- The brief says the suite has 8,422 tests at this HEAD. The first filtered run reported 3 run plus 8,422 filtered out, which is 8,425 tests with the 11 I had added at that point, so 8,414 at HEAD.

## Files

Edited:

- `app-gpui/src/run_testing/services.rs`
- `app-gpui/src/workbench/services_window.rs`
- `app-gpui/src/workbench/mod.rs` (the stripe fix and two `mod` lines)
- `app-gpui/src/workbench/tests/proofs43_database_results.rs`

New:

- `app-gpui/src/workbench/tests/proofs47_services_folders.rs`
- `app-gpui/src/workbench/tests/proofs47_stripe_clicks.rs`

Only these were formatted, with `$S/fmt.sh`. Nothing is committed, and nothing was edited under `docs/`, `crates/`, `src/`, `src-tauri/` or `e2e/`.

Logs: `$S/dsvc/run0_head.log`, `$S/dsvc/run1_change.log`, `$S/dsvc/stripe0_head.log`, `$S/dsvc/stripe1_fix.log`, `$S/dsvc/mut_M1/` through `$S/dsvc/mut_M6/`, and `$S/dsvc/gates/`.
