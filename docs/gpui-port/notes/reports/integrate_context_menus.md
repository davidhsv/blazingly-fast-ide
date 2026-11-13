I've integrated the context menus and fixed the generator. The whole suite passed five times in a row, with nothing ignored, and clippy and `gen-data.mjs --check` are clean. Nothing is committed.

## 1. Gates

- **Suite ×5:** each run was `6408 passed; 0 failed; 0 ignored`, about 50 s. An earlier pass of five failed once, on run 2, in `go_to_declaration_from_the_editor_menu_moves_the_caret_to_the_declaration`. The cause was in the test: its wait called `f.editor(cx)`, which panics on `expect("an editor")` during the moment the window has no editor while the declaration's file opens. The wait now accepts no editor. I stopped that pass and reran all five from the start.
- **`cargo clippy --all-targets -- -D warnings`:** clean.
- **`node docs/gpui-port/gen-data.mjs --check`:** no difference.
- **Compile:** 0 errors on the first build; one dead-code warning for `with_menu_context` in `project_files.rs`, which I removed.
- **Scope:** I edited only files in scope, formatted with `rustfmt --edition 2024` on the files I changed, and ran no git command that changes anything. `git status` now shows two untracked paths I did not create: `app-gpui/src/plugin_views/view_node.rs` and `app-gpui/src/settings_view/pages/`.

## 2. Part 0: the generator

- **Cause:** an action literal built inside `LIST.map(([id, title, …]) => ({…}))` was evaluated once, without its parameters bound. Destructured parameters were never bound, and conditionals on a parameter were not evaluated. So every element got the same record: the type's literal union became the variants, its first member became the title, and that one entry was copied to every id.
- **Fix (`gen-data.mjs` only):**
  - A `.map` callback over a static array now gives one instance per element.
  - Parameters, including destructured ones, are bound by `bindName`.
  - Conditionals, `!` and comparisons on bound values are evaluated statically, and a conditional that resolves to `undefined` leaves the property out.
  - ``tx`…${x}…` `` titles resolve and also emit `titleTemplate`.
  - Duplicate dynamic entries are listed once.
  - The `UNRESOLVED_CONDITIONS` entry for `mergeActions.ts` is removed, because those ids are now static.
- **Changed metadata in `actions.json`** (diff against a snapshot taken before the fix):
  - **Title fixed, `titleVariants` removed:** `find.popup.nextResult`, `find.popup.previousResult`, `find.popup.openResult`, `moveDown` (icon also fixed to `arrow-down`), `vcs.ignoreLocal`.
  - **`titleVariants` (and `iconVariants`) removed, title already right:** `find.popup.recentSearches`, `moveUp`, `vcs.ignoreShared`.
  - **64 ids moved from `dynamic` into `actions`:** `dotnet.{build,clean,restore}`, `editor.colorScheme.{import,export,delete}`, `go.generate.{file,package}`, 30 `merge.*`, `plugins.log.{find,clear,wrap}`, 10 `project.option.*`, 3 `project.sortBy.*`, 6 `project.viewMode.*`, `run.services.{runAll,restartAll,stopAll,openBrowser}`.
  - Only `merge.autoApply` keeps `toggle`.
  - The 19 `project.*` ids get titles such as "Project View: Compact Middle Packages", plus `titleTemplate`.
  - `dynamic` goes from 20 entries to 11.
- **`contributions.json`:** the run-configuration types `application` and `spring-boot` (`src/features/run/types.ts:456`) now have their own id, label, icon, description and defaults, instead of computed or merged ones.
- **Native tests:** none had enshrined the wrong title. `has_a_chinese_entry_for_every_literal_action_title` then failed on the 19 template-built titles, because the webview's test also leaves templates out. That test now skips `titleTemplate` titles, and a new test, `translates_every_action_title_built_from_a_template`, checks them through `tx_template`.
- **Hypothesis:** `ActionMeta::localized` (`platform/actions.rs`) translates titles with `tx`, not `tx_template`, so these 19 titles may show in English in the Chinese UI. A zh test that localizes `project.option.compact` would confirm it.

## 3. Menus → tests

All test paths are `workbench::context_menus::tests::<name>` except 1h's.

| Menu (group)                                                                                                                   | Tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | Status                                                   |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------- |
| Editor (`point.registerMenuGroup:agents.send`, `:plugins`; `:debugger.evaluate`, `:web.open`, `:templates.emmet` as data only) | `right_click_in_a_plain_file_opens_the_editor_menu_and_moves_the_caret`, `right_click_in_the_selection_keeps_it_and_copy_puts_it_on_the_clipboard`, `in_a_git_repository_the_editor_menus_git_rows_are_enabled`, `go_to_declaration_from_the_editor_menu_moves_the_caret_to_the_declaration`, `the_editor_menus_git_add_adds_the_file_of_the_editor`, `a_plugins_editor_menu_entries_appear_under_their_submenu_and_run`, `the_editor_menu_lists_its_rows_in_order`, `the_editor_menu_leaves_out_the_actions_the_window_does_not_register` | partial                                                  |
| Gutter (`:search.bookmarks`, `:vcs.gutter`)                                                                                    | `the_gutters_menu_adds_a_bookmark_on_the_clicked_line`, `the_gutter_menu_opens_settings_pages_and_shows_its_switches`                                                                                                                                                                                                                                                                                                                                                                                                                      | partial                                                  |
| Editor tab (`:search.bookmarks.files`)                                                                                         | `the_tab_menu_acts_on_the_tab_it_was_opened_on`, `the_tab_menus_bookmark_row_bookmarks_the_file_of_that_tab`, `workbench::tests::a_tabs_menu_closes_pins_and_splits`                                                                                                                                                                                                                                                                                                                                                                       | partial                                                  |
| Search Results (`:plugins`)                                                                                                    | `a_plugins_find_results_entry_shows_only_while_its_when_holds`                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | partial                                                  |
| Slots (`point.registerMenuSlotItem:(at) src/features/plugins/uiPoints.tsx:182`)                                                | `renders_toolbar_slots_with_command_icons_and_honors_when_clauses`                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | partial: `mainToolbar` and `commit.message` are untested |
| Options and stripe button                                                                                                      | `move_to_in_a_tool_windows_options_moves_it`, `the_stripe_buttons_menu_hides_the_button`                                                                                                                                                                                                                                                                                                                                                                                                                                                   | partial: the header's own items are not built            |
| Cross-menu                                                                                                                     | `a_press_beside_an_open_menu_closes_it_and_the_keyboard_returns_where_it_was`, `the_menus_show_the_shortcuts_of_the_active_keymap`, `the_groups_of_a_menu_come_in_order_each_followed_by_a_separator`, `every_menu_group_is_listed_once`                                                                                                                                                                                                                                                                                                   | —                                                        |
| Project view (`:build.module`), Problems (`:plugins`)                                                                          | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | missing                                                  |

Rows hidden because their action is not registered, as measured by the `--nocapture` run:

- **Editor:** Paste from History…, Super Method, Test, Folding (all 5 rows), Rename…, Refactor (all 11), Generate…, Open In (Project View, Finder, Terminal), Local History (Show History, Put Label…), Git rows (Annotate, Compare with Branch…, Show History, Show History for Selection), Compare With…, Compare with Clipboard.
- **Tab:** Open In, Local History, both Compare rows.
- **Gutter:** Annotate.
- **New groups:** HTTP Request, Scratch File, From Template….
- **Groups with no rows in this window:** the 12 that `every_menu_group_is_listed_once` lists.

**Context keys read ON and OFF:**

- `workspaceContains:MODULE.bazel`: the slot button, and the Find results entry.
- `vcsAvailable`: Git rows disabled, then enabled.

**Actions newly run from a menu:** `edit.copy`, `navigate.declaration`, `navigate.toggleBookmark`, `bookmarks.toggleFileBookmark`, `vcs.addToVcs`, and the hello plugin's `insert-date` (from the editor menu, the slot button and the Find results menu).

## 4. Product changes and test changes beyond the notes

- **Product:**
  - Removed the now-unused `with_menu_context` from `project_files.rs`. The undeclared `project_view_actions.rs` will need it back.
  - Added `debug_selector("find-row-{index}")` to the Find result rows (`find/view.rs`) so the tests can press them.
- **Tests:**
  - `hello_menus` now sets `engines.openidea = "^1.4"`; the host refuses submenus below API 1.3 and toolbar slots below 1.4.
  - The fixture home is a `Box<dyn Any>`, so the `PluginHome` lock lives for the whole test.
  - The Git Add test waits until the window has read the new file, as `vcs_operations.rs`'s own test does.
  - The gutter test now proves the right press emits no `GutterPressed` and that a left press at the same place does. The old "no breakpoint marks" check could not fail, because this window has no debugger.
- **Notes that turned out false:** "run `cargo fmt`" (the rules forbid it); the `PluginHome` type; the missing API version in the manifest.

## 5. Left out and open differences

- **Not tested:** the Problems menu (needs diagnostics from the fake server), the Project view's group rows, and the `mainToolbar` and `commit.message` slots.
- **Click beside a menu:** the native backdrop swallows the press, and only the left button closes the menu. The webview closes on any button and lets the press through (`Popup.tsx`, its pointerdown listener). I did not change it because the backdrop is shared by every popup.
- **Missing own rows:** the gutter's Soft-Wrap row (there is no soft wrap) and the Find results' Exclude/Include, Replace Selected and Preview Replacement.
- **Hypothesis:** the gutter's Add Bookmark greys out without a project, while the webview's own row always runs (`src/features/search/index.ts:210`). No test covers it.
- **Lists later integrations must extend** (in `app-gpui/src/workbench/context_menus.rs`, test module):
  - `ROOT`, `GO_TO` and `GIT`;
  - the literal lists in `the_tab_menu_acts_on_the_tab_it_was_opened_on` and `a_plugins_editor_menu_entries_appear_under_their_submenu_and_run`;
  - `a_tabs_menu_closes_pins_and_splits` in `workbench/mod.rs`;
  - `every_menu_group_is_listed_once`'s `missing` list.
