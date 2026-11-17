Items 6, 7, 10, 11 and 12 are done. Items 8, 9 and 13 are partly done, and the rest of them is listed below. Every gate passed. Nothing is committed and no git command that changes the index, the tree or the stash was run.

Logs are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dfol2b/`; below, a bare log name means a file in that folder.

## 1. Gates

| Gate                                        | Result                                                                                                                                        | Log                                                                                                                                                                       |
| ------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Suite run 1 (nothing else running)          | `test result: ok. 8262 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 215.53s`. Load average before 3.17, after 46.83.  | `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dfol2b/suite_run1.log`                   |
| Suite run 2 (20× loop beside it)            | `test result: ok. 8262 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 229.72s`. Load average before 43.24, after 12.86. | `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dfol2b/suite_run2.log`                   |
| 20× loop                                    | 20 of 20 runs `test result: ok. 95 passed; 0 failed …`. No test failed even once.                                                             | `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dfol2b/loop20.log` (script: `loop20.sh`) |
| `cargo test --test web_view`                | `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.42s`                                               | `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dfol2b/web_view.log`                     |
| `cargo clippy --all-targets -- -D warnings` | exit 0, no warnings                                                                                                                           | `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dfol2b/clippy.log`                       |
| `cargo build`                               | exit 0, 0 warnings                                                                                                                            | `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dfol2b/build.log`                        |

- The 20× loop covers 95 tests: every test I added or changed plus their whole modules.
- **Failures seen on the way:**
  - In a full run before the gates (`suite_first.log`), `settings_model::inspections_model::tests::describes_codes_counts_groups_and_keys_copies` failed with `left: "6 inspections." right: "2 inspections."`. It pinned the old Python group, which now has 2 servers and 4 built-ins (`inspections.test.ts:120-128`), so I changed its expectation to "6 inspections.".
  - Before I added Cmd-A to `proofs21_execute_popup::clears_selection_when_typing_changes_the_rows_and_ignores_section_clicks`, it timed out on "the shorter list at its top". The command line now shows the selected line, so typed text was appended to it; the spec replaces the value instead (`ExecutePopup.test.tsx:74`). After the change it passed in both suite runs and in all 20 loop runs.
- After `web_view`, `pgrep -fl "cargo|openidea_gpui|openidea-gpui"` printed nothing. The binary was never launched.

## 2. Items

### Item 6: plugin decorations and indexed markers

- **Spec at HEAD:**
  - `src/features/plugins/__tests__/decorations.test.ts:60` "filters tree badges by project root but not editor annotations, and drops unknown styles" (upstream `c8bdfa8ed`).
  - `src/features/plugins/indexedMarkers.test.ts:60` "keeps markers on their lines while typing and hides them when the entry goes away" (upstream `4a82c22fd`).
- **Before:**
  - Decorations from another project root were dropped from editors.
  - Any edit cleared every indexed marker.
- **Now:**
  - `PluginDecorations::editor(path, text)` no longer filters by root; tree decorations still do.
  - `IndexedMarkersView::doc_changed(old_doc, changes, new_doc)` moves each marker with its line start, the way CodeMirror maps a gutter marker (`MapMode.TrackBefore`, side −1, read in `node_modules/@codemirror/view` and `@codemirror/state`). A marker goes when a change from before its line start reaches it.
- **Rewritten:**
  - `filters_project_roots_and_unknown_styles_and_replaces_tree_badges` becomes `filters_tree_badges_by_project_root_but_not_editor_annotations_and_drops_unknown_styles`. A set from `/other` was empty in the editor; now it is shown, a tree badge from `/other` is hidden, and a badge from `/p` shows "M".
  - `hides_markers_on_edit_and_when_the_entry_goes_away` becomes `keeps_markers_on_their_lines_while_typing_and_hides_them_when_the_entry_goes_away`. The rows were empty after an edit; now they stay on line 1 while typing, move to 2 after a newline at 0, and go when the line is deleted.
  - In `drops_pending_icons_after_a_change` and `positions_a_new_indexed_result…`, the edit is now a real edit (the spec's replace-all and insert).
- **Added:** `stays_before_text_typed_at_its_line_start_and_goes_when_its_line_joins_the_one_above` (not from a spec).
- **Failed first:**
  - Both rewritten tests failed against the new signatures with the old behaviour kept (`fail_6_7.log`).
  - The added test failed with the mapping set back to strict `<` and `Assoc::After` (`fail_followups_off.log`, `left: [2]`).
- **Left:** these models have no caller in the app (measured by grep), so nothing draws them yet. The other new decorations cases (tree note, dynamic icons, lsp consent) are not done.

### Item 7: Maven and Gradle Execute popup

- **Spec at HEAD:** `ExecutePopup.test.tsx:46`, `:92` and `:103`. Upstream `2f2fa79cb`: the input shows the selected row (`ExecutePopup.tsx`), Enter always runs the selected line or the typed one, and a double click runs a row.
- **Before:** Enter accepted the selected completion into the line, and a click appended it.
- **Now:**
  - The arrows and a click select a row and show its line in the field.
  - Enter runs the shown line; a double click runs the row.
  - Choosing another project, or a build model that really changed, clears the selection. A model read again unchanged keeps it.
- **Rewritten:**
  - Model tests `starts_page_up_on_a_completion_rather_than_a_section` and `page_down_selects_a_completion_rather_than_a_section_at_its_landing_position`: Enter returned `None` and completed; now Tab completes.
  - The same two tests in `workbench::build_tools::tests::proofs21_execute_popup`. The PageUp one also asserts the field shows "clean".
  - `clears_selection…`: Cmd-A before typing, plus two asserts.
- **Added:**
  - In `build_tools::execute::tests`: `shows_a_clicked_completion_in_the_input_instead_of_appending_it` and `enter_runs_the_selected_completion_or_the_typed_line`.
  - In `proofs21_execute_popup`: `shows_a_clicked_completion_in_the_input_instead_of_appending_it`, `enter_runs_the_selected_completion_instead_of_completing_it`, `a_double_click_on_a_completion_runs_its_line` and `a_changed_build_model_clears_the_selection`.
- **Failed first:**
  - The model tests failed against shims (`select` that accepted, `shown_line` that returned the line).
  - The workbench click, Enter and PageUp tests failed against the old code (`fail_6_7.log`).
  - The double-click and model-change tests failed with their branch switched off (`fail_followups_off.log`).
  - Never seen failing: the two model Tab rewrites and the workbench PageDown rewrite, because Tab already accepted before.

### Item 8: debugger Attach dialog (cases 0, 1 and 4)

- **Spec at HEAD:** `src/features/debugger/__tests__/attach.test.ts:56`, `:65` and `:93` (upstream `aa8822d9a` and `209fc796c`).
- **What exists:** the native model already handles Unity and .NET: kinds, labels, order and requests.
- **Added to the tests:**
  - New test `attaches_the_chosen_unity_editor_by_its_exact_id_without_launching_or_using_a_native_adapter` asserts adapter, request, `program` None, cwd `/game` and the exact extra.
  - The fixture gains .NET pid 40. `groups_processes_by_debugger_in_ideas_order_with_counts` now expects ".NET (netcoredbg):1", and `builds_the_attach_request_of_each_debugger` asserts netcoredbg, attach and processId 40.
- **Never seen failing:** the native behaviour already matched, so nothing was changed.
- **Left:** no view calls `attach_rows` (measured by grep), so I cannot show that the dialog is drawn. Hypothesis: the Attach popup draws no rows natively. Confirm by reading `src/debugger/overlays.rs` and the workbench for a view of `overlays.attach`.

### Item 9: context menu order and "Accent color"

- **"Accent color" (done):**
  - The label is renamed in `builtin_pages/behavior.rs` (keywords swapped to `accent colour`, `highlight`, as in `behavior.tsx:875`), `custom_controls.rs` and `settings_view/pages/appearance.rs`. "Custom colour" stays, as in `Form.tsx:265`. Upstream `33957ed34`.
  - Rewritten: `settings_model::builtin_pages::tests::indexes_built_in_option_labels_for_search` (`pages.test.ts:83`) and `settings_view::pages::appearance::tests::a_swatch_or_a_custom_colour_stages_the_accent`. Both failed first (`fail_8_13.log`).
- **Menu order (listed, not built):**
  - The brief says "editor tab", but report row 1193 is the project tree's `contextMenu.test.ts:122` (`project_view::context_menu::tests::follows_ideas_order_for_a_file`).
  - That case needs "Open in Right Split" (upstream `76f0b3fc6`, `project.openInRightSplit`) and a "File Color" submenu with "Edit File Colors…" (`bedd9d2fc`). Neither exists natively: grep for `openInRightSplit` and for file colours found nothing.
  - `follows_ideas_order_for_a_file` is untouched and still asserts the old list.

### Item 10: remote settings

- **Spec at HEAD:** `src/platform/remoteSettings.test.ts:132`, `:139-154` and `:223`. The client-only keys are `CLIENT_SETTINGS_KEYS` in `src/platform/clientSettings.ts` (107 keys).
- **Before:** the native list had 82 keys.
- **Now:** the native list holds the same 107 keys in the same order.
- **The one place the keys are read from:** `platform_more::client_settings::testing::shared_client_keys()` reads the webview's list (`include_str!` of `clientSettings.ts`).
  - `keeps_client_settings_writes_local_in_either_scope` asserts each key routes locally in both scopes.
  - Added `keeps_every_client_setting_of_the_shared_model_and_nothing_else` asserts the native list equals the shared one.
- **Host routing:** `preserves_routing_for_host_settings` gains `agents.newTaskApprovals`, `agents.defaults` and `agents.mcpServers` (project → host, global → both).
- **Client settings on load:** `preserves_client_keymaps_and_ui_settings_when_loading_remote_settings` now has the spec's `agents` fixtures. It asserts agents = `{fitWindowWidth: true, newTaskApprovals: "bypass", mcpServers: …}`.
- **Failed first:** the routing table test (on `general.closeWithoutWelcome`), the list check and the merge test all failed (`fail_8_13.log`). `preserves_routing_for_host_settings` was never seen failing: it guards the three `agents.*` keys after the change. No test opens a remote connection.

### Item 11: marker icons and line markers

- **Spec at HEAD:** `markerIcons.test.ts:29` and `:64`, `lineMarkerGutter.test.ts:97`.
- **Now:** an unknown or null icon falls back to `gutter-navigate`, and `implementing` is an alias of `gutter-implementing`.
- **Rewritten:** `retains_built_in_names_legacy_aliases_and_unknown_icon_fallback`, `falls_back_when_an_asset_fails_to_load` and `refreshes_a_fallback_when_the_package_icon_declaration_arrives_after_its_markers`. Each expected `gutter-implementing` and now expects `gutter-navigate`; all three failed first.
- **Left:** the shared gutter icon area that `lineMarkerGutter.test.ts` case 0 expects is not built.

### Item 12: inspections

- **Spec at HEAD:** `inspections.test.ts:104`. Built-ins in `src/platform/inspections.ts:265` (upstream `708cca8e4` and `33c5627bd`).
- **Now:** `BUILTIN_INSPECTIONS` gains `builtin:cucumber-undefined-step` (group Gherkin) and the four Python built-ins, with the webview's texts and levels.
- **Rewritten:** `groups_servers_by_language_with_seen_and_configured_codes_built_ins_by_category` adds the Gherkin group and the four Python ids; it failed first. `describes_codes_counts_groups_and_keys_copies` now expects "6 inspections." for Python.

### Item 13: the small ones

- **GitHub remotes on `ssh.github.com` (done):**
  - Spec `data.test.ts:396`. `GITHUB_REMOTE` in `github/data_model.rs` now takes the webview's prefixes, as in `model.ts:416`.
  - `github::repository::tests::recognizes_github_com_remotes` gains the two `ssh.github.com` URLs, `https://ssh.github.com` → None, and the `fork` remote between origin and upstream. It failed first.
- **Zig live templates:** nothing to do. They already landed in `efc778614` and `templates::live_builtins::tests::ships_the_requested_built_ins` passed in this tree (`fail_8_13.log`). The reader's row is stale.
- **Keymap presets:**
  - `platform::keymap::tests::lets_scoped_actions_win_shared_keys` gains the two `run.*` pairs (`ideaParity.test.ts:106-107`).
  - Added `loads_the_written_out_presets_as_their_sources_declare_them`. It reads `intellij-macos.ts`, `intellij-windows.ts` and `vscode-macos.ts` and asserts that `assets/gen/keymaps.json` has the same order and keys.
  - Both were never seen failing: the generated copy was already current (also checked with a one-off Python comparison).
  - Not checked: `vscode-windows`, `eclipse-macos` and `eclipse-windows`, which are built from spreads.
- **Diff dialog footer row:** listed, not built. The native diff window has only a header, toolbar and body; the webview's has a footer "Close" button (`diffDialog.tsx:106`).
- **Memory popup cpu, pid and End Process:** listed, not built. Upstream `d5cd5e7a5` is 445 lines, including the backend's CPU sampler and the guard that refuses processes the app did not start. Nothing was added natively, so End Process is not offered for any process.

## 3. Spec cases that gained a native test

| Spec case                                                                                                       | Native test                                                                                                                                                                                                                                        |
| --------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `decorations.test.ts`: filters tree badges by project root but not editor annotations, and drops unknown styles | `plugin_models::decorations::tests::filters_tree_badges_by_project_root_but_not_editor_annotations_and_drops_unknown_styles`                                                                                                                       |
| `indexedMarkers.test.ts`: keeps markers on their lines while typing and hides them when the entry goes away     | `plugin_models::indexed_markers::tests::keeps_markers_on_their_lines_while_typing_and_hides_them_when_the_entry_goes_away`                                                                                                                         |
| `ExecutePopup.test.tsx`: starts PageUp on a completion rather than a section                                    | `build_tools::execute::tests::starts_page_up_on_a_completion_rather_than_a_section`, `workbench::build_tools::tests::proofs21_execute_popup::starts_page_up_on_a_completion_rather_than_a_section`                                                 |
| `ExecutePopup.test.tsx`: PageDown selects a completion rather than a section at its landing position            | `build_tools::execute::tests::page_down_selects_a_completion_rather_than_a_section_at_its_landing_position`, `workbench::build_tools::tests::proofs21_execute_popup::page_down_selects_a_completion_rather_than_a_section_at_its_landing_position` |
| `ExecutePopup.test.tsx`: shows a clicked completion in the input instead of appending it                        | `build_tools::execute::tests::shows_a_clicked_completion_in_the_input_instead_of_appending_it`, `workbench::build_tools::tests::proofs21_execute_popup::shows_a_clicked_completion_in_the_input_instead_of_appending_it`                           |
| `attach.test.ts`: attaches the chosen Unity Editor by its exact ID without launching or using a native adapter  | `debugger::attach::tests::attaches_the_chosen_unity_editor_by_its_exact_id_without_launching_or_using_a_native_adapter`                                                                                                                            |
| `attach.test.ts`: groups processes by debugger in IDEA's order with counts                                      | `debugger::attach::tests::groups_processes_by_debugger_in_ideas_order_with_counts`                                                                                                                                                                 |
| `attach.test.ts`: builds the attach request of each debugger                                                    | `debugger::attach::tests::builds_the_attach_request_of_each_debugger`                                                                                                                                                                              |
| `pages.test.ts`: indexes built-in option labels for search                                                      | `settings_model::builtin_pages::tests::indexes_built_in_option_labels_for_search`                                                                                                                                                                  |
| `remoteSettings.test.ts`: keeps %s writes local in either scope                                                 | `platform_more::remote::tests::keeps_client_settings_writes_local_in_either_scope`                                                                                                                                                                 |
| `remoteSettings.test.ts`: preserves routing for host setting %s                                                 | `platform_more::remote::tests::preserves_routing_for_host_settings`                                                                                                                                                                                |
| `remoteSettings.test.ts`: preserves client keymaps and UI settings when loading remote settings                 | `platform_more::remote::tests::preserves_client_keymaps_and_ui_settings_when_loading_remote_settings`                                                                                                                                              |
| `markerIcons.test.ts`: retains built-in names, legacy aliases and unknown-icon fallback                         | `plugin_models::marker_icons::tests::retains_built_in_names_legacy_aliases_and_unknown_icon_fallback`                                                                                                                                              |
| `markerIcons.test.ts`: falls back when an asset fails to load                                                   | `plugin_models::marker_icons::tests::falls_back_when_an_asset_fails_to_load`                                                                                                                                                                       |
| `lineMarkerGutter.test.ts`: refreshes a fallback when the package icon declaration arrives after its markers    | `plugin_models::line_markers::tests::refreshes_a_fallback_when_the_package_icon_declaration_arrives_after_its_markers`                                                                                                                             |
| `inspections.test.ts`: groups servers by language with seen and configured codes, built-ins by category         | `settings_model::inspections_model::tests::groups_servers_by_language_with_seen_and_configured_codes_built_ins_by_category`                                                                                                                        |
| `data.test.ts`: recognizes github.com remotes                                                                   | `github::repository::tests::recognizes_github_com_remotes`                                                                                                                                                                                         |
| `live.test.ts`: ships the requested built-ins (already present)                                                 | `templates::live_builtins::tests::ships_the_requested_built_ins`                                                                                                                                                                                   |
| `ideaParity.test.ts`: lets scoped actions win shared keys                                                       | `platform::keymap::tests::lets_scoped_actions_win_shared_keys`                                                                                                                                                                                     |
| `ideaParity.test.ts`: %s (partly: the generated copy, not each key against IDEA's)                              | `platform::keymap::tests::loads_the_written_out_presets_as_their_sources_declare_them`                                                                                                                                                             |

Rows that stay MISSING:

- `contextMenu.test.ts` "follows IDEA's order for a file"
- `diffDialog.test.tsx` "owns the close shortcut with %s focus"
- `perf.test.ts` "builds popup sections in category order"
- `lineMarkerGutter.test.ts` case 0 (the shared icon area)

## 4. Stale or false in the notes and brief

- Item 9 says "editor tab's context menu"; the row is the project tree's menu.
- `RETRIAGE_UNIT_2.md` calls Zig MISSING; it landed in `efc778614`.
- Item 10 names the module `platform_more::remote_settings`; the routing tests are in `platform_more::remote`.
- The brief's own scratch folder is `$S/dfol2/`; this launch used `dfol2b/` as instructed.

## 5. Files edited (all under `app-gpui/src/`, no new files)

- `build_tools/execute.rs`
- `debugger/attach.rs`
- `github/data_model.rs`, `github/repository.rs`
- `platform/keymap.rs`
- `platform_more/client_settings.rs`, `platform_more/remote.rs`
- `plugin_models/decorations.rs`, `plugin_models/indexed_markers.rs`, `plugin_models/line_markers.rs`, `plugin_models/marker_icons.rs`
- `settings_model/builtin_pages.rs`, `settings_model/builtin_pages/behavior.rs`, `settings_model/inspections_model.rs`
- `settings_view/custom_controls.rs`, `settings_view/pages/appearance.rs`
- `workbench/build_tools/tests/proofs21_execute_popup.rs`

These were formatted with `$S/fmt.sh` in stdin mode.
