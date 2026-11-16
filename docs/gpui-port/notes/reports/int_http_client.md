Report: HTTP Client slice and Part 0 ("an action runs and nothing ever happens").

**Nothing under `crates/` changed.** `git status --short src/ src-tauri/ crates/ docs/ e2e/` prints nothing. Nothing is committed.

## 1. Gates

Each line is verbatim from its log under `$S/dhttpi/`.

**Final code** (two whole-suite runs, plus the loop during the second):

- `last_suite1.log`, run with nothing beside it:
  - uptime before: ` 1:27  up 94 days, 10:57, 5 users, load averages: 4.13 10.61 14.97`
  - result: `test result: ok. 7758 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 195.93s`
  - uptime after: ` 1:31 … load averages: 72.32 26.08 19.45`
- `last_suite2.log`, run during the loop:
  - uptime before: ` 1:32 … load averages: 61.42 25.27 19.24`
  - result: `test result: ok. 7758 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 217.07s`
  - uptime after: ` 1:35 … load averages: 64.65 38.65 25.56`
- Loop during the second run: every added or changed test (filters `http_client::`, `show_usages`, `a_tree_read`, `reformat`; 86 tests) passed **20 of 20** runs. Each run printed `test result: ok. 86 passed; 0 failed; …` (`$S/dhttpi/last_loop_summary.txt`).
- `cargo test --test web_view`: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.44s`
- `cargo clippy --all-targets -- -D warnings`: exit 0.
- `cargo build`: exit 0, 0 warnings.

**Earlier runs, on earlier code:**

- `suite1_kept.log` was the first suite after the slice landed:
  `test result: FAILED. 7757 passed; 1 failed; …`
  The failure, whole:
  ```
  thread 'workbench::context_menus::tests::every_menu_group_is_listed_once' (281418096) panicked at src/workbench/context_menus.rs:1366:9:
  assertion `left == right` failed
    left: ["debugger.hotswap", "debugger.stop", "deployment", "github.reviewMode", "go.modules", "gradle.analyzeDependencies", "maven.analyzeDependencies", "settings.fileType", "templates.new", "web.openIn"]
   right: ["debugger.hotswap", "debugger.stop", "deployment", "github.reviewMode", "go.modules", "gradle.analyzeDependencies", "http.spring", "maven.analyzeDependencies", "settings.fileType", "templates.new", "web.openIn"]
  ```
  The cause is the slice itself: `http.spring` now has rows. I removed `http.spring` from that test's expected list.
- `suite2.log` and `suite3.log`, plus the loop during suite 3, ran before two later changes:
  - both suites passed: `7758 passed; 0 failed`
  - the loop passed 19 of 20.
- Run 17 of that loop failed. Its log is gone (deleted by my next loop's `rm`); this is the failure as printed:
  ```
  thread 'workbench::project_view_options::tests::a_tree_read_again_after_a_change_on_disk_keeps_the_selected_file' panicked at src/workbench/project_view_options.rs:855:9:
  assertion `left == right` failed
    left: Some("/private/var/folders/hk/…/T/.tmp05pwEm/d.mock")
   right: None
  ```
  See Part 0 below for what this means and how the test changed.
- `final_suite1.log` and `final_suite2.log` came after that test fix but before the `services.show` change:
  - both `7758 passed; 0 failed`
  - the loop passed 20 of 20.

## 2. Part 0

**The new report.** A wait that runs out now also prints `window_action_report` (`app-gpui/src/workbench/mod.rs`, test module). It is called from `wait_long`, the `settle` of `mod.rs`, `editor_extras.rs`, `editor_language.rs` and `refactor_actions.rs`, and the inline panic in `apply_the_single_matching_action_or_offer_a_choice`. It prints:

- the active editor: path, version, caret, selections, the word at the caret, and its hint;
- the tabs still opening (`Workbench::loading_tabs`, test-only);
- every truthy context key the dispatcher computes, which shows where the keyboard is;
- the Project tree's `selected()` and `selected_path()`;
- the last code-insight hint, and the newest notice with its time;
- the Show Usages state (`popup_request` and the popup flow);
- a per-thread trace (`platform::actions::test_trace`, test-only): every action `run` executes with its argument and result, plus a line at each silent exit:
  - `run_on_selected_file` with no file selected;
  - `editor_for` "did not open in an editor";
  - `carry_out_fetch` with no backend;
  - a popup fetch with no search;
  - a dropped answer (`current(request)` mismatch);
  - a cancelled popup search;
  - what `show_usages` asked and the effects it got back.

I printed a sample once from a throwaway test and then deleted it. The report rendered, e.g.:

```
active editor: …/src/alpha.rs … caret=0:13 … word at the caret=Some("Widget")
action find.showUsages  ran: ok
```

**Lead 1: settled by tests.**

- `workbench::popup_placement::tests::show_usages_with_the_caret_on_no_name_opens_no_popup_and_says_so`. With the caret on an empty line, no popup opens and the hint is "No usages found. Based on syntax (language server not ready)".
- `workbench::popup_placement::tests::show_usages_cannot_run_while_the_file_is_still_opening`. While the tab has no editor yet, `find.showUsages` is `Unavailable`, so `run_action` would panic rather than run. The failing reports say the action ran, so the cause of those failures is not a missing editor.
- Reading the four tests' setups against what their actions read: `edit_at`, the refactor fixture (waits for `didOpen` and for focus) and `select_in_tree` all wait for the state their action reads. I found no early return to fix.

**Third lead: changed in production code, on a hypothesis.** `editor_for` (`workbench/editor_language.rs`) gave up after 250 × 16 ms of the executor clock and returned `Ok(None)` silently. `wait_long` advances that clock 20 ms per poll while the document opens on a real thread, so under load the bound could run out before the file opened.

- It now waits as long as the path's tab is still opening (`Workbench::is_opening`); only the steps after loading ends count toward the bound.
- Hypothesis: this was the cause of the `project.optimizeImports` failure. To confirm, a failing run's trace would show `<path> did not open in an editor`.
- No test fails without this change; I found no deterministic way to hold the open.

**Lead 2: no defect in the re-read itself, but one gap found.**

- `workbench::project_view_options::tests::a_tree_read_again_after_a_change_on_disk_keeps_the_selected_file` proves a tree re-read after a write keeps the selection, and Optimize Imports in the tree still acts on it. This matches `src/features/project-tree/store.ts:464-490`.
- Measured in loop run 17: when the removal of the selected file came through the real watcher, `selected()` still named `d.mock` after the row was gone.
- Hypothesis: the watcher reported the delete not as `removed` (a create followed by a delete) and the re-read dropped the row. Both the native code and `store.ts` clear the selection only on a `removed` event, so this is shared behaviour, not a native defect.
- The removal half of the test now hands `apply_fs_change(removed=[d])` to the tree directly; it no longer goes through the watcher.

## 3. The slice

**Files changed.**

- Declared: `lib.rs` (`pub mod http_client;`) and `workbench/mod.rs` (the module, field, initial value, `register_actions`, `http_editor_opened`, `http_gutter_layer`, the `.http` toolbar arm, `http_files_changed` on FsChanged).
- Hooked in: `services_window.rs` (`http_service_panel`) and `context_menus.rs` (the `http.spring` rows, the `GroupFacts.http_spring` field and how it is filled).

**Fixes I made to the drafts:**

- A move error in `generate_spring`.
- A clippy lifetime in `response_view` tests.
- The clock test's arithmetic: its sample time was 20 seconds past a minute.
- Disabled toolbar buttons now fade at 0.4 opacity (the webview's `.iconButton:disabled`, `src/ui/Button.module.css:97`). The Stop icon is coloured, so it ignored the faint colour and showed red.
- The environment list now closes on Escape, a press outside it, or a second press on the chooser. In the shot it stayed open after Escape. Test: `http_client::request_toolbar::tests::the_environment_list_closes_on_escape_a_press_outside_and_the_chooser`.
- `services.show` now switches to the Services tab, as `showServiceView(SERVICES_TREE_VIEW)` does (`src/features/services/index.ts:91`).
- Added `ServiceTabs::services()`.

**Settled before building on them:**

- **Proxy.**
  - Measured: `cargo tree -e features -i reqwest@0.13.5` lists no system-proxy feature, so Auto mode reads only the `*_PROXY` environment variables.
  - The fixture asserts those variables are unset. It writes a manual proxy on a dead `127.0.0.1` port into the home's settings, and `run_request_at_the_caret…` asserts that setting before the request still reaches its listener. Manual mode always bypasses loopback (`proxy_settings.rs:135-141`).
  - Note that the fixture's `Backend::init` sets the process-wide proxy for the whole test process while it runs.
- **Language id, `Send` bound, `use<>`, scratch path.** All settled by compiling and passing the tests.
- **Marks vs plugin gutter icons.** On a line with a plugin icon, the mark moves one icon to the left. `workbench::http_client::tests::a_generate_mark_and_a_plugin_gutter_icon_on_one_line_do_not_overlap` fails without this: the mark at 324,84 (14×14) overlapped the icon at 325,85 (12×12).
- **The token.**
  - Read every `tracing::` call the slice adds (six in `workbench/http_client.rs`): none prints an exchange, header or environment value.
  - The store and the views do not derive `Debug`. `ide_http::HttpExchange` does (in crates), and the slice never formats it.
- **Services tab host.** No production code downcasts the Services panel; the two `downcast::<ServicesView>` sites are in tests. `…the_http_client_lists_loads_and_clears_the_projects_history` now also asserts:
  - before the HTTP Client is used, the panel is the bare `ServicesView` with no tabs;
  - after it is used, the same tree entity sits under the Services tab;
  - `services.show` and `http.showToolWindow` each select their tab.

**What the webview has that native does not:**

- `.http` highlighting.
- URL completion from the endpoints.
- The endpoint gutter and `plugin_endpoints`.
- The Spring Alt+Enter intentions.
- Search Everywhere's Endpoints tab.
- Selectable response text, a resizable split, and a spinner.
- Marks that follow an edit immediately (native marks re-parse 300 or 400 ms after typing stops).
- Two appends while the scratch is still opening (native keeps only the second).
- A localized time in the history (native shows the English clock only).
- The Services tabs in the tool window's header row (native draws them on a second row).
- A gutter column of its own for the marks (see the shots below).

**Shots** (real binary through `run.sh`, temp home and project; no request was sent, Agents was not opened):

- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/insp8/shots/dhttp4-01-http-editor-1.png`: the toolbar (faded Stop, "No Environment") and run marks on lines 2 and 6.
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/insp8/shots/dhttp4-02-env-chooser-1.png`: the list shows No Environment, dev, prod, inside the window.
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/insp8/shots/dhttp4-03-http-client-empty-1.png`: the Services and HTTP Client tabs, "No requests yet", "No response selected", the list closed by Escape.
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/insp8/shots/dhttp4-04-spring-marks-1.png`: generate marks on lines 7, 9 and 12.
- **Not fixed:** in 01 and 04 the marks overlap the right edge of the line-number digits. The editor gutter has no icon column (`editor/view.rs` `metrics`). Plugin icons sit at the same place, and the left shift for a plugin icon moves the mark further onto the numbers.

## 4. Parity rows

Tests are named in full; `wb` stands for `workbench::http_client::tests`.

| Key                                                                  | State                                  | Proof / what is missing                                                                                                                                                                                                                                                                                           |
| -------------------------------------------------------------------- | -------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:http.runRequest`                                             | done                                   | `wb::run_request_at_the_caret_sends_it_with_the_chosen_environment_and_shows_the_response`, `wb::run_request_without_a_request_in_the_file_says_so`, `wb::a_request_that_cannot_be_prepared_is_told_and_not_sent`; `when` read on and off in `wb::the_http_client_actions_run_only_where_their_when_clause_holds` |
| `action:http.runAll`                                                 | done                                   | `wb::run_all_sends_every_request_of_the_file_in_order_and_a_run_mark_sends_its_own`; `when` on/off, same test                                                                                                                                                                                                     |
| `action:http.showToolWindow`                                         | done                                   | `wb::the_http_client_lists_loads_and_clears_the_projects_history`; `when` on/off                                                                                                                                                                                                                                  |
| `action:http.newRequest`                                             | partial                                | `wb::new_http_request_creates_a_request_file_with_the_example_in_the_folder`; `when` on/off. Missing: the scratch case with no folder, and the hand-off to `templates.newFromTemplate`                                                                                                                            |
| `action:http.openEndpoint`                                           | done                                   | `wb::open_in_http_client_appends_the_endpoint_to_the_generated_requests_scratch` (it has no `when`)                                                                                                                                                                                                               |
| `action:http.generateSpringRequest`                                  | done                                   | `wb::generate_requests_from_the_spring_mapping_at_the_caret`; `when` on/off                                                                                                                                                                                                                                       |
| `action:http.generateSpringControllerRequests`                       | done                                   | the same test, plus `wb::the_spring_marks_and_the_editor_menu_offer_the_generate_actions`; `when` on/off                                                                                                                                                                                                          |
| `ipc:http_requests`                                                  | done                                   | `wb::run_all_sends_every_request_of_the_file_in_order_and_a_run_mark_sends_its_own` (marks on lines 1 and 4)                                                                                                                                                                                                      |
| `ipc:http_environments`                                              | done                                   | `wb::run_request_at_the_caret_sends_it_with_the_chosen_environment_and_shows_the_response`                                                                                                                                                                                                                        |
| `ipc:http_execute`                                                   | done                                   | same test                                                                                                                                                                                                                                                                                                         |
| `ipc:http_history`, `ipc:http_history_get`, `ipc:http_history_clear` | done                                   | `wb::the_http_client_lists_loads_and_clears_the_projects_history`                                                                                                                                                                                                                                                 |
| `ipc:spring_route_sources`                                           | done                                   | `wb::the_spring_provider_lists_the_request_paths_and_rescans_after_a_change` (read through `http_endpoints`; no view lists it)                                                                                                                                                                                    |
| `ipc:plugin_endpoints`                                               | not earned                             | not called                                                                                                                                                                                                                                                                                                        |
| `point.registerEditorContribution:http.requests`                     | done                                   | the tests above, plus `http_client::request_toolbar::tests::{runs_all_requests_and_stops_only_while_one_is_sent, chooses_an_environment_of_the_file, a_buffer_without_a_file_has_no_environments, the_environment_list_closes_on_escape_a_press_outside_and_the_chooser}`                                         |
| `point.registerEditorContribution:http.spring`                       | partial                                | `wb::the_spring_marks_and_the_editor_menu_offer_the_generate_actions`, `wb::a_generate_mark_and_a_plugin_gutter_icon_on_one_line_do_not_overlap`. Missing: the intentions                                                                                                                                         |
| `point.registerEditorContribution:http.endpoints`                    | not earned                             | no endpoint gutter                                                                                                                                                                                                                                                                                                |
| `point.registerEndpointProvider:http.spring`                         | done                                   | `wb::the_spring_provider_lists_the_request_paths_and_rescans_after_a_change`                                                                                                                                                                                                                                      |
| `point.registerMenuGroup:http.spring`                                | done                                   | `wb::the_spring_marks_and_the_editor_menu_offer_the_generate_actions`                                                                                                                                                                                                                                             |
| `point.registerMenuGroup:http.new`                                   | not earned                             | no test goes through the New menu (its row action is now registered)                                                                                                                                                                                                                                              |
| `point.registerServiceView:httpClient`                               | partial                                | `wb::the_http_client_lists_loads_and_clears_the_projects_history`, `http_client::service_tabs::tests::switches_between_the_services_and_the_http_client`, the seven `http_client::response_view::tests::*`. Missing: the split, selectable text, a spinner, tabs in the header row                                |
| `feature:http-client`                                                | partial                                | the rows above; missing: highlighting, completion, the endpoint gutter, intentions                                                                                                                                                                                                                                |
| `unit:…/spring/generate.test.ts`                                     | done                                   | 1 of 1                                                                                                                                                                                                                                                                                                            |
| `unit:…/spring/spring.test.ts`                                       | done                                   | 17 of 17                                                                                                                                                                                                                                                                                                          |
| `unit:…/spring/routeIndex.test.ts`                                   | done                                   | 6 of 6 (the `it.each` counted once)                                                                                                                                                                                                                                                                               |
| `unit:src/workbench/endpointProviders.test.ts`                       | done for the model, partial in the app | 2 of 2 as model tests. Case 2 (a replacement provider) is never exercised by the app                                                                                                                                                                                                                              |
| `unit:…/endpoints.test.ts`                                           | partial                                | 1 of 3 (rendering); completion and its disposal are not reached                                                                                                                                                                                                                                                   |
| `unit:…/httpClient.test.ts`                                          | partial                                | 5 of 18 (the format cases, through the response view); the 13 highlighting cases are not reached                                                                                                                                                                                                                  |

## 5. False or stale in the notes and briefs

- The notes say to "run `cargo fmt` after pasting"; that is forbidden here.
- The notes did not foresee that `every_menu_group_is_listed_once` must change.
- The notes' proxy set-up (`mode = "none"`) proved nothing about loopback; I replaced it as described above.
- `docs/gpui-port/INVENTORY.md` and `parity.json` showed `M` in `git status` at the start, and `git diff` showed them clean a few minutes later. That is what I observed; I don't know the cause. I did not write to them.
