Both features are in the build. One requirement is only half proven: a trusted project's own plugin never loads natively in the window flows I tried, so the Restricted Mode test cannot show the difference trust makes.

## 1. Suite, clippy, scope

- **Suite ×5 on the final tree:** 6641 passed, 0 failed, 0 ignored in each run (86–101 s). Clippy `--all-targets -D warnings` is clean.
- **Compile errors, Part 1 (3):** two missing `AppContext` imports and one `str` sizing error that resolved with them. Then 3 dead-code warnings and 2 clippy errors (`type_complexity`, a needless lifetime), plus a deprecated `on_scroll_wheel` / `ScrollWheelEvent`, now `on_scroll` / `ScrollEvent`.
- **Compile errors, Part 2:** none.
- **Scope:** `git status --short` lists only my 8 files: `debugger/driver.rs`, `plugin_views/{mod,view_node}.rs`, `workbench/{mod,plugin_contributions,plugin_java_debug,plugin_runtime,plugin_tool_windows}.rs`. I added temporary debug prints to `workbench/debug.rs`, `run/driver.rs`, `debugger/state.rs` and `debugger/actions.rs` and took them out; those files show no diff. I ran rustfmt per file only, and no other agent's draft was touched.
- **Manifest test:** passes. The printed ids (550) are in `$S/registered_ids.txt`. That project loads no plugin, so no `toolwindow.plugin.*` id was printed. That the manifest's `("toolwindow.", Rest::Any)` rule covers such ids is from reading the rule, not from this run.

## 2. Part 1 — plugin views

All paths are under `workbench::plugin_tool_windows::tests::` unless marked `view_node::`.

| test                                                                                                | proves                                                                                                                                                                                           |
| --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `a_plugins_view_is_a_tool_window_with_its_title_and_icon_that_shows_the_rendered_rows`              | title "Greetings", icon `plugin`, right side, shown verbatim, stripe button; plugin still `Inactive` before opening; rows render; `activeToolWindow` and `toolWindow.<id>.visible` go on and off |
| `clicking_a_node_reaches_the_plugin_and_its_refresh_repaints_the_view`                              | expand reaches the plugin, its refresh repaints; double click raises the plugin's notice; Collapse All                                                                                           |
| `a_view_that_fails_to_render_says_the_plugin_could_not_show_it`                                     | failure state "The plugin could not show this view" with the plugin's error                                                                                                                      |
| `disabling_the_plugin_removes_its_tool_window`                                                      | window, panel, stripe button, visibility key and `toolwindow.` action all go                                                                                                                     |
| `closing_the_project_drops_its_plugin_windows`                                                      | panel and container dropped                                                                                                                                                                      |
| `a_plugin_table_sorts_selects_and_offers_its_row_command`                                           | sort, `table-select`, the `viewItem.context` command runs with the row                                                                                                                           |
| `reveal_shows_and_focuses_the_views_window_and_a_hidden_view_stays_hidden`                          | reveal shows and focuses; a group follows its `when`; a hidden view stays hidden                                                                                                                 |
| `a_restricted_projects_own_plugin_shows_no_view` (new)                                              | an installed plugin's view registers; the restricted project's own plugin is not loaded and has no window, stripe button or action                                                               |
| `view_node::…::a_tree_of_more_items_than_the_cap_each_inside_the_last_is_drawn_up_to_the_cap` (new) | 20,010 nested items drawn up to 20,000 in a window, keys still work, markup drawn as text                                                                                                        |
| `view_node::…::a_table_claiming_more_rows_than_can_be_laid_out_is_drawn_up_to_its_cap` (new)        | `total_rows = u32::MAX` draws up to 1,398,100 rows                                                                                                                                               |

- **Parity keys that exist:**
  - `unit:src/features/plugins/__tests__/views.test.tsx`: partial, 15 of 26 by name (13 in the new files plus 2 more in `plugin_models::view_helpers`). Missing: the 4 HTML frame cases, the HTML theme switch, "adds a plugin", command-argument unpacking, context-menu projection, "groups one plugin", zero-based coordinates. View-item menu projection is shown only end to end.
  - `wit:wit/views.wit#views`: refresh and reveal are now proven; post-message is still partial.
  - `context:activeToolWindow`: covered.
- **Backend calls:** the IPC adapters call `ide_host::plugins::views::render` / `event`, which are `view_invocation(host, window, doc, editor)` plus `render_view` / `view_event` on a blocking thread. Native makes the same calls with `doc = None`, after `authorize_command`.
- **Node kinds:** message, tree, list, form (label, read-only, text, multiline, checkbox, choice) and table (editable, sortable, paged, row commands).
- **Events:** every view event except `message`.
- **HTML bodies:** shown as "This view needs a newer version of OpenIDEA (html)." This is the runtime's wording for body kinds it does not know (`crates/ide-plugin/src/runtime.rs:2078`). It is not in the text catalog, and the backend never produces it for an HTML body: the webview draws HTML itself (`PluginView.tsx:301`).
- **Not built:** HTML and `web =` bodies, post-message, Services-location views, detail panes, plugin text catalogs, custom editors.

## 3. Part 2 — Java debug

- **Flow:**
  1. `plugin_run_request` with `debug` goes to `plugin_java_debug_request`.
  2. `AgentDebug::start` adds the detector's `debugArgs`, and `RunDriver::start` runs it (`ops::run::start`).
  3. A `RunStore` observer feeds each completed non-system console line to `AgentDebug::on_line`.
  4. The port must be 1–65535; the debugger then runs `debug.attach` with host `127.0.0.1`.
  5. From there the existing code runs: `JavaFlow::attach`, `java_debug_server` (in tests the `JavaDebugServerPort` global answers instead), then `debug_start`.
- **Tests:** all under `workbench::plugin_java_debug::tests::`, 7 in total.
  - `a_plugins_java_debug_request_attaches_to_the_port_the_jvm_announces`
  - `stop_ends_the_process_and_the_session`
  - `a_process_that_exits_before_announcing_a_port_leaves_no_debug_session`
  - `an_attach_the_adapter_refuses_is_reported_with_its_message`
  - `a_jdwp_port_out_of_range_is_not_attached_to`: now `70000`, `0`, a non-number and a 20-digit value.
  - `an_announced_host_is_ignored_and_the_debugger_attaches_on_loopback` (new): the line names `203.0.113.9:50123`, and the adapter records `127.0.0.1:50123`.
  - `a_plugins_debug_request_without_the_java_debugger_runs_under_its_own_adapter`
- **Keys:** `action:debug.pluginStart` drops "the JDWP branch is not reached" and stays partial. `wit:wit/run.wit#run` can become done.
- **Nothing real starts:** the "JVM" is an `sh` script in the temp home. The adapter is the patched fake run through `python3` behind a loopback listener the test opens. No JVM, Bazel or jdtls.

## 4. Fixes, changed tests, anchors, false notes

- **Product fix:** in a plugin view's tree, Enter and Tab carry a control character, which was being added to the speed search, so Enter never activated a row (`view_node.rs` `tree_key`). It now filters control characters, as `database/tree_view.rs` does. This was the only drafted test that failed for a product reason.
- **Test fixes:**
  - The Stop and attach tests now wait for the session to carry its process. Events show it "Running" before the start is answered, and the process is set only then, in native and in the webview alike (`src/features/debugger/state.ts:610`). Measured: Stop came while `process` was still `None`, so the JVM ran until its own 60 s time-out.
  - The cap test expects `MAX_ITEMS + 1` lines: the last drawn item is open with its children past the cap, so it shows a "Loading…" row.
- **Anchors:** all still occurred exactly once. The only change was `self.drop_github();` now following `drop_agents` in `open_project`.
- **False in the notes:**
  - These parity keys do not exist: `ipc:plugin_view_render`, `ipc:plugin_view_event`, `wit:…#view-types`, `#view-provider`, the `point.registerToolWindow:(at) …viewWindows.tsx:118` and `point.registerServiceView:(at) …` keys, and the `viewWindows` / `viewProjection` unit keys.
  - The start task does not guarantee the process is recorded on the debug session.
- **Drafted tests left out:** none.
- **Project plugins, measured:**
  - A project trusted before it opens, then Reload All Plugins: its own plugin is not loaded within 30 s.
  - After Trust Project, waiting for the trust, then Reload All Plugins: it is still not loaded. The existing proof (`a_projects_own_plugins_load_only_while_the_project_is_trusted`) also opens the Plugins page, which I did not.
  - Hypothesis, from reading: even once loaded, its views would not show, because `Plugins::refresh` lists with `host.list(None)`, which leaves out project plugins. Confirm by adding an assertion that the view registers after trusting, opening the Plugins page and loading, then running `cargo test --lib -- a_restricted`.
- **What a `java-debug` detector plugin still lacks natively** (hypotheses from reading only; check with the `parity.json` rows named):
  - its menu entries and settings page if it is a project plugin, for the reason above;
  - "Debug Target…" from the configuration chooser (`run.debugConfig` goes straight to `debug.startConfig`, not through this attach);
  - stopping its `workspaceContains:` listener (`unit:…workspaceContains.test.ts`: 12 of 13);
  - the mainToolbar slot (built, no test draws it);
  - the Debug-tool-window failure reports of the Java flow, which the test seam never reaches.
- **Seen, not fixed:** a plugin view whose body changes kind keeps the scroll handle of the previous body.
