I wrote the five files and the integration notes; none of it has been compiled or run. 26 of the 23 + 4 action ids are registered (22 of 22 assigned minus 4 left out, plus nothing extra); 4 are left out, each with a reason below. The notes also add the Project tool window's Options menu, which the brief did not ask for.

## 1. Files written

| File                                                        | Lines | Contents                                                                                                                                                                                                                                        |
| ----------------------------------------------------------- | ----- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/project_view/tree_clicks.rs`                  | 186   | `row_click(options, node, count, plain) -> RowClick {Select, Toggle, Open{preview, focus}}`; `selection_open(options, node) -> Option<OpenRequest>`; `OpenRequest {path, preview, focus}` with `impl EventEmitter<OpenRequest> for ProjectTree` |
| `app-gpui/src/project_view/pane.rs`                         | 184   | `TreePane`: `stored`, `mode`, `set_mode`, `set_open_files`, `set_changed_files`, `model`, `auto_expand`, `keeps_stored_expansion`                                                                                                               |
| `app-gpui/src/theme/zoom.rs`                                | 69    | `apply_zoom(&mut Theme, percent)`, `zoom_factor`; it scales every single-pixel-value token                                                                                                                                                      |
| `app-gpui/src/workbench/project_view_options.rs`            | 801   | `TreeOptionsSlot`, the actions, `header_menu_rows`, 9 end-to-end tests                                                                                                                                                                          |
| `app-gpui/src/workbench/zoom_actions.rs`                    | 248   | the zoom actions, Quick Switch (`SchemeCommand`), 2 end-to-end tests                                                                                                                                                                            |
| `.../scratchpad/drafts/project_view_options_integration.md` | 469   | every hunk, anchored on quoted text                                                                                                                                                                                                             |

No new views: the behaviour goes into `ProjectTree` through hunks (notes §3a–3k). Those hunks add the `pane` field and five methods: `pane()`, `set_open_files`, `set_changed_files`, `click_row`, `selection_changed`. Further hunks:

- **Zoom:** `theme/appearance.rs::apply`.
- **Workbench:** `mod.rs` (field, register calls, tree subscription, `tabs_changed`), `branches.rs::vcs_state_changed`, `context_menus.rs::show_tool_window_options`.

Several lines run past 120 columns, so run `cargo fmt` after pasting.

## 2. Actions registered

| Action                                                                  | What it does                                                                                                                  | Test that runs it                                                                     |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| `project.option.previewTab`, `project.option.singleClickOpen` (toggles) | A single click opens the file (preview tab / regular tab) and the keyboard stays in the tree                                  | `opens_files_on_double_click_and_previews_on_single_click_only_when_enabled`          |
| `project.option.singleClickExpand`                                      | A single click opens or closes a folder                                                                                       | `expands_and_collapses_folders_on_single_click_only_when_enabled` (includes a reopen) |
| `project.option.autoscrollFromSource`                                   | The tree selects the active tab's file                                                                                        | `follows_tab_switches_with_always_select_opened_file`                                 |
| `project.option.autoscrollToSource`                                     | The arrow keys open the file they move to                                                                                     | `autoscroll_to_source_opens_the_file_the_selection_moves_to`                          |
| All five behaviour options                                              | Checked rows of the Options menu; on and off both survive reopening the project                                               | `the_options_menu_lists_the_view_options_and_they_survive_reopening_the_project`      |
| `project.viewMode.{project,projectFiles,openFiles,changedFiles}`        | Pane switch via `ProjectViewState::set_view_mode`; stored under `openidea.projectTree.mode`; Project view shown without focus | `switches_panes_and_remembers_the_choice` (git temp repo, reopen)                     |
| `project.findUsages`                                                    | `ProjectCommand::FindUsages` → IPC `doc_open`, `doc_outline`, `lsp_references`, `doc_close`                                   | `find_usages_of_file_searches_the_selected_file`                                      |
| `project.reformatCode` / `project.optimizeImports`                      | Hand off to `code.reformatFile` / `code.optimizeImportsFile`                                                                  | `reformat_and_optimize_imports_in_the_tree_change_the_selected_file` (fake LSP)       |
| `project.reloadFromDisk`                                                | `ProjectCommand::Reload` → `fs_read_dir` (forced), then `file.reloadFromDisk`                                                 | `reload_from_disk_reads_the_selected_file_again`                                      |
| `view.zoomIn`, `view.zoomOut`, `view.resetZoom`                         | `settings_set("global", "appearance.zoom")`; asserts the `fs-ui` text size and the tree row height                            | `the_zoom_actions_step_the_ide_zoom_and_the_window_draws_at_it`                       |
| `view.quickSwitchScheme`                                                | Writes the theme / `editor.colorScheme` / `keymap` settings                                                                   | `quick_switch_scheme_switches_the_theme_and_the_editor_color_scheme`                  |

Not registered:

- **`project.option.showMembers`:** `ProjectTree` draws chevrons only for folders, `rows()` never adds member nodes, and nothing in the tree loads outlines.
- **`project.renameInline`:** the tree has no inline editor.
- **`project.viewMode.scratches`:** nothing outside `project_view/` registers a scratches root.
- **`project.viewMode.unity`:** this is unbuilt wiring, not a missing feature. The backend detects Unity, but `ProjectTree` is never given the Unity info.

## 3. Spec coverage

- **`ProjectView.test.tsx`:**
  - Covered: "opens files on double click…" (model + end-to-end), "expands and collapses folders on single click…" (model + end-to-end), "toggles folders once for each rapid double-click pair" (model).
  - Partly covered: "follows tab switches… hiding Select Opened File". The native header has no Locate button to hide.
  - Not covered: the three Show Members cases.
- **`viewModes.test.ts`:** "switches panes and remembers the choice" and "lists open and changed files, expanded once" are covered end-to-end. The scratches and library parts are not.
- **`store.test.ts`:** the single-click persistence case is covered.

## 4. Not built, assumptions, likely failures

Not built:

- The "Project ▾" pane switcher: the header title is plain text.
- Quick Switch's 500 ms theme preview on hover: `PopupMenu` has no active-row event.
- Zoom scales only the theme's lengths. Sizes written as literal pixels in views do not change, and GPUI has no window-wide scale.
- A preview or no-focus open always uses a text tab, so an image does not open in the image viewer that way.

Hypotheses for the integrator to check first:

- **Hypothesis:** the child module can reach the private `tabs_changed`, `load_tab_probed` and `Content` fields of `workbench`.
- **Hypothesis:** closing and reopening the project in the same window does not race `close_project_detached`.
- **Hypothesis:** in Light mode a `.txt` file's usage search ends as "Cannot find usages of 'notes.txt'".
- **Hypothesis:** the app calls `theme::appearance::init` at start-up.
- **Hypothesis:** `keymap.rs` pairing `file.reloadFromDisk` with `project.reloadFromDisk` routes ⌥⌘Y in the tree to `project.reloadFromDisk`.
- **Hypothesis:** the Options menu selects its first enabled row when it opens.

Tests most likely to fail on the first run:

- **Reformat / optimize imports:** the fake server has to start for a file that is not open yet.
- **Panes test:** depends on the git status read reaching the tree, and on the reopen.
- **Single-click expand after reopen:** needs the expansion written when the old tree is released.
- **Zoom row height:** compared with a ±0.5 px tolerance.
