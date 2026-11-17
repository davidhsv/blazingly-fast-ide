# F-SEARCH-B draft report: Search Everywhere scope and history, edited files, layout tests

I wrote three of the pieces in the brief, and all of them are left as new untracked files. None of the code has been compiled or run, because the brief forbids cargo. I edited no existing file and ran no git command that changes anything. Several pieces I marked `build` were not written in this pass; they are listed below rather than relabelled.

## Files written

| File                                                                                                                                                                                      | Lines |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----- |
| `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/everywhere/view/memory.rs`                                                   | 186   |
| `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/everywhere/view/parts.rs`                                                    | 232   |
| `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/everywhere/view/proofs41_stale.rs`                                           | 59    |
| `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/tests/proofs41_e2e_search_everywhere.rs`                           | 204   |
| `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/tests/proofs41_e2e_search_layout.rs`                               | 141   |
| `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/tests/proofs41_recent_edited.rs`                                   | 37    |
| `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/search_b_integration.md` (notes, sections a to g) | 802   |

The notes hold 26 hunks, each anchored on text copied from `git show HEAD:`:

- **`everywhere/view.rs`:** V1 to V20.
- **`everywhere/recent_view.rs`:** R1, which makes `marked_text` `pub(crate)`.
- **`workbench/mod.rs`:** M1 to M4.
- **`workbench/editor_core.rs`:** E1 to E3.

## What was built

1. **Search Everywhere (piece 1):**
   - **Scope:** asking for the shown tab's action again toggles "Include non-project items", which is "Include disabled actions" on the Actions tab. The header has that check box.
   - **Widening:** when the project has nothing for a query, the popup searches everywhere and says "Nothing found". A new query that no longer contains the old one narrows back to the project.
   - **History:** ⌥↑/⌥↓ walk the tab's earlier queries and give back the draft. A tab opened without a selection starts with its last query, and closing the popup remembers the query.
   - **Recent actions:** actions run from the popup are remembered and listed under Recent Actions.
   - **⌥↵ on an action:** runs `settings.assignShortcut` with the action id.
   - **⌘↵ on Text:** opens `find.inPath` with the query.
   - **Rows and frame:** a hint in the query field, a footer describing the selected row, shortcuts beside actions, matched characters marked, a 16 px icon slot on every row. The popup is 760 px wide, less in a narrow window, and the header wraps at 680 px or below.
2. **Edited files (piece 2):** Recent Files › Show edited only now lists the files edited in the window. Undo and redo count; changes made by the backend do not.
3. **Tests (piece 3):** a late answer of an old query is not shown (the view-level test the hard rule asks for), plus the end-to-end tests and the layout tests for e2e cases 1, 4 and 5.

## Totals of the list

| Verdict                       | Clauses                                 |
| ----------------------------- | --------------------------------------- |
| Already there                 | 2                                       |
| Built                         | 13                                      |
| Build, not built in this pass | 21                                      |
| Needs something else first    | 8                                       |
| Not native                    | 1 (the 125% browser zoom in e2e case 4) |

The eight "needs":

- opening in the other pane, which two clauses need (`TabCommand::OpenInOpposite => false`)
- a tab's close result, to keep a row whose tab refused to close
- language server symbols and endpoints as sources
- virtual documents for library classes
- the Structure window following a preview
- multi-selection in the Project view
- a native Python hierarchy over `nav_inheritance`

The largest pieces still marked build and not written:

- **Multi-selection in Search Everywhere:** 9 of the dialogs cases wait on it; the model is in `find/row_selection.rs`.
- **The preview pane:** `ui/code_preview.rs` exists, but the splitter with a stored ratio does not.
- **The rest:** the funnel and file-set scope, Recent Locations (both the place tracker and the popup), the Recent Searches menu, the refresh when indexing finishes, and the File Structure, Bookmarks and Select In › Structure items.

## Rows

| Row                                                                  | State after the hunks                                                                                              |
| -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `search.everywhere` overlay                                          | partial, with fewer gaps                                                                                           |
| Key claim at `SearchEverywhere.tsx:86`                               | ⌥↵ (on actions), ⌘↵ and ⌥↑/⌥↓ all act now. Hypothesis: it stays partial until ⌥↵ opens its menu on the other rows. |
| The five Search Everywhere actions                                   | partial; their own repeat toggle is done                                                                           |
| `recentFiles`, `switcher`, `search.recent`                           | partial; edited files done, Recent Locations and the other-pane open missing                                       |
| `recentLocations`                                                    | not started (it has no `parity.json` row)                                                                          |
| File Structure, Structure and Bookmarks tool windows, bookmark menus | unchanged                                                                                                          |
| `dialogs.test.tsx`                                                   | 9 of 23 cases in full (5 before), 2 in part                                                                        |
| `fileStructure.test.tsx`                                             | 4 of 5 (Vue case missing)                                                                                          |
| `inherited.test.ts`                                                  | 2 of 3 (Python case missing)                                                                                       |
| `searchKeys.test.ts`                                                 | done                                                                                                               |
| `e2e/searchEverywhereLayout.spec.ts`                                 | 5 of 9 runs (cases 2 and 3 need the preview)                                                                       |

## Hypotheses

- `Context::defer` resolves to `App::defer`, and `("name", index)` works as an `ElementId`. Both follow existing uses in the crate.
- The match indices of symbols, actions and settings are byte offsets, like those of files.
- The Recent Files paths and `editor.path()` are the same strings.
- A workbench never switches projects in place. If it does, the edited set must be cleared there.
- The tests run on macOS, where `cmd-enter` is the primary modifier.
- The file index finds `FindAction.kt` for "Find Action", with its space.
- `set_locale(Some("zh"), "en")` redraws the popup in Chinese.
- The popup's field is still alive in `on_release`.
- The Vue outline is not nested by the backend.
- `find_action_says_what_to_do_when_it_has_nothing_to_list` is the only existing test that changes; disabled actions are now hidden on Actions until the box is ticked.

The notes give a command to settle each one. Two I settled by reading the code: `set_text` does not notify the owner, and every `ActionMeta` field is `pub`.

## Existing tests that change

- **`workbench::tests::find_action_says_what_to_do_when_it_has_nothing_to_list`:** it now sees "Nothing found" after widening, where it expected "Nothing found in project files" (`SearchEverywhere.tsx:441-452`, `:496`).
- **Tests that reopen a tab:** the field now starts with the last query, selected, so typing still replaces it.

## Most likely to fail first

- `tabs_and_query_fit_narrow_windows`: the widths of the tabs and their wrapping are estimates.
- `mixed_file_and_action_rows_align_icons_and_names_and_keep_shortcuts_readable`: the space in the query.
- `searches_everywhere_when_nothing_is_found_in_the_project`: the deferred search after widening.

## What turned out wrong

- **Your brief points at readers' notes that are not there:** `RETRIAGE_UNIT_1.md` has no section for `dialogs.test.tsx` or `searchKeys.test.ts`. The table for `src/features/search/index.ts` in `RETRIAGE_ACTIONS.md` is empty (16 rows, all moved), and `RETRIAGE_REST.md` has no search overlay or tool window rows.
- **`parity.json` gaps:** it has no rows for `navigate.recentLocations`, `search.structure.selectIn` or the e2e spec, and it counts `dialogs.test.tsx` as 22 cases where HEAD has 23. The extra case is the `:541` table, "spells its shortcuts the %s way", which no native test covers.
- **A stale clause in the parity proof:** "the empty-result text" is listed as missing, but it exists at `everywhere/view.rs:633-650`.
- **The workplan's dependency for previews is too strong:** it says previews wait on I-UNDO-PER-DOC, but the editor preview in `ui/code_preview.rs` already serves Bookmarks and Find in Files. Hypothesis: the Search Everywhere preview only needs a splitter.
- **An unused store:** `SearchEverywhereState` in `everywhere/store.rs` was complete and tested but no native code used it. The view passed no recent actions and kept Include disabled actions always on in Actions.
