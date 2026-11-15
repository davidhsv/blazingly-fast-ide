# Integrating the Git actions on the editor's text and the external Git log

New files, not compiled yet:

- `app-gpui/src/vcs_more/caret_conflicts.rs` — model: the conflict block at a UTF-16 caret and where Next / Previous Conflict Marker go (wraps `vcs::conflicts`, which works in bytes).
- `app-gpui/src/vcs/selection_history_view.rs` — the History for Selection tab view and its pure helpers (`selected_lines`, `block_of`, `history_rows`).
- `app-gpui/src/vcs/external_log_view.rs` — the editor tab of an external Git log (commit list, read-only).
- `app-gpui/src/workbench/vcs_text_actions.rs` — the nine actions, the two tab builders, the backend calls and the end-to-end tests.

Every anchor below is quoted from the file as it is now; there are no line numbers because `mod.rs` is moving. All edits are insertions. Run `cargo fmt` after pasting: several lines in the new files run past 120 columns.

## 1. `app-gpui/src/vcs/mod.rs`

After `pub mod conflicts_dialog;` add:

```rust
pub mod external_log_view;
```

After `pub mod revision_chooser;` add:

```rust
pub mod selection_history_view;
```

## 2. `app-gpui/src/vcs_more/mod.rs`

Before `pub mod diff_blame;` add:

```rust
pub mod caret_conflicts;
```

## 3. `app-gpui/src/workbench/mod.rs`

### 3a. The module

After `mod vcs_operations;` add:

```rust
mod vcs_text_actions;
```

### 3b. The actions

In `pub fn register_actions(cx: &mut App)`, after `vcs_operations::register_actions(registry);` add:

```rust
    vcs_text_actions::register_actions(registry);
```

No `Workbench` field, no context key and no layer: the state lives in the two tab views.

## 4. `app-gpui/src/workbench/tab_views.rs`: the two tab kinds

In `pub const TAB_KINDS: &[TabKind] = &[`, after the Local History entry, i.e. after

```rust
        build: super::diff_tools::build_local_history_tab,
    },
```

add:

```rust
    TabKind {
        id: crate::vcs::selection_history_view::SELECTION_HISTORY_KIND,
        restorable: false,
        active_key: None,
        focus_key: None,
        title: crate::vcs::selection_history_view::selection_history_title,
        icon: crate::vcs::selection_history_view::selection_history_icon,
        build: super::vcs_text_actions::build_selection_history_tab,
    },
    TabKind {
        id: crate::vcs::external_log_view::GIT_LOG_KIND,
        restorable: false,
        active_key: None,
        focus_key: None,
        title: crate::vcs::external_log_view::external_log_title,
        icon: crate::vcs::external_log_view::external_log_icon,
        build: super::vcs_text_actions::build_external_log_tab,
    },
```

Without this hunk `open_view_tab` opens a tab that nothing builds, and `build_*_tab` are dead code (`-D warnings`).

## 5. Menu rows

- None to add to `menu_spec.rs`: Git › Current File already has `act("vcs.showHistoryForSelection", "Show History for Selection")`, and the editor's Git submenu (`context_menus.rs` `git_menu`) has `action("vcs.showHistoryForSelection", …)`. The other eight actions are in no webview menu: they are reached by keymap (Next / Previous Change only, `assets/gen/keymaps.json`), Find Action, the hunk popup and Vim's `rc` map.
- The Git log toolbar's Open Another Log Tab menu ("External Repository…", `GitLog.tsx:1014`) is not wired: `LogFilterEvent::ShowNewLogTab` has no handler in the workbench (`grep -rn ShowNewLogTab app-gpui/src` finds only its declaration and emit). Once someone handles it, its "External Repository…" row is `MenuItem::action("vcs.log.openExternal")`.

## 6. Table

| Action id                     | Webview source                                                                                    | Native handler                                                                                 | End-to-end test (in `vcs_text_actions.rs`)                                                                                                                                                                                                                                                                      | IPC commands                          |
| ----------------------------- | ------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------- |
| `vcs.acceptYours`             | `src/features/vcs/actions.tsx:605` (`resolveAtCaret("ours")`, `conflictsUi.tsx:173`)              | `Workbench::accept_at_caret(Resolution::Ours)`                                                 | `accept_yours_theirs_and_both_at_the_caret_replace_that_conflicts_block_and_leave_the_others`, `with_the_caret_outside_a_conflict_accept_changes_nothing_and_says_so`, `accept_yours_during_a_rebase_takes_the_commit_being_replayed`, `the_conflict_actions_are_enabled_only_while_an_editor_has_the_keyboard` | `doc_edit` (the editor's edit)        |
| `vcs.acceptTheirs`            | `actions.tsx:612`                                                                                 | `accept_at_caret(Resolution::Theirs)`                                                          | the first, third and fourth above                                                                                                                                                                                                                                                                               | `doc_edit`                            |
| `vcs.acceptBoth`              | `actions.tsx:619`                                                                                 | `accept_at_caret(Resolution::Both)`                                                            | the first and fourth above                                                                                                                                                                                                                                                                                      | `doc_edit`                            |
| `vcs.nextConflict`            | `actions.tsx:626` (`gotoConflict(1)`, `conflictsUi.tsx:193`)                                      | `Workbench::go_to_conflict(Direction::Next)`                                                   | `next_and_previous_conflict_marker_move_the_caret_and_wrap`, `the_conflict_actions_are_enabled_only_while_an_editor_has_the_keyboard`                                                                                                                                                                           | none                                  |
| `vcs.prevConflict`            | `actions.tsx:633`                                                                                 | `go_to_conflict(Direction::Previous)`                                                          | same two                                                                                                                                                                                                                                                                                                        | none                                  |
| `vcs.nextChange`              | `actions.tsx:430` (`focusedDiffNavigator().next()`, else `gotoChange(view, 1)`, `gutter.ts:268`)  | `diff.nextDifference` while `diffEditorFocus`, else `Workbench::go_to_change(Direction::Next)` | `next_and_previous_change_move_through_the_editors_changed_ranges_and_wrap`, `next_change_moves_between_the_differences_of_the_focused_diff_viewer`, `the_change_actions_are_enabled_only_in_a_project_under_version_control`, `show_history_for_selection_is_enabled_only_with_a_text_editor_active`           | `vcs_line_hunks`                      |
| `vcs.prevChange`              | `actions.tsx:443`                                                                                 | `diff.prevDifference` while `diffEditorFocus`, else `go_to_change(Direction::Previous)`        | the first and third above                                                                                                                                                                                                                                                                                       | `vcs_line_hunks`                      |
| `vcs.showHistoryForSelection` | `actions.tsx:540` (`showSelectionHistory`, `selectionHistoryKind.ts`; tab `SelectionHistory.tsx`) | `Workbench::show_history_for_selection` → tab `selectionHistory` → `load_selection_history`    | `show_history_for_selection_lists_the_commits_that_changed_the_selected_lines`, `show_history_for_selection_is_enabled_only_with_a_text_editor_active`, `the_change_actions_are_enabled_only_in_a_project_under_version_control`                                                                                | `vcs_block_history`, `cancel_request` |
| `vcs.log.openExternal`        | `src/features/vcs/historyActions.tsx:248` (`openExternalLog`, `RevisionBrowser.tsx:39`)           | `Workbench::open_external_log` → `open_external_logs` → tab `git-log` → `load_external_log`    | `open_external_git_log_shows_the_log_of_the_picked_repository`                                                                                                                                                                                                                                                  | `vcs_open_external_log`, `vcs_log`    |

`when` clauses (from `assets/gen/actions.json`, through `registry.handle`): the five conflict actions `editorTextFocus`; Next / Previous Change `vcsAvailable`; Show History for Selection `vcsAvailable && textEditorActive`; `vcs.log.openExternal` has none, so it has no ON/OFF test. Titles, icons (`arrow-down`, `arrow-up`, `history`) and shortcuts (Next / Previous Change only) come from the manifest and the generated keymaps.

## 7. Behaviour, and what is not wired

- **Accept at caret**: the edit replaces exactly the block (from `<<<<<<<` to past the `>>>>>>>` line); every selection is mapped through it with `Assoc::Before`, as Rollback Lines maps them, so a caret inside the block lands at its start. The rebase swap reads `vcs_state().branch.operation`, as `conflicts.rs` does. Outside a block: info notice "The caret is not inside a conflict" (group `vcs`). A read-only editor refuses the edit; that is logged, not told (the webview's dispatch tells nothing either).
- **Next / Previous Conflict Marker**: `from > caret` / `to < caret`, wrapping; no blocks: nothing happens and nothing is told. The editor then takes the keyboard.
- **Next / Previous Change**: the ranges are read with `vcs_line_hunks` for each move, because nothing in the native editor keeps the gutter's hunks (`HunkState` is only built by Rollback Lines). The editor's version is checked after the read, as `start_rollback` does. A file outside version control logs at debug level and keeps its caret. The caret is centred (`scroll_caret_to_center`), as the webview's `scrollIntoView(y: "center")`.
- **Show History for Selection**: a virtual document or no editor gives "Open a file to show the history of its selection". The tab is keyed `selectionHistory:<path>:<from>-<to>` and titled `History: <name>:<first>-<last>`. Its request is cancelled when the view is released before the answer (a detached `observe_release`). Asking again for the same file and lines brings that tab to the front; when the text changed meanwhile, the open view's request is cancelled and it reloads with the new text (`SelectionHistoryView::reload`). No end-to-end test runs this re-ask.
- **Open External Git Log…**: a prompt for folders (several allowed). Each one goes through `vcs_open_external_log`; the first failure raises the error "Could not open Git log" with the message as body and opens no tab. Cancelling does nothing. The tab is keyed by the sorted roots, titled with them joined by ", ", icon `toolwindow-git`.
- **Not built (partial)**: the external log tab is a commit list (summary, author, time, short id, the repository's folder name when several), at most 300 commits per repository, no paging, no graph, no filters, no details pane, no context menu. The native Git log view (`vcs/log_view.rs::GitLog`) always reads the project's log (`vcs_log(.., None, ..)`), and the log-tab models (`vcs_ops/log_store.rs` `open_log_tab`, `vcs_more/log_loading.rs` `PageLoad::external_logs`) have no view. The webview's tab is restorable; this one is not, because the backend forgets external logs between runs. The `git-log` tab kind is claimed here for external logs only: an input without `repositories` makes `build_external_log_tab` return `None`, which closes the tab. Whoever wires the In Editor log tabs (Show History, a branch's log) must share this kind and its builder.
- **Not built**: the selection history's diff uses the plain `DiffView`, with no syntax colours (the Local History tab gets them from `load_history_diff`).

## 8. Assumptions (check first)

0. **Losing the keyboard**: the OFF side of `editorTextFocus` gives the keyboard to the Project view (`toolwindow.project`), as `context_keys_follow_the_project_the_editor_the_tool_windows_and_the_repository` does.
1. **The diff window takes `diffEditorFocus`** once its dialog's focus handle has the keyboard; the test focuses it explicitly (`dialog.read(cx).focus_handle(cx)`, as `publish_diff_context` reads it). `DiffDialog: Focusable` is assumed from that call.
2. **Child modules see `pub(super)` items of siblings**: `branches::{This, call, finished, host_call, notify}`, `Workbench::open_view_tab`, `Workbench::active_tab_view`, `super::tests::{context_is_on, start, notice_titled, …}`, `plugins_window::test_support::trusted_project`. All are declared `pub(super)` or `pub(crate)`.
3. **`git rebase main` stops with exit status ≠ 0** and leaves `HEAD` detached with `branch.operation == "rebase"`; the backend reports it through `VcsState.branch`.
4. **`git merge feature` keeps three blocks** in `c.txt`: the changed lines are five unchanged lines apart.
5. **`vcs_block_history` lists the commit that added the lines** ("add b") after "Shout two"; the test only asserts the first three rows.
6. **`vcs_open_external_log` on a plain temporary folder fails** (`Repo::discover` finds no repository above the system temp folder). If the machine's temp folder is inside a Git worktree, the second half of the external-log test opens a log instead.
7. **`IdeError` is `ide_core::IdeError`** and `IdeResult<T>` is `Result<T, IdeError>` (used for the `Ok::<_, IdeError>` in `load_external_log`).

## 9. Compile risks

- `registry.handle(id, move |call, window, cx| …)` reads `call.ctx.is_truthy(..)`: `ActionCall.ctx` is `&ContextKeys`, `is_truthy` is `pub` in `platform/context.rs`.
- Nested `execute_action("diff.nextDifference", …)` from inside an action handler, outside `on_workbench`: the registry counts nesting; the workbench is not borrowed at that point.
- `move_to_change` takes `&mut App` and is called with `&mut Context<Workbench>` (deref coercion).
- `cx.observe_release(&view, |_, view: &mut SelectionHistoryView, cx| …).detach()` in a `Context<Workbench>`.
- `return tracing::debug!(…)` in a match arm of a closure returning `()`.
- The `if let (Some(_), Some(head)) = (…) && head != current` let-chain in `history_rows` (Rust 2024).

## 10. Spec files

- `src/features/vcs/__tests__/selectionHistory.test.tsx`: 6 of 6 cases. The five `selected lines` rows run in one table test, `selected_lines_of_a_text`. The tab cases are view tests in `selection_history_view.rs`; the cancel on close is the window's (`pending_request` + `observe_release`), not unit-tested.
- `src/features/vcs/__tests__/conflicts.test.ts`: already covered by `vcs/conflicts.rs` tests, so nothing is duplicated. `caret_conflicts.rs` adds the caret cases: UTF-16 with non-ASCII text, outside a block, wrapping, no blocks, the rebase swap.
- `src/features/vcs/__tests__/hunks.test.ts` › hunk navigation: already covered by `vcs_diff/hunks.rs` and `vcs_diff/gutter.rs` (`goto_change`).
