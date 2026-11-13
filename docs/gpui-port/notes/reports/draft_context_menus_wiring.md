The context menus are drafted in one new module plus an integration notes file, but nothing has been compiled or run. The module only builds after the `plugin_contributions.rs` draft is compiled and declared, and every hunk in sections 1 to 6 of the notes is applied. Several pasted lines run past 120 columns, so run `cargo fmt` after pasting.

**The brief was partly wrong** (I found this by reading the code, not by running it):

- The editor tab already has a context menu (`Workbench::tab_menu`, `TabCommand`, `run_tab_command`), opened by right click and tested by `a_tabs_menu_closes_pins_and_splits`. Problems also already has its menu. I added only what each was missing.
- A native group model already exists in `run_producers/context_run.rs` (`ContextMenu`, `MenuGroups`, ordered by order then id). I reused it.
- `assets/gen/contributions.json` has each group's id, menus, order and source, but no rows. So the rows are Rust functions in `MENU_GROUPS`, and the source line of each group is in the notes table.
- The source has no keyboard opening for the editor menu; Shift+F10 is Run in the Windows keymaps. I bound nothing.
- The menu bar and the context menus treat an unregistered action differently. The bar shows it as enabled; the context menus leave it out, as the webview's `Menu.tsx` does. I followed `Menu.tsx` and left the bar unchanged.

## 1. Files

- `app-gpui/src/workbench/context_menus.rs` (1747 lines). It has no view of its own: the menus are shown with `show_menu`, and the slot buttons are elements built by `slot_buttons(slot, window, cx)`.
  - **Row builders:** `editor_menu`, `gutter_menu`, `tool_window_options`, `stripe_button_menu`, `open_in_menu`, `local_history_menu`, `git_menu`.
  - **Commands:** `EditorMenuCommand`, `GutterCommand`, `ToolWindowCommand`.
  - **`impl Workbench`:** `open_editor_context_menu`, `editor_menu_items`, `tab_file_rows`, `with_problem_groups`, `show_search_result_menu`, `project_view_contributed`, `show_tool_window_options`, `show_stripe_button_menu`.
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/context_menus_integration.md` (541 lines). It holds the hunks for `workbench/mod.rs`, `code_insight.rs`, `project_files.rs`, `find/view.rs` (a new `FindPanelEvent::ContextMenu`), `title_bar.rs` and `vcs/commit_box.rs`, plus the change to the existing tab test.
  - Its Project view hunk replaces section 6.2 of the plugin notes.

**One hunk is required to compile (1i).** It splits `show_menu` into `show_menu` plus `show_popup_menu`. The editor menu's Column Selection Mode row reads the workbench to show its check mark (`command_actions.rs` → `active_editor`). Building that menu inside the workbench's update would panic, so `show_menu_later` builds it in `window.defer`.

## 2. Actions

- **Registered:** none. All rows run existing actions; Cut, Copy and Paste run `edit.cut`, `edit.copy` and `edit.paste`. The only backend call is the settings write (IPC `settings_set`) for the gutter's Appearance switches and Show Tool Window Names.
- **Built (13 of 25 groups):** `agents.send`, `build.module`, `debugger.evaluate`, `http.new`, `plugins`, `scratch.new`, `search.bookmarks`, `search.bookmarks.files`, `search.structure.selectIn`, `templates.emmet`, `templates.newFromTemplate`, `vcs.gutter`, `web.open`. The `plugins` group is built with `plugin_menu_items::<Infallible>(PluginMenu::…, paths, position, &window_context(window, cx), cx)`.
- **Gaps (12 groups):** `debugger.hotswap`, `debugger.stop`, `deployment`, `github.reviewMode`, `go.modules`, `gradle.analyzeDependencies`, `http.spring`, `maven.analyzeDependencies`, `run.context` (its actions are not registered), `settings.fileType`, `templates.new`, `web.openIn`. Each needs feature state the window does not have yet.
- **Hidden rows:** rows whose action is not registered do not appear. The test `the_editor_menu_leaves_out_the_actions_the_window_does_not_register` prints the editor menu's list. A list I found by grep only is in the notes; it includes `navigate.selectIn`, `project.openInTerminal`, `code.generate`, `vcs.annotate`, `emmet.updateTag` and `web.openInBrowser`.

## 3. Spec files

- `ContributedToolbar.test.tsx`: the one case is partly covered (the button is drawn and runs); hiding by `when` is not tested.
- `controller.test.ts` "builds IDEA's editor tab menu": covered by the existing tab test.
- `controller.test.ts` "closes read-only tabs…": not covered end to end. I don't know how to make a read-only document in a test.
- `virtual.test.ts` (groups on tabs without a path): not built.
- There are no specs for the editor menu, the gutter menu, the menu groups or Move To.

**Tests:** 4 unit tests and 9 end-to-end tests. The exact order of the editor menu is asserted only by the unit test `the_editor_menu_lists_its_rows_in_order`. The end-to-end tests compare the shown menu with the builder's output filtered by the registry, plus fixed checks: Paste before Go To, Cut/Copy/Paste together, and Git › Commit File… greyed out without a repository and enabled in one.

The end-to-end tests cover:

- A right click outside the selection moves the caret; inside it, the selection stays. Escape closes the menu and the editor gets the keyboard back.
- Copy puts the selected text on the clipboard.
- Go To › Declaration with the fake server moves the caret.
- The gutter menu adds a bookmark on the clicked line.
- The tab menu acts on the clicked tab, not the active one.
- The Hello plugin's submenu appears, its Sync entry is greyed out, Insert Date runs, and its `editor.title` button is drawn.
- Move To in a tool window's Options menu moves the window.

## 4. Not built, assumptions, likely failures

- **Not built:**
  - Open in New Window on the tab menu.
  - The Find results' own rows (Exclude, Replace Selected); only the groups are shown, and no menu opens when they are empty.
  - Gutter Soft-Wrap, because the native editor has no soft wrap.
  - Tool windows' own header items before Move To.
  - Check marks on rows of a menu's own, which are drawn with a check icon instead.
- **Cut/Copy/Paste hints:** the keymaps leave `edit.copy` and friends unbound, so the menu shows the OS keys (⌘C and so on), as the menu bar's Edit menu does.
- **Settings write:** it blocks on the main thread (`platform::settings::set`), which is how the crate already writes settings.
- **Assumptions to check first:**
  - The `Infallible` row mapping and the boxed group closure compile.
  - The settings serialize with camelCase keys; `editor.showGutterIcons` and `editor.showWhitespace` may not exist and would then read as off.
  - Unknown text passes through translation unchanged, so "Open 'index.html' in Browser" shows as built.
  - A right click in a Markdown preview also opens the editor menu.
- **Likely to fail first:**
  - The tab and Options tests, unless sections 1e and 1f are applied.
  - The plugin test, which needs the plugin draft compiled and the Hello fixture's wasm.
  - The declaration test, which needs `python3` for the fake server.
  - The gutter test, which assumes a gutter at least 12px wide and that `file.settings` is registered.
