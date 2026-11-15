# Integrating the HTTP Client into the workbench

New files, not compiled yet:

- `app-gpui/src/http_client/mod.rs`, `store.rs`, `marks.rs`, `request_toolbar.rs`, `response_view.rs`, `service_tabs.rs`
- `app-gpui/src/workbench/http_client.rs` and its tests `app-gpui/src/workbench/http_client/tests.rs` (declared by `#[cfg(test)] mod tests;` at the end of `http_client.rs`)

No existing file was edited. Every anchor below was checked with `grep -cF` to occur exactly once in its file at the time of writing; line numbers are left out. Some pasted lines run past 120 columns: run `cargo fmt` after pasting. Sections 1 to 4 are all required: `http_service_panel`, `http_editor_opened`, `http_gutter_layer`, `http_toolbar_of`, `http_files_changed`, `http_spring_rows` and `spring_group` are used only from the hunks (and tests), so without them `-D warnings` fails on dead code.

## 1. `app-gpui/src/lib.rs`

After `pub mod github;` add:

```rust
pub mod http_client;
```

## 2. `app-gpui/src/workbench/mod.rs`

### 2a. The module

After `mod github;` add:

```rust
mod http_client;
```

### 2b. The `Workbench` field

In `pub struct Workbench`, after

```rust
    plugin_contributions: plugin_contributions::PluginContributions,
```

add:

```rust
    /// The HTTP Client: request files, responses, the Spring generate marks and the endpoint
    /// providers.
    http_client: http_client::HttpClientSlot,
```

### 2c. Its initial value

In `Workbench::new`, after `plugin_contributions: Default::default(),` add:

```rust
            http_client: Default::default(),
```

### 2d. The actions

In `pub fn register_actions(cx: &mut App)`, after `database_extras::register_actions(registry);` add:

```rust
    http_client::register_actions(registry);
```

All seven ids are in `assets/gen/actions.json` (`grep -n '"id": "http\.' app-gpui/assets/gen/actions.json` lists them), so `registry.handle` takes their title, icon and `when` from there. `http.runRequest` gets `Meta+Enter` (`Ctrl+Enter` on Windows) from `assets/gen/keymaps.json`. `http.newRequest` and `http.showToolWindow` are already in the Tools menu (`menu_spec.rs`, `act("http.newRequest", "New HTTP Request")`), and `http.new` already makes the New › HTTP Request row (`context_menus.rs`, `fn http_new`). No menu entry needs adding.

### 2e. Following the editors

In the open-file task, replace

```rust
                            workbench.watch_plugin_gutter(&editor, window, cx);
```

with

```rust
                            workbench.watch_plugin_gutter(&editor, window, cx);
                            workbench.http_editor_opened(&editor, window, cx);
```

This gives a request file its toolbar and run marks and a Java/Kotlin source its generate marks. It also applies the edit waiting for a file the HTTP Client just opened: the example of New HTTP Request, or the requests appended to the scratch.

### 2f. The gutter marks' layer

In `fn render`, replace

```rust
            .children(self.plugin_gutter_layer(cx))
```

with

```rust
            .children(self.plugin_gutter_layer(cx))
            .children(self.http_gutter_layer(cx))
```

### 2g. The toolbar over a request file's editor

In `fn pane_element`, replace

```rust
            (None, None, Some(editor)) => Some(match self.sql_console_of(&editor) {
                Some(console) => console.into_any_element(),
                None => editor.into_any_element(),
            }),
```

with

```rust
            // An SQL editor is drawn inside its query console, an `.http` editor under its toolbar.
            (None, None, Some(editor)) => Some(match (self.sql_console_of(&editor), self.http_toolbar_of(&editor)) {
                (Some(console), _) => console.into_any_element(),
                (None, Some(toolbar)) => toolbar.into_any_element(),
                (None, None) => editor.into_any_element(),
            }),
```

The comment line above the original arm ("An SQL editor is drawn inside its query console, under the console's toolbar.") can go.

### 2h. A changed mapping source invalidates the Spring request paths

In `pub fn backend_event`, replace

```rust
            IdeEvent::VcsChanged { .. } | IdeEvent::FsChanged { .. } => {
                self.vcs_backend_event(&event.event, cx);
```

with

```rust
            IdeEvent::VcsChanged { .. } | IdeEvent::FsChanged { .. } => {
                if let IdeEvent::FsChanged { created, modified, removed } = &event.event {
                    let paths: Vec<String> = created
                        .iter()
                        .chain(modified)
                        .chain(removed)
                        .map(|path| path.to_string_lossy().into_owned())
                        .collect();
                    self.http_files_changed(paths.iter().map(String::as_str));
                }
                self.vcs_backend_event(&event.event, cx);
```

## 3. `app-gpui/src/workbench/services_window.rs`: the HTTP Client's tab

In `pub(super) fn mount_services`, replace

```rust
        let view = self.services_window(window, cx)?.view.clone();
        Some(Panel::new(view, None))
```

with

```rust
        let view = self.services_window(window, cx)?.view.clone();
        // Once the HTTP Client has been used, the window shows a tab per view.
        if let Some(panel) = self.http_service_panel(&view, cx) {
            return Some(panel);
        }
        Some(Panel::new(view, None))
```

The Services panel stays the bare `ServicesView` until the HTTP Client is first shown, so the existing tests that downcast `tool_window_view(SERVICES_TOOL_WINDOW)` to `ServicesView` (`workbench/tests/smoke_services.rs`, `proofs_services.rs`, `proofs3_run.rs`, `proofs5_run.rs`, `mod.rs`) are unaffected. A panel mounted before that point is swapped for the tabbed host by `wrap_services_panel`. The swap keeps the same `ServicesView` entity, and its events are subscribed by `ServicesWindow`, not by the panel.

## 4. `app-gpui/src/workbench/context_menus.rs`: the editor menu's Spring group

### 4a. The group's rows

Replace

```rust
    GroupDecl { id: "http.spring", menus: &[ContextMenu::Editor], order: 40, rows: None },
```

with

```rust
    GroupDecl { id: "http.spring", menus: &[ContextMenu::Editor], order: 40, rows: Some(super::http_client::spring_group) },
```

### 4b. The facts field

In `pub struct GroupFacts`, after

```rust
    pub run: Vec<GroupRow>,
```

add:

```rust
    /// The Spring group's rows for the menu being built: the generate actions at the caret.
    pub http_spring: Vec<GroupRow>,
```

Every other literal of `GroupFacts` uses `..GroupFacts::default()` (`context_menus.rs` tests and `context_menus/tests/proofs16_menus.rs`), so none needs changing.

### 4c. Filling it

In `fn group_facts`, after `run: self.run_menu_rows(menu, ctx, cx),` add:

```rust
            http_spring: if menu == ContextMenu::Editor { self.http_spring_rows(ctx, cx) } else { Vec::new() },
```

## 5. IPC commands the actions stand for

These are called the way the adapters in `src-tauri/src/commands/http.rs` and `nav.rs` call them:

- `http_requests`: `ide_http::parse::requests(&text)` runs on the background executor (`find_http_marks`, `run_http_at_caret`, `send_all`).
- `http_environments`: `ide_http::environments(Some(file), root)`, with `root = host.project(label).ok().map(|p| p.root)`, on a blocking thread (`load_environments`).
- `http_execute` (the send command): `ide_http::run(RunInput { text, index, file, root: Some(&project.root), env }, &cancel)`. Then `project.http_history.add` on a blocking thread, a failure logged as `http history: …` (`execute`). Unlike the adapter, the add is awaited before the answer is handed back, so a history read right after an answer lists it. The `CancellationToken` is the window's own (the adapter's `begin_request`): Stop and a newer run cancel it.
- `http_history`, `http_history_get`, `http_history_clear`: `project.http_history.list()`, `.get(&id)`, `.clear()` on a blocking thread.
- `spring_route_sources`: `ide_host::ops::routes::spring_route_sources(&host, label)` on a blocking thread, its sources handed to `RouteIndex::scanned`.
- Not called: `plugin_endpoints` (see section 8).
- The fs commands the webview used: `ide_vfs::create_dir` (`fs_create_dir`), `ide_host::ops::fs::read_dir` (`fs_read_dir`), `ops::fs::create_file` (`fs_create_file`) and `ops::fs::read_text` (`fs_read_text`, for the Spring configuration and body sources).

## 6. `parity.json` rows and their proofs

The tests are `workbench::http_client::tests::…` (run through the registry in a project window) and the view tests `http_client::<file>::tests::<name>`.

| Key                                                       | Status  | Proof                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| --------------------------------------------------------- | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `action:http.runRequest`                                  | done    | `workbench::http_client::tests::run_request_at_the_caret_sends_it_with_the_chosen_environment_and_shows_the_response`, `workbench::http_client::tests::run_request_without_a_request_in_the_file_says_so`, `workbench::http_client::tests::a_request_that_cannot_be_prepared_is_told_and_not_sent`: the caret's request reaches the loopback server with the chosen environment's variables; the answer shows on the HTTP Client tab, formatted; no request says "No HTTP request at the caret"; an unresolved variable says "Cannot send the request"                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `action:http.runAll`                                      | done    | `workbench::http_client::tests::run_all_sends_every_request_of_the_file_in_order_and_a_run_mark_sends_its_own`: both requests arrive in order and the history lists the last first; the run mark sends its own request                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `action:http.showToolWindow`                              | done    | `workbench::http_client::tests::the_http_client_lists_loads_and_clears_the_projects_history`: the Services tool window shows the HTTP Client tab with the project's history                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `action:http.newRequest`                                  | partial | `workbench::http_client::tests::new_http_request_creates_a_request_file_with_the_example_in_the_folder`: `requests.http`, then `requests_1.http`, with the example. Not proven: the scratch folder without a directory, and the hand-off to `templates.newFromTemplate` when that action is registered                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `action:http.openEndpoint`                                | done    | `workbench::http_client::tests::open_in_http_client_appends_the_endpoint_to_the_generated_requests_scratch`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `action:http.generateSpringRequest`                       | done    | `workbench::http_client::tests::generate_requests_from_the_spring_mapping_at_the_caret`: the server port from `application.properties`, the chosen path, and "No Spring request mapping at the caret"                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `action:http.generateSpringControllerRequests`            | done    | same test, plus `workbench::http_client::tests::the_spring_marks_and_the_editor_menu_offer_the_generate_actions` (the controller mark's menu)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `ipc:http_requests`                                       | done    | `workbench::http_client::tests::run_all_sends_every_request_of_the_file_in_order_and_a_run_mark_sends_its_own`: the run marks on lines 1 and 4 come from the parse                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `ipc:http_environments`                                   | done    | `workbench::http_client::tests::run_request_at_the_caret_sends_it_with_the_chosen_environment_and_shows_the_response`: the toolbar offers `dev` and `prod` from the project's env file                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `ipc:http_execute`                                        | done    | same test: the request reaches the server, and the backend history lists the entry                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `ipc:http_history`                                        | done    | `workbench::http_client::tests::the_http_client_lists_loads_and_clears_the_projects_history`: an entry saved through the backend's history only is listed                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `ipc:http_history_get`                                    | done    | same test: the click loads that entry's body from the backend                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `ipc:http_history_clear`                                  | done    | same test: after Clear History the backend's `list()` is empty                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `ipc:spring_route_sources`                                | done    | `workbench::http_client::tests::the_spring_provider_lists_the_request_paths_and_rescans_after_a_change`: the routes, with file and line, and a changed mapping rescanned. The state is read through `Workbench::http_endpoints`; no view lists it yet (see 8)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `point.registerEditorContribution:http.requests`          | done    | the run marks, the toolbar and the re-parse after edits (tests above, plus `http_client::request_toolbar::tests::runs_all_requests_and_stops_only_while_one_is_sent`, `http_client::request_toolbar::tests::chooses_an_environment_of_the_file`, `http_client::request_toolbar::tests::a_buffer_without_a_file_has_no_environments`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `point.registerEditorContribution:http.spring`            | partial | `workbench::http_client::tests::the_spring_marks_and_the_editor_menu_offer_the_generate_actions`: the marks and their menus. Missing: the Alt+Enter intentions                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `point.registerEndpointProvider:http.spring`              | done    | `workbench::http_client::tests::the_spring_provider_lists_the_request_paths_and_rescans_after_a_change`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `point.registerMenuGroup:http.spring`                     | done    | `workbench::http_client::tests::the_spring_marks_and_the_editor_menu_offer_the_generate_actions`: the editor menu's path submenu with `{ path }` arguments, and the controller row                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `point.registerServiceView:httpClient`                    | partial | `workbench::http_client::tests::run_request_at_the_caret_sends_it_with_the_chosen_environment_and_shows_the_response`, `workbench::http_client::tests::the_http_client_lists_loads_and_clears_the_projects_history`, `http_client::service_tabs::tests::switches_between_the_services_and_the_http_client`, `http_client::response_view::tests::shows_the_empty_history_and_the_request_being_sent`, `http_client::response_view::tests::lists_the_history_and_a_click_selects_an_entry`, `http_client::response_view::tests::formats_the_body_until_show_raw_is_pressed`, `http_client::response_view::tests::shows_the_headers_and_the_request_as_sent`, `http_client::response_view::tests::describes_binary_bodies_and_failed_requests`, `http_client::response_view::tests::the_header_actions_ask_for_their_work`. The view is a tab of the Services tool window from its first use on; its header buttons sit in the tab row. Missing: the resizable split, selectable text, a spinner (a static refresh icon is drawn) |
| `feature:http-client`                                     | partial | the rows above; missing: highlighting, URL completion, the endpoint gutter, intentions                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `unit:src/features/http-client/spring/generate.test.ts`   | done    | 1 of 1 case: `features_a::http_client::generate::tests`, reached by the generate actions                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `unit:src/features/http-client/spring/spring.test.ts`     | done    | 17 of 17 cases: `features_a::http_client::spring::tests`, `features_a::http_client::request::tests`, all reached by the generate actions and the marks                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `unit:src/workbench/endpointProviders.test.ts`            | done    | 2 of 2 cases: `platform_more::endpoint_providers::tests`, reached through the window's providers (`HttpClientSlot::providers`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `unit:src/features/http-client/spring/routeIndex.test.ts` | done    | 6 of 6 cases: `features_a::http_client::route_index::tests`, reached through `Workbench::http_endpoints`, which passes the workspace's member folders (`ide_host::ops::project::current(..).workspace.projects`, as `workbench/agents_views.rs` `workspace_members` reads them). No end-to-end test opens a workspace                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `unit:src/features/http-client/endpoints.test.ts`         | partial | 1 of 3: `renders_optional_handlers_and_any_method_routes_into_http_blocks`, reached by `http.openEndpoint`. The completion cases are not reached: there is no URL completion                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `unit:src/features/http-client/httpClient.test.ts`        | partial | 5 of 18: the response-body cases (`features_a::http_client::format::tests`) through the view. The 13 highlighting cases (`highlight::tests`) are not reached                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |

Not earned: `ipc:plugin_endpoints`, `point.registerEditorContribution:http.endpoints`, `point.registerLanguageMode:(at) src/features/http-client/index.ts:122`, `point.registerMenuGroup:http.new` (rows existed before; no test here goes through the Project view's New menu). The inventory has no `settings:` row for this feature.

The other `endpoints.test.ts` model also exists: `features_a::http_client::endpoints::EndpointProviders`, a provider list by id. The app reaches `platform_more::endpoint_providers::EndpointProviders` instead. The first stays model-only.

## 7. Views (constructor, events, accessors)

- `HttpClientView::new(cx)`, fed by `set_state(history, selected, exchange, running_index, cx)`.
  - Events: `HttpClientViewEvent::{Select(id), Stop, ReloadHistory, ClearHistory}`.
  - Accessors: `rows()`, `running_text(cx)`, `is_empty()`, `is_loading()`, `tab()`, `tabs(cx)`, `select_tab`, `raw_toggle()`, `toggle_raw`, `status_line()`, `request_line()`, `body()`, `header_rows()`, `request_text()`, `header_actions()`, `press(id)`.
- `RequestToolbar::new(path, editor_view, cx)`, fed by `set_state(environments, chosen, running, cx)`.
  - Events: `RequestToolbarEvent::{RunAll, Stop, Choose(Option<String>), Reload}`.
  - Accessors: `toolbar()`, `options()`, `chosen()`, `is_chooser_open()`, `press`, `choose`.
- `ServiceTabs::new(services_view, services_focus, http_view, active)`.
  - Accessors: `tabs()`, `active()`, `select`, `header_actions(cx)`, `http()`.
- Models: `HttpClientStore` (store.rs); `request_marks`, `request_at_line`, `spring_marks`, `spring_rows`, `spring_targets`, `line_of` (marks.rs).

## 8. Left out, and why

- **Highlighting of `.http` files.** `features_a::http_client::highlight` is not used by any file outside `features_a` (`grep -rln "http_client::highlight" app-gpui/src` finds only `endpoints.rs`, for `METHODS`). The editor builds its spans from the backend's highlight answer (`editor/view.rs` around lines 922 and 1468). Hypothesis: there is no hook for a feature's highlighter. Check `grep -n "highlights" app-gpui/src/editor/view.rs`.
- **URL completion from the endpoints (`endpointCompletion`).** Not built. Hypothesis: the editor's completion (`CompletionSource`, `editor/intelligence.rs:86`) takes only the language server's answers. Check by reading `CompletionSource` in `app-gpui/src/editor_lsp/`.
- **Endpoint gutter marks (`endpointEditorExtension`) and `plugin_endpoints`.**
  - The marks show only endpoints with a `pluginId` (`endpoints.ts:119`).
  - Those come from the plugins feature's `endpoints` UI point (`src/features/plugins/points.tsx:157`, `providePluginEndpoints`), not from this feature.
  - With no plugin provider the marks would never show, and no plugin fixture reports endpoints.
  - Adding it needs one provider per plugin fed by `ide_host::plugins::index_ops::plugin_endpoints`, and `EndpointProviders<SpringProvider>` turned into an enum of providers.
- **The Spring intentions (⌥↵).** Not built. The intentions list is built from code actions (`editor/intelligence.rs:26-30`, `editor_lsp::intentions::listed_intentions`). Hypothesis: a feature cannot add rows to it. Check `app-gpui/src/editor_lsp/intentions.rs:154`.
- **Search Everywhere's Endpoints tab.** `everywhere/view.rs` answers `SeFetch::Routes` with `SeAnswer::Failed` (line 361) and builds `Sources` without `routes` (line 216). Feeding it `Workbench::http_endpoints` is a hunk in that view and in the workbench's Search Everywhere opener. It belongs to the search slice.
- **Marks between an edit and the next parse.** The run and generate marks keep their old lines until the re-parse 300 ms (Spring: 400 ms) after the last edit. The webview maps them through each edit at once.
- **Two appends while the scratch is still opening.** The second replaces the pending first (`pending_edits` holds one edit per path), so only the second is appended.
- **The time of a history row** is shown as an English clock only (`clock_time`), with no locale.
- **Selectable response text, the split handle and a spinning indicator** are not built: no native primitive for them is used here.
- **The Run All button's tooltip** shows no shortcut: the action has none by default (`assets/gen/keymaps.json`, `"http.runAll", []`).
- **A history read keeps this session's answers.** `HttpClientStore::set_history` keeps an answered entry the read missed, so a reload racing an answer cannot drop it. The webview replaces the list.
- **The tests file `app-gpui/src/workbench/http_client/tests.rs`.** The assignment did not name this file. It holds the module's end-to-end tests, so `http_client.rs` stays readable.

## 9. Hypotheses (not checked without a compiler or a run)

Checked by reading, so not hypotheses:

- A scratch outside the project opens as a text tab: `open_text_tab` (`workbench/tab_views.rs:633`) has no root check.
- The scratches folder is the host's `data_dir/scratches` (`crates/ide-host/src/project.rs:546`). That is `dirs.data` (`backend.rs:238`), as `scratch_file` builds it.
- `### Users` names the request: `parse.rs:156` takes the separator's title as the name.
- The unresolved-variable error names the variable: `exec.rs:79`, "unresolved variable(s): nowhere …".
- Group rows reach the editor menu unchanged: `context_menus.rs` `editor_menu`, `items.extend(into_menu(groups))`.

1. **The `.http` language.** `languages/http.toml` (`id = "http"`) is known to the backend in tests, so an `.http` editor's `language()` is `http` and `editorLangId == http` holds. Java and Kotlin likewise give `java` and `kotlin`.
2. **No proxy for loopback.** Read, not run: the fixture writes `[proxy]\nmode = "none"` into the test home's `settings.toml`. `Host::new` loads that file (`crates/ide-workspace/src/settings.rs:166`, from `HostConfig.config_dir` = `dirs.config`, `app-gpui/src/backend.rs:237`) and calls `apply_proxy` (`crates/ide-host/src/lib.rs:167`). Still a hypothesis: `set_proxy` is process-wide, so under `cargo test` (one process) another test's backend can reset it to `Auto`. Under nextest each test is its own process.
3. **Scratch path equality.** The editor's `path()` of the scratch equals `scratch_file(dirs.data)`. The fixture canonicalises the home for this. If the backend canonicalises differently, `pending_edits` misses the editor and the scratch opens empty.
4. **`GenerateRequests` is `Send`.** It moves into a backend task. Read `BodyLookup` and `Load` in `request.rs` if the compiler says otherwise. The fix is to read the files one host call at a time.
5. **`templates.newFromTemplate` is not registered** in the test fixture (`editor_extras.rs` asserts it is absent), so New HTTP Request with a directory creates the file itself.
6. **`execute`'s return type.** `impl Future + 'static + use<>` compiles. `on_host`'s `use<T, F, W>` mirrors `Backend::with_host`.
7. **The panel's focus closure.** `(panel.focus)(app)` with `let app: &App = cx;` coerces.
8. **`AsyncWindowContext::background_executor()`** is reachable through its deref to `AsyncApp`.
9. **Reading the workspace members on the main thread.** `ide_host::ops::project::current` is a cheap in-memory read. `workbench/agents_views.rs:38` already calls it from the main thread.

## 10. Tests most likely to fail first

- **`the_spring_marks_and_the_editor_menu_offer_the_generate_actions`.** The mark is clicked at its painted bounds (`http-gutter-8`), which depends on the gutter geometry copied from `plugin_gutter_layer`. The menu's row selectors are `menu-row-0-<n>`.
- **`run_request_at_the_caret_…`.** Hypothesis 2 (proxy). The header case: reqwest sends `authorization` lowercased, and the test compares in lower case.
- **`generate_requests_from_the_spring_mapping_at_the_caret`.** Re-opening `Users.java` must make it the active editor again before each action. The `### Users.` count assumes four generated blocks.
- **`the_spring_provider_lists_…`.** The routes' order is the scan's (by path, then by mapping). `rel` is relative to the project root.
- **View tests drawing the history.** `relative(0.32)` and `min_w` on a flex child: if a row is not drawn, check the left pane's width.
- **Mark placement.** The marks sit at the same x as plugin gutter icons (`GUTTER_INSET` 14 against the plugins' 13). On a Java file a plugin icon on the same line would overlap.
