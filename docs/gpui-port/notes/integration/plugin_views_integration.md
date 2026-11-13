# Integrating plugin views and plugin tool windows

New files, not compiled yet:

- `app-gpui/src/plugin_views/view_node.rs`: the body model and drawing (tree, list, form, table, message, the html body as an unsupported state), as `impl PluginViewPanel` blocks, with plain and `#[gpui_kit::test]` tests.
- `app-gpui/src/plugin_views/view_panel.rs`: `PluginViewPanel` (toolbar, error banner, status line, loading and failure states, render sequencing) and `PluginToolWindowView` (the tabs of a group). Its `tests` module is `pub(super)` because `view_node.rs` tests reuse `open`, `click`, `right_click` and `events_of`.
- `app-gpui/src/workbench/plugin_tool_windows.rs`: the `impl Workbench` wiring, the declaration model with plain tests, and the end-to-end tests.

No existing file was edited. Every anchor below is quoted from the file as it is now, and was checked with `grep -cF` to occur exactly once. Line numbers are left out because `mod.rs` is moving. Sections 1 to 5 are all required: without section 4, `plugin_editor` and `static_action_id` are private, and without section 3, the three plugin events still go to the debug arm and the new `pub(super)` functions are dead code under `-D warnings`. Run `cargo fmt` after pasting, since a few lines run past 120 columns.

## 1. `app-gpui/src/plugin_views/mod.rs`

Replace the whole file (6 lines) with:

```rust
//! Views of what plugins show: the questions a waiting command asks, the log tabs plugin code opens,
//! their pages in Settings, and their declared views in tool windows.

pub mod log_tab;
pub mod prompt_dialog;
pub mod settings_page;
pub mod view_node;
pub mod view_panel;
```

## 2. `app-gpui/src/workbench/mod.rs`

### 2a. The module

After `mod plugin_runtime;` add:

```rust
mod plugin_tool_windows;
```

### 2b. The field

In `pub struct Workbench`, after

```rust
    plugin_contributions: plugin_contributions::PluginContributions,
```

add:

```rust
    /// The tool windows of the views plugins declare, their panels and what they wait on.
    plugin_tool_windows: plugin_tool_windows::PluginToolWindows,
```

### 2c. Its initial value

In `Workbench::new`, after `plugin_contributions: Default::default(),` add:

```rust
            plugin_tool_windows: Default::default(),
```

### 2d. Mounting a plugin window's view

In `fn mount_panels`, replace the arm (the only one with this indentation)

```rust
                _ => continue,
```

with

```rust
                // A plugin's view, or nothing this app draws.
                other => match self.mount_plugin_tool_window(other, window, cx) {
                    Some(panel) => panel,
                    None => continue,
                },
```

`id` is a `String` that the match borrows as `id.as_str()`, so `other: &str`. `mount_plugin_tool_window` takes `&mut self`. This is the same shape as `DATABASE_TOOL_WINDOW => match self.mount_database(window, cx)`, so the outer borrow of `self.content` has already ended at that point.

### 2e. Registering them when a project opens

In `open_project`'s completion closure, directly before

```rust
                // A restored tab gets its editor when it is the one showing.
```

insert:

```rust
                // The views plugins declare register after the IDE's own windows, reconciled the same way.
                workbench.mount_plugin_tool_windows(window, cx);
```

It must come after the `with_layout` that registers `TOOL_WINDOWS`, because `sync_plugin_tool_windows` returns early unless `self.content` is `Content::Project`.

### 2f. Dropping them with the project

`self.drop_agents(cx);` occurs twice, so each insert is anchored on the line that follows it.

- In `close_project`, replace

```rust
        self.drop_agents(cx);
        // The project's servers and runs stop in the background: this is the UI thread.
```

with

```rust
        self.drop_agents(cx);
        self.drop_plugin_tool_windows();
        // The project's servers and runs stop in the background: this is the UI thread.
```

- In `open_project`, replace

```rust
        self.drop_agents(cx);
        self.files = Default::default();
```

with

```rust
        self.drop_agents(cx);
        self.drop_plugin_tool_windows();
        self.files = Default::default();
```

## 3. `app-gpui/src/workbench/plugin_runtime.rs`: the three events

There is one catch-all arm, not three. In `fn plugin_runtime_event`, replace

```rust
            // Only the kind is logged: such events carry what plugin views and pages exchange.
            other => tracing::debug!("plugin event not shown in this window: {}", plugin_event_kind(other)),
```

with

```rust
            PluginEvent::ViewChanged { plugin_id, view } => self.plugin_view_changed(plugin_id, view, cx),
            PluginEvent::RevealView { plugin_id, view } => self.reveal_plugin_view(plugin_id, view, window, cx),
            PluginEvent::ViewMessage { plugin_id, view, .. } => self.plugin_view_message(plugin_id, view),
            // Only the kind is logged: such events carry what plugin views and pages exchange.
            other => tracing::debug!("plugin event not shown in this window: {}", plugin_event_kind(other)),
```

`ViewChanged` and `ViewMessage` are emitted for every window (`client` is `None`). `RevealView` is emitted for the window of the command, and the early `client != self.label` return already filters it. `RuntimeUi::handle` returns `None` for all three (its `_ => return None`), so they do reach this match.

## 4. Visibility for the sibling module

- `app-gpui/src/workbench/plugin_runtime.rs`: replace `    fn plugin_editor(&self, cx: &App) -> Option<PluginEditor> {` with `    pub(super) fn plugin_editor(&self, cx: &App) -> Option<PluginEditor> {`. Renders and events carry the window's focused editor, as `plugin_view_render`'s `editor` argument does.
- `app-gpui/src/workbench/plugin_contributions.rs`: replace `fn static_action_id(id: &str) -> &'static str {` with `pub(super) fn static_action_id(id: &str) -> &'static str {`. The view item menu's action rows need a `&'static str` id.

## 5. Context keys (no hunk)

`activeToolWindow` (the plugin window's id while its panel has the keyboard) and `toolWindow.<id>.visible` come from `publish_context` for every registered window. When a plugin window is withdrawn, `sync_plugin_tool_windows` clears its `toolWindow.<id>.visible`, because nothing else would. Tests read both keys ON and OFF (`a_plugins_view_is_a_tool_window_…`, `disabling_the_plugin_…`, `reveal_shows_…`).

## 6. The tool window registry (no hunk)

`ToolWindowLayout::register` / `unregister` already take entries that are not in `TOOL_WINDOWS`, and a `Registration` handle is kept per plugin window. The stripe button runs `toolwindow.<id>`, so `sync_window_actions` registers that action per plugin window with `registry.register(Action::new(…))`. It is not in `assets/gen/actions.json`, so `handle` would panic. The action is unregistered when the window's declaration goes. These actions are process-wide, as plugin declarations are, and act in the window they run in.

## 7. Fixtures

No new fixture is needed. All three below have `dist/plugin.wasm` checked in, and the tests copy them with `plugin_copy` into the temporary home:

- `crates/ide-plugin/tests/fixtures/hello`: view `greetings` (title "Greetings", icon `plugin`, `location = "right"`), a tree whose Expand calls `refresh_view`, Activate notifies the greeting, and the "Collapse All" button. One test appends a view the code rejects:

```toml
[[contributes.views]]
id = "broken"
title = "Broken"
location = "bottom"
```

- `tables`: a sortable table, `table-select` / `table-activate` reported in its status line, and the `inspect` command on `viewItem.context`.
- `grouped-views`: an ungrouped `legacy` view, the group `demo` with `when` clauses, and the `show` / `hide` / `reveal` commands.

If a fixture's source ever changes, rebuild it as `plugins/build.sh` documents: `plugins/build.sh crates/ide-plugin/tests/fixtures/<name>` ("one plugin (a path under plugins/, or any path)"). `plugins/build.sh --check` fails on a stale component. The output is `<plugin>/dist/plugin.wasm` plus `dist/plugin.wasm.sources`.

## 8. Assumptions (unverified, check first)

1. **The error text of the rejected view contains "broken".** `PluginHost::code_call` maps the guest's `Error::NotFound("broken")`, which I assume keeps the text. The test opens Greetings first so the plugin is active (its activation is `on-view:greetings`); otherwise the host may refuse with "not running" instead. If it fails, relax the assertion to `error().is_some()`.
2. **`link` leaves the plugin `Inactive`.** The first test asserts this before the window is opened. `grant` may activate an `on-command:*` plugin; if so, drop that assertion. The behaviour it guards (no render before the window is shown) holds by construction, since rendering only happens in `ensure_plugin_view`.
3. **`set_enabled` followed by `Plugins::refresh`.** The listing then drops the view, and the `observe_global_in::<Plugins>` observer syncs.
4. **`observe_global_in::<ContextStore>` fires after every `set_context`**, because `default_global` pushes `NotifyGlobalObservers`. The observer compares `context_changes` before acting, so it does not loop with the redraw.
5. **Clicking a tree row with `on_mouse_down` + `window.prevent_default()` keeps the focus on the panel.** This is the rule from the view templates. `key_down` acts only while the panel root itself has the keyboard, so typing into a form or cell input never reaches the tree's speed search.
6. **`Localization` defaults to English in tests**, so `tx_built(&["Delete ", " Rows"], …)` yields "Delete 2 Rows" (catalog key "Delete {length} Rows").
7. **The `uniform_list` scroll offset is read in `render`** to find the table rows shown. Rows the render lacks are asked for in `cx.defer_in`, never from inside the list's own processor.

## 9. Compile risks

- **`view_node.rs` extends `PluginViewPanel` from its own module.** It uses `pub(super)` fields of `view_panel.rs`, and `view_panel.rs` uses `pub(super)` methods of `view_node.rs` (`sync_body`, `body_element`, `tree_key`, `list_key`, `table_key`). Both are children of `plugin_views`, so this resolves.
- **Closures that may need annotation.**
  - The `cx.processor` closures capture `Vec`s by move and must be `Fn`. They only index them, but this is unverified.
  - `cell.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())` is a plain (non-listener) handler. If inference fails, annotate it as `|_: &MouseDownEvent, _: &mut Window, cx: &mut App|`.
- **`PluginWindowEvent::Selected(view): &PluginWindowEvent` in a test closure parameter** is an irrefutable pattern, since the enum has one variant. If that is refused, match it inside the closure.
- **`ToolWindow { order, secondary, verbatim, ..ToolWindow::new(…) }`** relies on every field of `ToolWindow` being `pub`; they are.
- **`Anchor` is `Copy + PartialEq`** (derived), so `Signature`'s `PartialEq` holds.

## 10. Tests most likely to fail first

- **`a_plugin_table_sorts_selects_and_offers_its_row_command`.**
  - It needs `menu.lines()[0]` (the first menu level) to list "Inspect Row" as the action title.
  - It needs the plugin command to get `"0"` as its argument from `MenuItem::action_with(.., Value::String("0"))`.
  - If the menu's lines carry a shortcut suffix, match with `starts_with`.
- **`reveal_shows_and_focuses_…`.**
  - The group appears only after the plugin's `ready` context key reaches the window, which takes `ContextChanged` → `FetchContextKeys` → `SetContext`.
  - That round trip and the reveal's focus are both waited for, but "a hidden view stays hidden" is asserted after a single `run_until_parked`.
- **`the_tree_keys_select_open_close_and_search_as_the_tree_does`.** It needs `simulate_keystrokes("b e")` to deliver `key_char` values, and `f4` to be named `f4`.
- **`a_form_sends_typed_text_ticked_boxes_and_chosen_options`.** It assumes clicks on non-focusable rows do not move the focus. The assertions allow an extra blur commit.

## 11. What is not built, and why

- **`html` bodies, `web = …` views, `post-message` / `ViewMessage` and the `message` view event.** `app-gpui/src/web_view.rs` (`WebSurface`, wry) exists but is unused anywhere and cannot run in GPUI test windows. These show the IDE's runtime text "This view needs a newer version of OpenIDEA (html)." and never paint the HTML. A follow-up would host `frameDocument` (CSP, `openidea.postMessage` bridge, theme posts; `src/features/plugins/views/frame.ts`) in a `WebSurface`.
- **`location = "services"` views (the Services tool window's plugin providers) and `detail = true` panes (`ServicePane`).**
  - `services_window.rs` has no plugin provider; its plugin commands only log.
  - A `detail` view is drawn as a plain tree or list.
  - Without the detail pane, the list's arrow and Space keys (which the webview has only with `detail`) are absent, as in the webview's non-detail mode.
- **Plugin text catalogs (`localizeView`).** No `PluginCatalogs` is loaded in the GPUI app; plugin text shows as written.
- **Custom editors (`doc`).** Out of scope, so `ipc:plugin_view_*` rows stay partial.
- **URLs.** No node kind of `wit/views.wit` carries or opens a URL, so no URL path exists to guard. `open-url` from plugin code keeps going through `RuntimeUi`'s http(s) check and `cx.open_url`.
- **Re-rendering on a plugin reload.** A reload that keeps a view's declaration keeps its window and content, as the webview's projection does (it re-projects only on a changed declaration or position).
- **Not tested here.**
  - Keyboard focus on sortable headers (Enter / Space on a header) and the table's keyboard context-menu key other than Shift-F10.
  - The SVG view icon (implemented: `assets("viewIcon/<id>")` → `register_external_icons(standalone(svg))`).
  - Moved placements across re-registration.

## 12. Untrusted input

- **Caps.** The host caps items at 20,000, table rows at 2,000, columns at 1,000 and buttons at 32, and the views repeat the item and column caps.
- **Trees.** They are laid out with an explicit stack (`tree_rows`), with no recursion and no nested drops, and the indent is capped at depth 100. A parent chain of 20,010 items is tested.
- **Tables.** `total_rows` is drawn up to 1,398,100 rows, the rows a browser can lay out at 24 px. All `first_row` / `total_rows` arithmetic saturates (tested with `u32::MAX`). The webview itself has no explicit caps.
- **Text.** Plugin text is drawn as text with control characters shown as spaces (`display_text`, tested with markup).
- **Icons.** An unknown icon name draws `file`, as `Icon` does (tested).
- **Logs.** No payload is logged: `plugin_view_message` logs only the plugin and view ids.

## 13. IPC commands the wiring calls

- **`plugin_view_render`**: authorized under that name, then `PluginHost::render_view` with `ide_host::plugins::views::view_invocation(host, label, None, focused editor)` on the blocking pool (`crate::plugins::host_blocking`). This is what `ide_host::plugins::views::render` does inside its `spawn_blocking`.
- **`plugin_view_event`**: the same, with `PluginHost::view_event`.

## 14. Parity rows the tests prove

| `docs/gpui-port/parity.json` key                                               | Proof                                                                                                                                                                                                                                                                                      | Status                                                                                                                                                                                                                                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ipc:plugin_view_render`                                                       | `workbench::plugin_tool_windows::tests::a_plugins_view_is_a_tool_window_with_its_title_and_icon_that_shows_the_rendered_rows`, `…::a_view_that_fails_to_render_says_the_plugin_could_not_show_it`                                                                                          | partial: no custom editor (`doc`)                                                                                                                                                                                                                                                         |
| `ipc:plugin_view_event`                                                        | `…::clicking_a_node_reaches_the_plugin_and_its_refresh_repaints_the_view`, `…::a_plugin_table_sorts_selects_and_offers_its_row_command`                                                                                                                                                    | partial: no custom editor (`doc`)                                                                                                                                                                                                                                                         |
| `wit:wit/views.wit#view-types`                                                 | `plugin_views::view_node::tests` (every body and event kind), the e2e tests above                                                                                                                                                                                                          | partial: `html` body and `message` event not drawn                                                                                                                                                                                                                                        |
| `wit:wit/views.wit#view-provider`                                              | the four e2e tests above                                                                                                                                                                                                                                                                   | partial: custom editors                                                                                                                                                                                                                                                                   |
| `wit:wit/views.wit#views`                                                      | `…::clicking_a_node_reaches_the_plugin_and_its_refresh_repaints_the_view` (refresh), `…::reveal_shows_and_focuses_the_views_window_and_a_hidden_view_stays_hidden` (reveal), plus the existing proofs                                                                                      | partial: post-message has no page; open-terminal(-session), open-content-tab and reply-content-tab unchanged                                                                                                                                                                              |
| `point.registerToolWindow:(at) src/features/plugins/views/viewWindows.tsx:118` | `…::a_plugins_view_is_a_tool_window_…`, `…::reveal_shows_…`, `…::disabling_the_plugin_removes_its_tool_window`, `…::closing_the_project_drops_its_plugin_windows`                                                                                                                          | partial: `web =` views show the unsupported state                                                                                                                                                                                                                                         |
| `point.registerServiceView:(at) src/features/plugins/uiPoints.tsx:372`         | none                                                                                                                                                                                                                                                                                       | not done: Services providers not built                                                                                                                                                                                                                                                    |
| `unit:src/features/plugins/__tests__/views.test.tsx`                           | `plugin_views::view_node::tests`, `plugin_views::view_panel::tests`; 14 of 26 here (+2 already in `plugin_models::view_helpers::tests`)                                                                                                                                                    | partial: missing are the 4 HTML frame cases (CSP, frame theme, theme document, posted themes), the HTML theme switch, keymaps, command-argument unpacking, context-menu projection, submenu grouping (the menu row only, here untested) and zero-based coordinates (other rows' subjects) |
| `unit:src/features/plugins/__tests__/viewWindows.test.tsx`                     | `plugin_views::view_panel::tests::collects_one_plugins_views_in_declaration_order_and_renders_only_the_selected_tab`, `workbench::plugin_tool_windows::tests::{keeps_other_plugins_…, uses_the_first_group_members_split_…, keeps_legacy_ungrouped_window_ids_…, reveal_shows_…}`; 8 of 12 | partial: SVG icon updates, the translated-IDE label case (verbatim flag only), moved placement across reload, and context reaction with active tab preserved (partly)                                                                                                                     |
| `unit:src/features/plugins/__tests__/viewProjection.test.tsx`                  | `…::a_plugins_view_is_a_tool_window_…` (no activation before shown); 1 of 6                                                                                                                                                                                                                | partial: order on single-view change, manifest reorder, other plugins' declarations, Services conditions, SVG not resurrecting (implemented, untested)                                                                                                                                    |

Spec cases covered by name (snake_case of the spec sentence):

- `builds_the_tree_from_the_flat_item_list`
- `asks_for_the_visible_table_rows_plus_some_around_them`
- `renders_a_tree_and_sends_expand_activate`
- `opens_the_item_menu_on_right_click`
- `opens_the_item_menu_on_a_list_row_and_none_when_it_has_no_entries` (the "none" half is in `show_plugin_item_menu`)
- `edits_a_table_cell_and_renders_again_when_the_plugin_refreshes`
- `sorts_a_sortable_table_from_its_column_headers_inverting_on_a_second_click`
- `keeps_plain_table_headers_inert`
- `shows_a_message_when_the_plugin_fails`
- `shows_selectable_labels_and_readonly_text_without_sending_field_changes`
- `keeps_editable_table_row_actions_beside_contributed_commands`
- `uses_absolute_row_indices_after_paging_and_supports_keyboard_context_menus`
- `sends_absolute_row_events_and_contributed_context_commands` (both `sortable` values in one loop)
- `collects_one_plugins_views_in_declaration_order_and_renders_only_the_selected_tab`
- `keeps_other_plugins_and_ungrouped_views_separate_even_when_their_ids_equal_a_group`
- `keeps_legacy_ungrouped_window_ids_and_removes_conditional_views_until_they_apply`
- `uses_the_first_group_members_split_even_when_hidden_and_keeps_views_primary_unless_secondary_is_true`

## 15. What `plugin_tool_windows.rs` holds (for orientation)

- **Declarations.** `declared_views` / `window_specs` (pure): window ids `plugin.<p>.<v>` and `plugin.<p>.group:<g>`, the `when` filter, and the first member's title, side, split and icon.
- **State.** `PluginToolWindows`: registrations, chosen tabs, containers, view slots (panel plus a FIFO of events with one in flight), SVG icons, and observers.
- **Sync.** `mount_plugin_tool_windows` / `sync_plugin_tool_windows` / `drop_plugin_tool_windows`, plus `sync_window_actions` (`toolwindow.<id>`).
- **Panels.** `mount_plugin_tool_window` (the `mount_panels` arm) and `ensure_plugin_view`; rendering and events through `render_plugin_view`, `queue_plugin_view_event` / `send_next_plugin_view_event` and `plugin_view_event`.
- **Menus.** `show_plugin_item_menu` with `view_item_menu` (`viewItem` / `viewItemKind`, submenus, and an editable table's row commands).
- **Plugin events.** `plugin_view_changed`, `reveal_plugin_view` and `plugin_view_message`.
