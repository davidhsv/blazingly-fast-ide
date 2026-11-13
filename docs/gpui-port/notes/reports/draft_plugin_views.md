I've drafted native plugin views and plugin tool windows: three new Rust files plus the notes file. None of it has been compiled or run, and no existing file was touched. Everything below is drafted, not proven.

**Files written**

- `app-gpui/src/plugin_views/view_node.rs` (1714 lines): lays out and draws each body kind, handles pointer and keyboard on nodes, and holds its tests.
- `app-gpui/src/plugin_views/view_panel.rs` (673 lines): `PluginViewPanel` (toolbar, error banner, status line, loading and failure states) and `PluginToolWindowView` (a tab per view for grouped views).
- `app-gpui/src/workbench/plugin_tool_windows.rs` (1016 lines): the `impl Workbench` wiring, the declaration model, and the end-to-end tests.
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/plugin_views_integration.md` (276 lines): hunks, fixtures, assumptions, compile risks and the parity table.

**Node kinds: 5 of the 6 bodies, 13 of 14 events**

- **Bodies drawn:** message, tree, list, form (label, read-only text, text, multiline, checkbox, choice) and table (editable, sortable, paged, row commands).
- **`html` body:** shown as an explicit unsupported message, never painted.
- **Events:** all except `message`. That one comes only from an HTML page, which isn't drawn.
- **Tree keyboard:** as the webview's tree component — arrows, Home/End, PageUp/PageDown, Enter/F4/⌘↓, and speed search. Enter or double-click on an expandable row toggles it.
- **Expansion:** stays under the plugin's control, so nothing opens until the plugin renders again.

**Tests (about 30)**

- **Unit tests in `view_node.rs`:** tree building, a 20,010-deep parent chain, text and icon fallback, viewport arithmetic.
- **GPUI tests on hand-built trees:** every body kind and event, including the tree keyboard and the table's Enter, Space, Shift-F10 and Tab.
- **Panel tests:** toolbar, failure state, dropping a stale render, tabs.
- **Pure tests of the declaration model:** groups, `when`, `secondary`.
- **Seven end-to-end tests** using the shared harness:
  - the view appears as a tool window with its title and icon, and the plugin doesn't run before it is opened;
  - its rows show, and the `activeToolWindow` and visibility keys go ON and OFF;
  - expanding reaches the plugin, which refreshes and repaints; double-clicking a greeting raises its notification; Collapse All works;
  - a render failure shows the webview's text;
  - disabling the plugin removes the window, its panel, its action and its key;
  - closing the project drops everything;
  - a table sorts, selects, and runs its row-menu command;
  - reveal shows and focuses the window, keeps a hidden view hidden, and handles groups appearing and disappearing on `when`.

**Fixtures:** none new. `hello`, `tables` and `grouped-views` already have `dist/plugin.wasm` checked in and are copied per test. The render-failure test appends a view that `hello`'s code rejects to the copied manifest. The rebuild step (`plugins/build.sh <path>`) is quoted in the notes.

**Hunks needed in compiled files (exact text in the notes)**

- `plugin_views/mod.rs`: the two `pub mod` lines.
- `workbench/mod.rs`:
  - the `mod` line, the field and its initial value;
  - a catch-all arm in `mount_panels` replacing `_ => continue`;
  - a call at project open after the static windows register;
  - `drop_plugin_tool_windows()` in `close_project` and `open_project`.
- `plugin_runtime.rs`: there is one catch-all debug arm, not three. The three new arms (`ViewChanged`, `RevealView`, `ViewMessage`) go above it. `plugin_editor` becomes `pub(super)`.
- `plugin_contributions.rs`: `static_action_id` becomes `pub(super)`.
- **No registry change is needed:** `register`/`unregister` already accept dynamic entries.
- **Stripe actions:** the stripe button runs `toolwindow.<id>`, so that action is registered per plugin window with `registry.register` (`handle` would panic on an id outside the manifest).

**What the webview does that I didn't build**

- **HTML bodies, `web =` views, `post-message`:** `app-gpui/src/web_view.rs` exists but nothing uses it, and it can't run in GPUI test windows.
- **Services-location views and `detail` panes:** the Services tool window has no plugin provider yet. Without the detail pane, lists have no arrow or Space keys, which matches the webview's own non-detail mode.
- **Plugin text catalogs:** none are loaded in the GPUI app, so plugin text shows as written.
- **Custom editors:** out of scope.
- **Re-render on a reload that keeps the declaration:** like the webview, the window and its content stay.
- **URLs:** no node kind in the view API carries one, so there is no URL path to guard.
- **Size limits:** the webview has none of its own. I cap items at the host's 20,000, cap the tree indent at depth 100, and draw at most 1,398,100 table rows (what a browser can lay out at 24 px each). All row arithmetic saturates.

**Assumptions to check first**

1. The rejected view's error text contains "broken"; otherwise relax that assertion.
2. `link` leaves the plugin `Inactive`.
3. `observe_global_in::<ContextStore>` fires on every `set_context`.
4. `tx_built(&["Delete ", " Rows"], …)` gives "Delete 2 Rows".
5. In the table test, the first level of the open menu (`menu.lines()[0]`) lists "Inspect Row".
6. The SVG view-icon path and moved placements across re-registration are implemented but not tested.

The tests most likely to fail first are the table menu, reveal-with-groups, tree keyboard and form tests, for the reasons listed in the notes.
