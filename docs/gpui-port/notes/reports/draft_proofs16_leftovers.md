I wrote 13 new tests in four declared files plus 3 pending tests, and nothing has been compiled or run. No existing file was edited. The `mod` lines, call sites and full per-case table are in the notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs16_leftovers.md`.

**Files** (under `app-gpui/src/`)

| File                                                      | Lines | Tests | Declared                           |
| --------------------------------------------------------- | ----- | ----- | ---------------------------------- |
| `workbench/plugins_window/tests/proofs16_plugins.rs`      | 214   | 5     | after `mod proofs15_plugins;`      |
| `workbench/plugin_runtime/tests/proofs16_runtime.rs`      | 117   | 3     | after `mod proofs14_plugins;`      |
| `plugin_models/plugins_page/tests/proofs16_page.rs`       | 100   | 2     | after `mod proofs14_page;`         |
| `workbench/context_menus/tests/proofs16_menus.rs`         | 90    | 3     | after `mod proofs7_toolbar_slots;` |
| `workbench/context_menus/tests/proofs16_menus_pending.rs` | 23    | 1     | no (pending)                       |
| `workbench/tests/proofs16_todo_run_pending.rs`            | 81    | 2     | no (pending)                       |

**Part 3, `plugins.test.tsx`:** there are 61 cases; proofs14 counted 59 because it missed the two homepage cases.

- **Existing tests alone:** 17 cases are fully proven through the application and 13 in part. Another 17 are proven only on the Plugins page mounted alone, with no host running. 12 are model only or missing, and 2 have no native counterpart.
- **So the "44" estimate is too high:** 47 cases have any window or page test, but only 30 run through the application.
- **With my tests:** 22 full, 10 partial, 18 page only, 9 model only or missing, 2 none. The row stays partial.
- **proofs14 is out of date here:** `plugins.updateAll` is now registered (`plugins_window.rs:165`), so its pending test is real.

**Part 1, rows that would become done:**

- **`point.registerOverlay:plugins.consent` and `ipc:plugin_grant`:** one new test seeds a plugin with an old 0.1 manifest, so it asks for a permission when it runs. The permission page shows; Cancel grants nothing; OK grants it on the host. The native path (`plugins_window.rs:746`) and the adapter (`commands/plugin.rs:222`) both call `PluginHost::grant`.
- **`ipc:plugin_list`:** already proven by the existing two-window tests in `project_plugins_per_window.rs`. The recorded "no test opens two windows" is stale.

**Rows that stay partial:** `pluginLog` (staying open across tab switches is now proven; Find in Logs is missing natively), the `plugins` menu group (the editor tab menu is now proven; no test for the Problems menu yet), the `search.bookmarks` overlay (the chooser-placement gap is stale; the preview is still plain text), the `search.bookmarks` menu group, `agents.send` (my test of it is model level only), `run`, `search.bookmarks.files` and `ipc:plugin_run_command`. For the last one, nothing in the checked-in fixtures can show whether the command chain is passed on.

**Part 2:** the `PreviewEditor` view does exist, in a reduced form. It is read-only, makes a new editor for each item, and has no TODO colouring. My pending test asks for the webview's behaviour. For the services debug pane, `selectableText`, deployment `SyncPreview` and actions, and remote-dev `ports`, the views are missing. I wrote no tests for those; the notes give file:line evidence.

**Hunks needed:** the declared tests need none. The pending Run badge test needs a new accessor, `Workbench::stripe_badge(&self, id: &str) -> Option<usize>`.

**Left out:**

- **Plugin endpoints (case 61):** nothing calls `PluginHost::endpoints`.
- **Icon-theme resolver and theme preview:** neither has any caller.
- **Problems menu test:** the test setup can't combine a language server with a plugin.
- **Command-chain test:** no fixture calls back into its caller.
- **Project view multi-select:** that view is being edited right now.

**Differences from the webview:**

- Add Bookmark is greyed out without a project (the webview's looks enabled).
- The TODO preview is read-only, rebuilt per item and uncoloured.
- The Run stripe button has no running-count badge.
- Plugin log tabs have no Find.
- Nothing previews plugin themes, draws plugin file icons or asks plugins for endpoints.
- A plugin command may get only the editor's main selection. This is read from code and not confirmed.

**Most likely to fail first:**

- **The permission test:** it assumes the plugins folder is `<home>/data/plugins` and that the window's label reaches the plugin host as the client.
- **Three tests that hand the app's own host to `use_plugin_host`:** no existing test does this; reading the code, it should be safe.
- **The editor-tab menu test:** after the menu closes, the editor must have the keyboard back.
- **The api14 test:** `plugins.settings` must accept the argument api14 passes.
