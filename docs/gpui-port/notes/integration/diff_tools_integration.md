# Integrating the compare tools and Local History into the workbench

New files, not compiled yet:

- `app-gpui/src/vcs/local_history_view.rs`: the Local History tab view, plus its models (`snapshots_before`, `revision_range`, `recent_changes`, `recent_change_label`).
- `app-gpui/src/vcs/revision_chooser.rs`: the branch/tag/revision chooser view, plus `revision_items`.
- `app-gpui/src/workbench/diff_tools.rs`: the actions, the handlers, the backend calls and the end-to-end tests.

Every anchor below is quoted from the file as it is now. Line numbers are left out because `mod.rs` is moving. Sections 1 to 5 are all required:

- `apply_diff_edit` is used only from the hunk in 4c.
- `load_text` / `open_texts` are imported from `diff_window`.
- `build_local_history_tab` is used only from the hunk in 3.

Without them the build fails, or `-D warnings` fails on dead code.

## 1. `app-gpui/src/vcs/mod.rs`

Add in alphabetical order: `local_history_view` after `pub mod interactive_rebase_dialog;`, and `revision_chooser` after `pub mod prompt_dialogs;`:

```rust
pub mod local_history_view;
pub mod revision_chooser;
```

## 2. `app-gpui/src/workbench/mod.rs`

### 2a. The module

After `mod diff_window;` add:

```rust
mod diff_tools;
```

### 2b. The actions

In `pub fn register_actions(cx: &mut App)`, after `diff_window::register_actions(registry);` add:

```rust
    diff_tools::register_actions(registry);
```

No new `Workbench` field is needed:

- Each Local History view lives in its tab's `TabView`.
- The revision chooser lives in the existing vcs dialog layer (`open_vcs_dialog`).
- Every backend task is detached and lands on a `WeakEntity`.

## 3. `app-gpui/src/workbench/tab_views.rs`: the Local History tab kind

In `pub const TAB_KINDS: &[TabKind] = &[`, after the entry whose `build:` is `super::plugin_runtime::build_plugin_log_tab,` (and its closing `},`), add:

```rust
    TabKind {
        id: crate::vcs::local_history_view::LOCAL_HISTORY_KIND,
        restorable: false,
        active_key: None,
        focus_key: None,
        title: crate::vcs::local_history_view::local_history_title,
        icon: crate::vcs::local_history_view::local_history_icon,
        build: super::diff_tools::build_local_history_tab,
    },
```

## 4. `app-gpui/src/workbench/diff_window.rs`

### 4a. Reuse the side loader

Replace

```rust
async fn load_text(
```

with

```rust
pub(super) async fn load_text(
```

### 4b. Reuse the open editors' texts

Replace

```rust
    fn open_texts(&self, cx: &App) -> HashMap<String, String> {
```

with

```rust
    pub(super) fn open_texts(&self, cx: &App) -> HashMap<String, String> {
```

### 4c. Accept Left / Right Side writes into the file

In `fn diff_dialog_event`, replace

```rust
                // Both panes are read-only here, and have no menu of their own.
                DiffViewEvent::Apply { .. } | DiffViewEvent::ShowMenu { .. } => {}
```

with

```rust
                // An arrow, a hover Revert or Accept Left / Right Side on an editable side.
                DiffViewEvent::Apply { target, change } => self.apply_diff_edit(*target, change, cx),
                // The panes have no menu of their own.
                DiffViewEvent::ShowMenu { .. } => {}
```

Only diffs opened by the compare tools (`Workbench::open_compared` in `diff_tools.rs`) call `DiffView::set_editable`. Every other diff (Show Diff, the log, the shelf, the merge editor's Compare) keeps `ArrowSides { to_a: false, to_b: false }`. Its viewer never emits `Apply`, and its toolbar and badges are unchanged.

## 5. Tests in compiled files that change with these registrations

### 5a. `app-gpui/src/workbench/project_view_actions.rs`, test `compare_with_shows_two_files_side_by_side_and_asks_for_the_second_of_one`

This file is another agent's. Coordinate before pasting.

With `diff.compareWith` registered, `compare_with_chosen_file` delegates to it through a deferred top-level `execute_action_from`, so the action listener sees it too. `diff.compareWith` asks the backend for the path's kind (`ops::fs::entry_kind`) before it prompts, so the prompt is not there right after `run_until_parked`.

Replace

```rust
        assert_eq!(actions_run_by(&primary("d"), cx), ["project.compareWith"]);
        assert!(cx.did_prompt_for_paths(), "the file to compare with is asked for");
```

with

```rust
        // One file goes to the diff feature's Compare With…, which looks at the path off the window's
        // thread before it asks for the other file.
        assert_eq!(actions_run_by(&primary("d"), cx), ["project.compareWith", "diff.compareWith"]);
        wait_long(cx, "the file to compare with is asked for", |cx| cx.did_prompt_for_paths());
```

The OS-picker fallback in `compare_with_chosen_file` stays: it runs when the action is not registered. The test's later `!actions_run_by(...).contains(&"project.compareWith")` part is unchanged.

### 5b. `app-gpui/src/workbench/context_menus.rs` `mod tests`: the literal row lists

Rows of newly registered actions now show:

- Local History › Show History / Put Label…
- Compare With…
- Compare with Clipboard
- Git › Compare with Branch… / Show History

`vcs.annotate` and `vcs.showHistoryForSelection` stay unregistered.

Before:

```rust
    /// The root of the editor's menu in a project, without a selection, with the actions this window
    /// registers: Refactor, Open In, Local History and the Compare rows have none of their actions
    /// registered, so they are not offered.
    const ROOT: [&str; 10] = [
        "Show Context Actions",
        "Paste",
        "Copy / Paste Special",
        "Column Selection Mode",
        "Go To",
        "Folding",
        "Rename…",
        "Generate…",
        "Send to Agent",
        "Git",
    ];
```

After:

```rust
    /// The root of the editor's menu in a project, without a selection, with the actions this window
    /// registers: Refactor and Open In have none of their actions registered, so they are not offered.
    const ROOT: [&str; 13] = [
        "Show Context Actions",
        "Paste",
        "Copy / Paste Special",
        "Column Selection Mode",
        "Go To",
        "Folding",
        "Rename…",
        "Generate…",
        "Send to Agent",
        "Local History",
        "Git",
        "Compare With…",
        "Compare with Clipboard",
    ];
```

Before:

```rust
    const GIT: [&str; 11] =
        ["Commit File…", "Add", "-", "Show Diff", "Rollback…", "-", "Push…", "Pull…", "Fetch", "-", "Branches…"];
```

After:

```rust
    const GIT: [&str; 13] = [
        "Commit File…",
        "Add",
        "-",
        "Show Diff",
        "Compare with Branch…",
        "Show History",
        "Rollback…",
        "-",
        "Push…",
        "Pull…",
        "Fetch",
        "-",
        "Branches…",
    ];
```

Assumption: "Open In" really is absent today, as the current `ROOT` says. If the first run shows it, keep it where `editor_menu` puts it (before "Local History").

`ROOT` is asserted in both the no-repository test and the Git test (`root_rows(&menu, cx)`):

- In the no-repository project, Show History and Compare with Branch… have `when: vcsAvailable`. They are offered greyed out, so the root still lists "Git".
- If `root_rows` drops disabled rows, ROOT differs between the two tests. Check the first failure output.

### 5c. Other menus to grep after the first run (no literal assertion was found)

These pick up rows automatically:

- the tab menu (`mod.rs` `tab_menu`: "Compare With…", "Compare with Clipboard");
- `shelf.rs` ("vcs.localHistory");
- `vcs_ops/commit_view.rs` ("vcs.localHistory");
- the VCS Operations popup (`vcs_operations.rs` `OPERATIONS`: Show History, Compare with Branch…, Local History);
- the "Version Control ▾" fallback (`title_bar.rs` `VCS_FALLBACK_ACTIONS`: now also "Show Local History").

`grep -rn '"Show Git Log"' app-gpui/src/workbench/mod.rs` shows the fallback test uses `contains`, so it is unaffected.

## 6. Action id → end-to-end test → IPC → expected `parity.json` status

| Action id                    | End-to-end test (`diff_tools.rs`)                                                                            | IPC command → backend function                                                                                                                              | Status                             |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------- |
| `diff.compareFiles`          | `compare_files_shows_both_files_and_accept_left_side_writes_the_change_into_the_right_one`                   | the diff's loads: `fs_read_text` → `ops::fs::read_text`, `fs_probe` → `ops::bytes::fs_probe`                                                                | done                               |
| `diff.applyLeftSide`         | same test (action, key ⌃⌘→ refused in the editor)                                                            | none in the webview (live document edit); here `Documents::replace_all` + `save` (`ops::doc::blocking`)                                                     | partial                            |
| `diff.applyRightSide`        | same test (key ⌃⌘← runs it in the diff window)                                                               | as above                                                                                                                                                    | partial                            |
| `diff.compareWith`           | `compare_with_asks_for_the_other_file_and_shows_both` (+ `project_view_actions` test of 5a)                  | `fs_entry_kind` → `ops::fs::entry_kind`; then the diff's loads                                                                                              | partial                            |
| `diff.compareWithClipboard`  | `compare_with_clipboard_compares_the_file_or_the_selection_and_compare_texts_its_argument`                   | diff loads only (clipboard is GPUI's)                                                                                                                       | done                               |
| `diff.compareTexts`          | same test                                                                                                    | none                                                                                                                                                        | done                               |
| `vcs.localHistory`           | `show_history_diffs_a_saved_revision_and_revert_writes_it_back_once_confirmed`, `put_label_names_…` (folder) | `history_list_dir` → `ops::history::history_list_dir`; `history_content` → `ops::doc::history_content`; `history_restore` → `ops::history::history_restore` | done                               |
| `project.showHistory`        | `put_label_names_…` (tree selection → the file's tab)                                                        | delegates to `vcs.localHistory`, as above                                                                                                                   | done                               |
| `localHistory.putLabel`      | `put_label_names_the_projects_state_and_shows_in_the_histories_and_recent_changes`                           | `history_put_label` → `ops::history::history_put_label`                                                                                                     | done                               |
| `localHistory.recentChanges` | same test, and `the_recent_changes_key_runs_only_with_a_project_open` (⌥⇧C)                                  | `history_list_dir`                                                                                                                                          | partial (popup has no time column) |
| `vcs.showHistory`            | `show_history_for_file_filters_the_git_log_to_the_file`                                                      | `vcs_log` (with `path`) → `ops::vcs::vcs_log`                                                                                                               | partial                            |
| `vcs.compareWithBranch`      | `compare_with_branch_diffs_the_file_against_the_branch_chosen`                                               | `vcs_compare_scope` → `ops::vcs::vcs_compare_scope`; `vcs_branches`, `vcs_tags`; `vcs_file_at`                                                              | partial                            |

Measured (`grep -rn` in `app-gpui/src`), against the claim that these rows have no native caller:

- `history_content` already had one: `diff_window.rs` `load_text`.
- `history_list_dir` and `history_restore` already had one: `project_files.rs` (undo of a delete).
- `history_put_label` had none before this file.

`history_list` (`ops::doc::history_list`) is still not called. The webview's Local History uses `history_list_dir` for files too.

Not registered, with why:

- `diff.compareDirectories`: the backend `fs_compare_directories` → `ops::fs::compare_directories` exists, but there is no native directory-comparison view and no model for one. The webview's is `DirectoryCompare.tsx`, 292 lines. `diff.compareWith` on a folder says "Compare Directories is not available" instead.
- `vcs.annotate` (a toggle, it needs `handle_toggle`): no gutter-column seam in `editor/view.rs` (`grep -n "annotation\|blame" app-gpui/src/editor/*.rs` → nothing). The `view.rs` hunks were not drafted. It needs a separate stage:
  - a gutter column painted beside the line numbers;
  - fed from `ops::vcs::vcs_blame`;
  - its menu;
  - and `GroupFacts.annotations_shown` in `context_menus.rs`.
- `vcs.showHistoryForSelection`: needs the Selection History view (`SelectionHistory.tsx`, `git log -L`). Not built.
- `vcs.log.openExternal`: needs `RevisionBrowser` (external log). Not built.

## 7. Assumptions (check these first)

1. **`ide_host::ops::doc::blocking`** is public and re-exported (`ops/doc.rs` `pub use commands::*`). `Documents::{by_path, text_of, open, replace_all, save, close}` keep their signatures (`crates/ide-workspace/src/documents.rs`).
2. **`DiffView::set_editable` persists across `set_texts`.** It is a plain field set once after `open_diff`. A chain step (`DiffDialog::show`) would keep it, but compare diffs have no chain.
3. **`TextChange` offsets match the text written to.** The change is applied with `Doc::new(text).apply(&change)` to the document's text (or the file's). The viewer computed it on the text it shows, which is identical for LF files. A CRLF file is a risk.
4. **An open editor follows `replace_all`** through `DocPatched` (as in the conflicts notes). An Apply into a file open in an editor leaves it unsaved, like the webview's live document. A file nobody has open is saved and closed. The webview keeps that document attached and unsaved; this is a deliberate difference.
5. **`history_restore` overwrites and saves an open, unsaved document** with one confirmation and no second prompt, as the webview does (`LocalHistory.tsx` `restore` → `confirmDialog` → `historyRestore`). The test asserts the unsaved editor's text before and after.
6. **Each save through the app records one "Saved" snapshot, and the watcher adds no "External change" for it.** The test takes the second "Saved" entry. If extra entries show up, select by content instead.
7. **Times use `DateFormatOptions::default()`.** The settings' date format is not read here. `vcs_diff::model::format_time` had no caller before.
8. **`root_rows` / menu rows.** `MenuItem::entry("Recent Changes", None)` is a row, so `menu-row-0-{i}` matches the index in `rows()`.
9. **`with_layout(… layout.show(GIT_TOOL_WINDOW, true))` mounts the Git log synchronously** (`mount_panels`), so `git_log_view()` is `Some` right after it.
10. **`project.showHistory` may also be claimed** by the agent editing `project_view_actions.rs`. Register it once.

11. **The window's Git gate.** No Git command is issued here, only reads. `vcs_compare_scope`, `vcs_branches`, `vcs_tags`, `vcs_file_at` and `vcs_log` run without `git_command_starts`, as `branches_of` and the log do. Revert from Local History is not a Git operation: it calls `history_restore` directly (not `run_op`, which needs a repository) and then `refresh_vcs`.
12. **Autosave.** The Local History test turns `files.autoSave` off first. Otherwise the tab switch saves the unsaved editor (`Workbench::autosave` on the previous tab), and there would be no unsaved editor to revert over.

## 8. Compile risks

- **Inherent method names are unique per type across the crate.** `diff_tools.rs` adds these methods to `Workbench`:
  - `compare_root`, `compare_active_path`, `compare_selection`, `notify_compare`, `run_compare_outcome`, `open_compared_diff`;
  - `compare_with_chosen`, `pick_file_to_compare`, `compare_against_clipboard`, `accept_diff_side`, `apply_diff_edit`;
  - `open_local_history`, `local_history_view`, `local_history_event`, `load_local_history`, `load_history_diff`;
  - `show_recent_changes`, `show_recent_changes_menu`, `show_git_file_history`, `compare_file_with_branch`, `show_revision_chooser`.

  None existed when grepped (`grep -rn "fn <name>(" app-gpui/src`). Other agents' drafts may add the same names: grep again after merging.

- `revision_items` keeps a `BTreeMap` plus an insertion `order` vec (inside `or_insert_with`). If the borrow checker objects, push to `order` outside the closure.
- `LocalHistoryView::set_history` mutably borrows `cx` (Localization) while it builds `self.loaded`. If it conflicts, compute the entries before the `match`.
- `the_recent_changes_key_runs_only_with_a_project_open` opens two windows from one `TestAppContext`. `empty_cx`'s last use comes before `trusted_project(cx, …)`. If the borrow checker still objects, move the empty-window half into its own test.
- `PromptRequest` fields: `title, label, initial, ok_label, mono, validate_initial, checkbox` (`vcs_ops/dialogs.rs`).
- `branches::{call, confirm, i18n, notify, run_action_later, finished, This}` are `pub(super)` in `branches.rs`, so they are reachable from the sibling module.
- `Workbench::{editors, label}` are private to `workbench` and reachable from its children.
- Long `match` arms may need `cargo fmt`. The integrator runs fmt.

## 9. Tests most likely to fail first

1. **`compare_files_…` on the key half.** `DiffView` claims keys on its focus (`input::claim_keys`). If it takes ⌃⌘← for its own caret moves, `actions_run_by(apply_key(false))` is empty. Check `DiffView::key`.
2. **`show_history_diffs_…` on the entry count / order.** See assumption 6. Then "the editor follows" may wait on the clean flag after the backend's save.
3. **`put_label_…` on clicking the Recent Changes row** (assumption 8), and on whether the chosen row's path equals the canonical `text_of(&file)`.
4. **`compare_with_branch_…`.** `simulate_input` goes to the chooser's field only if `open_vcs_dialog` focused it (it is given `chooser.focus_handle`, which is the field's).

## 10. What the webview does that is not built

- The Local History tab is not restored with the session (`restorable: false`). The webview's editor kinds are restorable.
- The Recent Changes popup shows no time column (`MenuItem` has no detail text).
- Show History for File filters the main log instead of opening a history tab of its own, so the main log's path filter changes. It takes the argument or the active file. The Commit window's selection is ignored.
- Compare with Branch on a folder or on several paths: the webview opens the path comparison tree (`showPathComparison`). Here it is "Comparing folders with a branch is not available". That text is invented.
- Accept Left / Right Side works only in diffs opened by the compare tools. The webview also allows it on Show Diff's working copy. ⌘D on two files in the tree (`project.compareWith` calls `open_diff` itself) opens a read-only diff. To make it editable, route it to `diff.compareFiles`.
- After an Apply into a file open in an editor, the diff reloads from `open_texts` (the editors' text). It may read before the editor has taken the patch. The test covers files nobody has open.
- The diff inside the Local History tab is a bare `DiffView`, with none of these:
  - the diff toolbar;
  - `diffEditorFocus`, so F7 / Accept do not reach it;
  - handling of its events (Refresh, Jump to Source, settings changes are dropped).
- Annotate: the exact `editor/view.rs` hunks the assignment asked for were not drafted. This is not delivered, not ruled out.
- Not built at all: Compare Directories, Annotate, Show History for Selection, Open External Git Log.
