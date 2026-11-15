The HTTP Client slice is drafted as new files only; nothing has been compiled or run. Wiring needs the hunks in section 1–4 of the notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/http_client_integration.md`.

**1. Files written**

| File                                          | Lines | Tests           |
| --------------------------------------------- | ----- | --------------- |
| `app-gpui/src/http_client/mod.rs`             | 9     | —               |
| `app-gpui/src/http_client/store.rs`           | 315   | 6 plain         |
| `app-gpui/src/http_client/marks.rs`           | 252   | 5 plain         |
| `app-gpui/src/http_client/request_toolbar.rs` | 374   | 3 gpui          |
| `app-gpui/src/http_client/response_view.rs`   | 801   | 6 gpui, 1 plain |
| `app-gpui/src/http_client/service_tabs.rs`    | 217   | 1 gpui          |
| `app-gpui/src/workbench/http_client.rs`       | 1307  | —               |
| `app-gpui/src/workbench/http_client/tests.rs` | 468   | 10 end-to-end   |

The tests file sits beside the module and the assignment did not name it.

Views:

- **`HttpClientView::new(cx)`** (the responses view), fed by `set_state(history, selected, exchange, running_index)`.
  - Events: `Select(id)`, `Stop`, `ReloadHistory`, `ClearHistory`.
  - Accessors: `rows`, `running_text`, `is_empty`, `is_loading`, `tab`/`tabs`/`select_tab`, `raw_toggle`/`toggle_raw`, `status_line`, `request_line`, `body`, `header_rows`, `request_text`, `header_actions`, `press`.
- **`RequestToolbar::new(path, editor_view, cx)`** (the panel above an `.http` editor), fed by `set_state(environments, chosen, running)`.
  - Events: `RunAll`, `Stop`, `Choose(Option<String>)`, `Reload`.
  - Accessors: `toolbar`, `options`, `chosen`, `is_chooser_open`, `press`, `choose`.
- **`ServiceTabs::new(services_view, focus, http_view, active)`**: the Services tool window with a tab per view (Services, HTTP Client) and the active view's header buttons.
  - Accessors: `tabs`, `active`, `select`, `header_actions`, `http`.

The `mod` lines are `pub mod http_client;` in `lib.rs` and `mod http_client;` in `workbench/mod.rs`. The other hunks:

- `workbench/mod.rs`: the field and its initial value, `register_actions`, a call after `watch_plugin_gutter`, the gutter layer in `render`, the toolbar arm in `pane_element`, and the `FsChanged` arm.
- `services_window.rs`: `mount_services`.
- `context_menus.rs`: the `http.spring` rows, a new `GroupFacts` field, and filling it in `group_facts`.

**2. Actions** (id, label, `when` and shortcut come from `assets/gen/actions.json` and `keymaps.json`; the Tools menu entries already exist)

| Action                                                                | Test (`workbench::http_client::tests::`)                                                                                                                                                              | IPC                                                      |
| --------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| `http.runRequest`                                                     | `run_request_at_the_caret_sends_it_with_the_chosen_environment_and_shows_the_response`, `run_request_without_a_request_in_the_file_says_so`, `a_request_that_cannot_be_prepared_is_told_and_not_sent` | `http_requests`, `http_environments`, `http_execute`     |
| `http.runAll`                                                         | `run_all_sends_every_request_of_the_file_in_order_and_a_run_mark_sends_its_own`                                                                                                                       | `http_requests`, `http_execute`                          |
| `http.showToolWindow`                                                 | `the_http_client_lists_loads_and_clears_the_projects_history`                                                                                                                                         | `http_history`, `http_history_get`, `http_history_clear` |
| `http.newRequest`                                                     | `new_http_request_creates_a_request_file_with_the_example_in_the_folder`                                                                                                                              | fs create/read                                           |
| `http.openEndpoint`                                                   | `open_in_http_client_appends_the_endpoint_to_the_generated_requests_scratch`                                                                                                                          | fs                                                       |
| `http.generateSpringRequest`, `http.generateSpringControllerRequests` | `generate_requests_from_the_spring_mapping_at_the_caret`, `the_spring_marks_and_the_editor_menu_offer_the_generate_actions`                                                                           | `fs_read_text`                                           |

`spring_route_sources` is proven by `the_spring_provider_lists_the_request_paths_and_rescans_after_a_change`, but only through the `Workbench::http_endpoints` accessor, because no native view lists endpoints yet.

All seven actions are registered. Every request a test sends goes to a `127.0.0.1` listener the test opens on a port the OS picks. The token from the private environment file is asserted absent from the view's and the notifications' `Debug` output.

**3. Spec files (cases reached from the app / total)**

| Spec                                  | Cases | Status                                                                                 |
| ------------------------------------- | ----- | -------------------------------------------------------------------------------------- |
| `spring/spring.test.ts`               | 17/17 | done                                                                                   |
| `spring/generate.test.ts`             | 1/1   | done                                                                                   |
| `workbench/endpointProviders.test.ts` | 2/2   | done                                                                                   |
| `spring/routeIndex.test.ts`           | 6/6   | done; workspace member folders are passed in, but no end-to-end test opens a workspace |
| `endpoints.test.ts`                   | 1/3   | partial; no URL completion                                                             |
| `httpClient.test.ts`                  | 5/18  | partial; the 13 highlighting cases have no hook                                        |

**4. Left out**

- **Highlighting, URL completion and Alt+Enter intentions:** I found no extension point for a feature to add these (hypothesis; the files to check are in the notes).
- **Endpoint gutter marks and `plugin_endpoints`:** those endpoints come from the plugins feature (`points.tsx:157`), and no plugin fixture reports any. `plugin_endpoints` is not earned.
- **Search Everywhere's Endpoints tab** still answers "failed" (`everywhere/view.rs:361`); feeding it is a hunk for the search slice.
- **There is no separate Endpoints view in the webview**, so none was built. The providers, Open in HTTP Client and the Spring routes are what exists.
- **Smaller gaps:** marks keep their old lines until the next re-parse; a second append while the scratch is still opening replaces the first; no resizable split, selectable response text or spinning indicator.

**Behaviour that differs from the Tauri adapter or the webview:**

- The history write is awaited before the answer comes back, so a reload right after a run cannot miss it.
- A history reload keeps this session's answers that the read missed.

**Hypotheses** (section 9 of the notes):

- The `.http` file opens with language id `http`.
- `GenerateRequests` is `Send`.
- The scratch path matches the editor's path; the test home is canonicalised for this.
- The `use<>` return types compile.
- The no-proxy setting holds for loopback. `Host::new` reads it from `settings.toml` (`ide-host/src/lib.rs:167`), but the proxy setting is process-wide, so it is only safe when each test runs in its own process (nextest).

**Tests most likely to fail first:**

- The Spring gutter-mark clicks: they rely on geometry copied from the plugin gutter.
- The proxy case above.
- The re-open of `Users.java` between generate actions.
- The view tests' left-pane width.

Also, the run marks sit at the same x as plugin gutter icons and would overlap on a line that has both.
