I wrote one native test per spec flow for all 15 specs: 42 tests are ready to be declared, and 16 flows sit in undeclared `_pending.rs` files. Nothing was compiled or run (no cargo by design), and no existing file was edited.

Once they pass, 7 rows are done and 8 stay partial:

| Row                 | Declared / spec flows                              | State                                                                                      |
| ------------------- | -------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| commit              | 1 / 1                                              | done                                                                                       |
| gitIgnore           | 2 / 2                                              | done                                                                                       |
| keymap              | 2 / 2                                              | done                                                                                       |
| openProject         | 1 / 1                                              | done (also proves double Shift opening Search Everywhere)                                  |
| pluginNotifications | 1 / 1                                              | done                                                                                       |
| newBranch           | 4 / 4                                              | done                                                                                       |
| compareWith         | 1 new + 3 already covered by existing window tests | done (row's proof must cite those 3)                                                       |
| plugin-settings     | 1 / 1                                              | partial: the spec's failing-action alert isn't driven, the real fixture's command succeeds |
| codeInsight         | 4 / 7                                              | partial                                                                                    |
| toolwindows         | 7 / 12                                             | partial                                                                                    |
| mergeEditor         | 5 new + 9 existing / 16                            | partial                                                                                    |
| run                 | 2 / 4                                              | partial                                                                                    |
| search              | 3 / 4                                              | partial                                                                                    |
| editor              | 6 / 8                                              | partial                                                                                    |
| diff-navigation     | 2 / 3                                              | partial                                                                                    |

**Files**

- Declared, with line counts: in `app-gpui/src/workbench/tests/`: `proofs18_e2e_commit.rs` (58), `proofs18_e2e_git_ignore.rs` (58), `proofs18_e2e_keymap.rs` (111), `proofs18_e2e_open_project.rs` (82), `proofs18_e2e_new_branch.rs` (120), `proofs18_e2e_toolwindows.rs` (177), `proofs18_e2e_merge_editor.rs` (151), `proofs18_e2e_run.rs` (81), `proofs18_e2e_search.rs` (76), `proofs18_e2e_editor.rs` (249), `proofs18_e2e_diff_navigation.rs` (69), plus the shared helpers `proofs18_e2e_fixtures.rs` (106). Elsewhere: `app-gpui/src/workbench/plugin_runtime/tests/proofs18_e2e_plugin_notifications.rs` (54), `app-gpui/src/workbench/plugin_contributions/tests/proofs18_e2e_plugin_settings.rs` (128), `app-gpui/src/editor/intelligence/tests/proofs18_e2e_code_insight.rs` (167), `app-gpui/src/workbench/diff_tools/tests/proofs18_e2e_compare_with.rs` (40), `app-gpui/src/workbench/diff_tab/tests/proofs18_e2e_diff_navigation.rs` (43).
- Pending, not to be declared: `*_pending.rs` next to toolwindows, merge_editor, run, search, editor and diff_navigation in `app-gpui/src/workbench/tests/`, and `app-gpui/src/editor/intelligence/tests/proofs18_e2e_code_insight_pending.rs`.
- Module names are snake_case (`proofs18_e2e_git_ignore`, not `gitIgnore`). camelCase would fail `non_snake_case` under `-D warnings`.

**Hunk the declared tests need** (only one): in `app-gpui/src/settings_view/dialog.rs`, `SettingsDialogView` gets `pub fn page_view(&self, page: &str) -> Option<&AnyView> { self.page_views.get(page) }`. The plugin-settings test depends on it. The pending tests assume further accessors and debug selectors, each listed in its file's header.

**Steps missing natively** (why those flows are pending):

- **codeInsight:** ⇧F6 opens the Rename dialog instead of renaming in place, which also blocks the rename-preview flow. No fixture plugin provides a lazy code action.
- **toolwindows:** double-clicking a tool window's title doesn't toggle maximize. Terminal tabs and their close buttons have no debug selectors.
- **editor:** there are no code breadcrumbs, and `navigate.navigationBar` isn't registered. An edited tab draws no modified mark.
- **Not checked, recorded as hypotheses** with the command that would settle each:
  - Go Rerun Failed Tests has no caller in the UI.
  - CMake before-launch may already work, so its test may be promotable as written.
  - Find in Files' preview is not editable.
  - Merge divider controls can't take the keyboard.
  - A window can't hold two merge editors at once.
  - Diff tabs skip binary files when walking with F7.
- **Not reproduced:** window resizing (`setViewportSize`) in the maximized-Terminal flow.
- **Corrections:** two of my own early claims turned out wrong. Before-launch tasks do run in the run driver. Diffs open in maximizable editor tabs under the non-modal commit setting. The notes call both out as corrections.

**Behaviours that may differ from the webview** (written to the spec, so they may fail):

- The second ⌘⇧F12 may not bring the tool windows back.
- A new find bar may not start from the last search.
- A stopped run may not report exit code 143.
- The Conflicts dialog may not open the merge editor for a modify/delete conflict.

The full first-run failure list, per-flow mappings, fixture differences and the exact `mod` lines with their anchors are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs18_e2e.md`.
