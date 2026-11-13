# Integrating the Git log's actions into the workbench

New files, not compiled yet:

- `app-gpui/src/workbench/log_actions.rs`: the wiring and the end-to-end tests.
- `app-gpui/src/vcs/log_reset_dialog.rs`: Git Reset.
- `app-gpui/src/vcs/log_message_dialog.rs`: Edit Commit Message and Squash Commits.
- `app-gpui/src/vcs/interactive_rebase_dialog.rs`: Interactively Rebase from Here.
- `app-gpui/src/vcs/log_tag_dialog.rs`: New Tag on a commit.

Every anchor below is quoted from the files as they are now; line numbers are left out because `mod.rs` moves. Sections 1 to 6 and 8a are required for `log_actions.rs` to compile without dead code (`show_commit_menu` is called only from 8a and the tests). Section 7 (Push up to a commit) is required for the test `push_all_up_to_here_pushes_the_branch_to_the_bare_remote` to pass; without it the action pushes the whole branch. Run `cargo fmt` after pasting.

## 1. `app-gpui/src/vcs/mod.rs`

Add these lines in alphabetical order: `interactive_rebase_dialog` after `pub mod graph;`, and the three `log_*` ones after `pub mod log_options;`, keeping `log_view` after them.

```rust
pub mod interactive_rebase_dialog;
pub mod log_message_dialog;
pub mod log_reset_dialog;
pub mod log_tag_dialog;
```

## 2. `app-gpui/src/workbench/mod.rs`

### 2a. The module

After `mod log_window;` add:

```rust
mod log_actions;
```

### 2b. The field

In `pub struct Workbench`, after

```rust
    vcs_dialog: Option<branches::VcsDialog>,
```

add:

```rust
    /// What the Git log's actions keep for their notifications' buttons (Undo of a history rewrite).
    log_actions: log_actions::LogActionsSlot,
```

### 2c. Its initial value

In `Workbench::new`, in `let mut workbench = Self { … }`, after `vcs_dialog: None,` add:

```rust
            log_actions: Default::default(),
```

### 2d. The actions

In `pub fn register_actions(cx: &mut App)`, after `log_window::register_actions(registry);` add:

```rust
    log_actions::register_actions(registry);
```

All 38 ids are in `assets/gen/actions.json` (checked with `grep -c '"vcs.log.<id>"'`, 1 each), so `registry.handle` does not panic.

### 2e. The Undo button of a history rewrite

In `fn run_notification_action`, add this arm before `EntryAction::Own(own) if own.command.starts_with(conflicts::FOLLOW_UP_PREFIX) => {`:

```rust
            EntryAction::Own(own) if own.command.starts_with(log_actions::UNDO_PREFIX) => {
                if !self.run_log_undo(&own.command, window, cx) {
                    tracing::debug!("the notification command {} has no handler any more", own.command);
                }
            }
```

Without this arm, the generic `EntryAction::Own(own)` arm swallows the button. The test `edit_commit_message_rewords_the_head_and_an_older_commit_and_undo_restores_it` would then fail.

## 3. `app-gpui/src/workbench/branches.rs`: visibility for the sibling module

- In `pub(super) struct VcsSlot`:
  - `    state: VcsState,` becomes `    pub(super) state: VcsState,`
  - `    console: VcsConsole,` becomes `    pub(super) console: VcsConsole,`
- `enum Success {` becomes `pub(super) enum Success {`.
- `async fn checkout(` becomes `pub(super) async fn checkout(`.
- In `impl Workbench`, `fn vcs_state_changed(&mut self, cx: &mut Context<Self>) {` becomes `pub(super) fn vcs_state_changed(&mut self, cx: &mut Context<Self>) {`.
- In `pub(super) fn drop_vcs(&mut self)`, after `self.vcs_dialog = None;` add the line below. The Undo buttons belong to the project that goes.

```rust
        self.log_actions = Default::default();
```

## 4. `app-gpui/src/workbench/conflicts.rs`

`async fn save_exact(host: &Host, label: &str, path: PathBuf, text: String) -> IdeResult<()> {` becomes `pub(super) async fn save_exact(…`. Get from Revision writes through it, so an open editor of the file follows.

## 5. `app-gpui/src/workbench/log_window.rs`

### 5a. Options from the actions

`fn set_log_options(&mut self, options: LogOptions, cx: &mut Context<Self>) {` becomes `pub(super) fn set_log_options(…`.

### 5b. Revert and Get from Revision of the changes menu and toolbar

In `pub(super) fn run_log_changes_command`, replace

```rust
            ChangesCommand::Revert | ChangesCommand::GetFromRevision | ChangesCommand::ShowHistory { .. } => {
                tracing::info!("{command:?} on the log's changes is not available");
            }
```

with

```rust
            ChangesCommand::Revert => self.revert_log_changes(changes, window, cx),
            ChangesCommand::GetFromRevision => self.get_from_revision(changes, window, cx),
            ChangesCommand::ShowHistory { .. } => {
                tracing::info!("{command:?} on the log's changes is not available");
            }
```

## 6. `app-gpui/src/workbench/commit.rs`: Commit and Rebase after Fixup… / Squash Into…

Replace

```rust
    if request.rebase_onto.is_some() {
        tracing::info!("Commit and Rebase is not available: the fixup commit was made without the rebase");
    }
```

with

```rust
    if let Some(target) = request.rebase_onto.clone() {
        super::log_actions::rebase_after_fixup(this.clone(), cx, target).await?;
    }
```

`rebase_after_fixup` is `pub(super)` in `log_actions.rs`. It is otherwise unused, so without this hunk `-D warnings` fails on dead code. No end-to-end test covers it, because it is not an action. It reuses `range_from` and `run_rebase`, and both are tested.

## 7. Push All up to Here: the Push dialog pushes up to a commit

The source passes `{ branch, revision }` to `vcs.push` (`historyActions.tsx`). The source's Push dialog pushes `revision` explicitly (`remoteUi.tsx`, `const revision = pushTarget.revision?.id ?? null`). `PushChoice.revision` and `push_operation` already handle it (`vcs_more/push_target.rs`); only the dialog and the action drop it today.

### 7a. `app-gpui/src/vcs/prompt_dialogs.rs`

1. In `pub struct PushDialogInput`, after `pub protected_branches: String,` add:

```rust
    /// Push All up to Here…: the commit the selected repository's branch is pushed up to.
    pub revision: Option<String>,
```

2. In `pub struct PushDialog`, after `    protected: String,` add:

```rust
    /// The repository pushed only up to a commit, and that commit.
    revision: Option<(String, String)>,
```

3. In `PushDialog::new`, in the returned `Self { … }`, after `protected: input.protected_branches,` add:

```rust
            revision: input.selected.zip(input.revision),
```

`input.repos` is moved earlier, which is a partial move. `selected` and `revision` are still whole, and `selected` was only borrowed before.

4. In `fn choice`, replace `            revision: None,` with:

```rust
            revision: self.revision.as_ref().filter(|(root, _)| *root == row.repo.push.root).map(|(_, id)| id.clone()),
```

Test constructors use `..PushDialogInput::default()`, so they compile unchanged.

### 7b. `app-gpui/src/workbench/branches.rs`

1. Replace the `vcs.push` registration

```rust
    registry.handle("vcs.push", |call, window, cx| {
        let branch = call.arg.and_then(|arg| arg.get("branch")).and_then(Value::as_str).map(str::to_owned);
        on_workbench(window, cx, |workbench, window, cx| workbench.start_push(branch, window, cx))
    });
```

with

```rust
    registry.handle("vcs.push", |call, window, cx| {
        let branch = call.arg.and_then(|arg| arg.get("branch")).and_then(Value::as_str).map(str::to_owned);
        // A commit id, or a commit object with one.
        let revision = call.arg.and_then(|arg| arg.get("revision")).and_then(|revision| {
            revision.as_str().or_else(|| revision.get("id").and_then(Value::as_str)).map(str::to_owned)
        });
        on_workbench(window, cx, |workbench, window, cx| workbench.start_push(branch, revision, window, cx))
    });
```

2. `fn start_push(&mut self, branch: Option<String>, window: &mut Window, cx: &mut Context<Self>) {` becomes

```rust
    fn start_push(&mut self, branch: Option<String>, revision: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
```

In its body, `cx.spawn_in(window, async move |this, cx| finished(push(this, cx).await)).detach();` becomes `cx.spawn_in(window, async move |this, cx| finished(push(this, cx, revision).await)).detach();`.

3. `pub(super) async fn push(this: This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {` becomes `pub(super) async fn push(this: This, cx: &mut AsyncWindowContext, revision: Option<String>) -> anyhow::Result<()> {`. In its body, `let input = PushDialogInput { repos, selected, has_hook, protected_branches: protected };` becomes:

```rust
    let input = PushDialogInput { repos, selected, has_hook, protected_branches: protected, revision };
```

4. In `app-gpui/src/workbench/commit.rs`, `push(this.clone(), cx).await?;` becomes `push(this.clone(), cx, None).await?;`.

The dialog still lists every outgoing commit of the branch, not only those up to the commit. The source lists `upstream..revision`; `push_repos` would need the revision for that.

## 8. Additions the views need

### 8a. The commit context menu (exact; required)

`Workbench::show_commit_menu` (in `log_actions.rs`) builds the menu with `vcs_ops::git_log::commit_menu` and shows it with `show_menu`. The `vcs.log.*` rows run through the registry with the commit key as argument. `LogCommand::Branch` rows go to `run_branch_command`, Copy Commit Message copies the message, and Bookmark / Assign Mnemonic / Browse Repository at Revision are logged as unavailable. It reads no outgoing commits (a default `vcs_ops::log_store::GitLog`), so no commit counts as pushed and the history rewriting rows are never disabled. Filling `outgoing` needs `vcs_log` with range `@{u}..HEAD` before the menu shows.

`app-gpui/src/vcs/log_view.rs`:

1. Replace

```rust
/// What the log tells its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitLogEvent {
    /// Another commit is selected, or none.
    SelectionChanged,
}
```

with

```rust
/// What the log tells its window.
#[derive(Debug, Clone, PartialEq)]
pub enum GitLogEvent {
    /// Another commit is selected, or none.
    SelectionChanged,
    /// Show the commit context menu of the selected commit at `position`.
    ShowMenu { position: Point<Pixels> },
}
```

`Pixels` is not `Eq`, so `Eq` goes. Nothing compares `GitLogEvent`s (`grep -rn GitLogEvent app-gpui/src`).

2. Add `MouseButton, MouseDownEvent, Point` to the `use gpui_kit::{ … }` list.

3. In `fn commit_list`, replace

```rust
                            .on_click(cx.listener(move |log, _, window, cx| {
                                window.focus(&log.focus, cx);
                                log.select(index, cx);
                            }))
```

with

```rust
                            .on_click(cx.listener(move |log, _, window, cx| {
                                window.focus(&log.focus, cx);
                                log.select(index, cx);
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |log, event: &MouseDownEvent, window, cx| {
                                    window.focus(&log.focus, cx);
                                    log.select(index, cx);
                                    cx.emit(GitLogEvent::ShowMenu { position: event.position });
                                    cx.stop_propagation();
                                }),
                            )
```

`app-gpui/src/workbench/log_window.rs`: the `let GitLogEvent::SelectionChanged = event;` is irrefutable only while there is one variant. Replace

```rust
        let selected = cx.subscribe_in(&log, window, |workbench, _, event: &GitLogEvent, _, cx| {
            let GitLogEvent::SelectionChanged = event;
            workbench.feed_log_details(cx);
        });
```

with

```rust
        let selected = cx.subscribe_in(&log, window, |workbench, _, event: &GitLogEvent, window, cx| match event {
            GitLogEvent::SelectionChanged => workbench.feed_log_details(cx),
            GitLogEvent::ShowMenu { position } => workbench.show_commit_menu(*position, window, cx),
        });
```

The test `the_commit_context_menu_offers_the_commit_actions_and_runs_the_chosen_one` calls `show_commit_menu` directly. A test that right-clicks a row (`commit:<id>` element) can follow once 8a is in.

### 8b. `vcs/log_view.rs`: several selected commits

- The view keeps one `selected: Option<usize>`. Squash Commits, Drop Commits, Compare Versions, Cherry-Pick and Revert of several get their commits only as an argument (a list of keys), which is what the menu passes.
- Add a `selection: Vec<usize>` with ⌘ / Ctrl click to toggle and ⇧ click for a range. The model is `vcs_ops::log_store::GitLog::{select_commit, toggle_commit_selection}`.
- Add `pub fn selected_commits(&self) -> Vec<&Commit>`, newest first. Then `Workbench::log_commits` in `log_actions.rs` takes it, instead of `selected()`, when there is no argument.

### 8c. `vcs/log_view.rs`: Collapse Linear Branches

- The view draws every loaded commit whatever `options.collapse_linear` says. The actions store the option and the view's options follow it, but the rows do not change.
- To draw the collapsed graph: when `options.collapse_linear` (or `linearize_merges`) is on, `commit_list` draws `vcs_more::log_presentation::present_log_graph(&commits, &options, &fragments, &keep)`. Its `commits` are indices into the loaded commits and its `rows` are graph rows, with `keep` = the selection plus HEAD. The selection then has to map through those indices.
- The test `collapse_and_expand_linear_branches_set_the_option_the_log_presents_its_graph_with` asserts the stored option, the view's option, and what `present_log_graph` gives for the view's commits and options (3 of 6 rows: `collapse_linear` hides the unreferenced one-parent one-child commits, as `vcs/graph.rs`'s own tests show). It does not assert rows the view draws.
- With Linearize Merges on, the source sets the merge fragments instead of the option (`logStore.ts` `updateLogOptions`). No fragments exist here, so that case only changes the option.

### 8d. The log toolbar

- The view has no toolbar. Go to Hash/Branch/Tag is reachable from the filter bar (`LogFilterEvent::GoToRef` already runs `vcs.log.goToRef`) and from the keymap (`Meta+F` in `intellij-macos.ts`).
- Collapse / Expand Linear Branches belong in a Graph Options menu (`GitLog.tsx` around `{ kind: "action", action: "vcs.log.collapseLinear" }`).

## 9. What `log_actions.rs` holds (for orientation)

- **`register_actions`**: 21 commit actions, 5 Branches pane actions, 6 changed files actions, plus Go to Hash/Branch/Tag, Show Commit Bookmark, Collapse / Expand Linear Branches, and the toggles Group by Directory and Show Diff Preview. The toggles read the stored log options: the workbench entity cannot be read while a menu is being built.
- **`run_log_operation`**: the long Git operations (cherry-pick, revert, reset, interactive rebase). It is the counterpart of `branches::run_git_operation`, with an owned success text, a "stopped for editing" text, conflicts opening the Conflicts dialog instead of failing, and an Undo button. **Merge the two at integration**: `run_git_operation` takes `&'static str` texts and none of the rest.
- **The `UNDO_PREFIX` follow-ups**: `LogActionsSlot.undo` maps a button command to the HEAD before and after a rewrite; `run_log_undo` runs `vcs_restore_head`.
- **Dialogs**: they go in the existing VCS dialog layer through `open_answer_dialog`, which uses `open_vcs_dialog` / `close_vcs_dialog`. Questions use `show_prompt_dialog` (`PromptDialog::ask` / `confirm` / `prompt`).
- **Dialogs to merge with other agents' work**:
  - `vcs/log_reset_dialog.rs` with the Reset HEAD… dialog of `vcs.resetHead`. Theirs adds a "To commit:" field.
  - `vcs/log_tag_dialog.rs` with the Tag dialog of `vcs.newTag`. Theirs adds Force, Commit and Message.

## 10. IPC commands each action stands for

| Action                                                                                                              | IPC commands                                                                                           |
| ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| cherryPick                                                                                                          | `vcs_log_details` (record origin), `vcs_operation` (`cherryPick`)                                      |
| revert                                                                                                              | `vcs_operation` (`revert`)                                                                             |
| resetToHere                                                                                                         | `vcs_operation` (`reset`)                                                                              |
| interactiveRebase, dropCommit, squashCommits                                                                        | `vcs_branch_info`, `vcs_log` (range), `vcs_operation` (`interactiveRebase`), `vcs_restore_head` (Undo) |
| editMessage                                                                                                         | `vcs_log` (range), `vcs_log_details` (truncated message), `vcs_reword`, `vcs_restore_head` (Undo)      |
| undoCommit                                                                                                          | `vcs_undo_commit`                                                                                      |
| checkoutRevision                                                                                                    | `vcs_checkout`                                                                                         |
| compareWithLocal                                                                                                    | `vcs_diff_with_worktree`, then `vcs_file_at` / `fs_read_text` for the sides                            |
| compareVersions                                                                                                     | `vcs_diff_revisions`, then `vcs_file_at`                                                               |
| newBranch, branches.new                                                                                             | `vcs_branches`, `vcs_create_branch`                                                                    |
| newTag                                                                                                              | `vcs_tags`, `vcs_create_tag`                                                                           |
| rebaseOntoCommit                                                                                                    | `vcs_rebase`                                                                                           |
| pushUpToHere                                                                                                        | `vcs_branch_info`, `vcs_remotes`, `vcs_log`, `vcs_has_hook`, `vcs_remote` (`pushBranch`)               |
| openInBrowser                                                                                                       | `vcs_config_get`                                                                                       |
| goToRef, revealBookmark, branches.navigate                                                                          | `vcs_log` (`branch` = the ref, `limit` 1)                                                              |
| fixup, squashInto, copyRevision, goToParent, goToChild, collapseLinear, expandLinear, groupByDirectory, diffPreview | none                                                                                                   |
| branches.update                                                                                                     | `vcs_remote` (`updateBranch`)                                                                          |
| branches.delete                                                                                                     | `vcs_worktrees`, `vcs_delete_branch`; `vcs_remote` (`deleteBranch`) for a remote one                   |
| branches.toggleFavorite                                                                                             | none (UI storage)                                                                                      |
| changes.showDiff, changes.compareWithLocal, changes.compareBeforeWithLocal                                          | `vcs_file_at`, `fs_read_text`                                                                          |
| changes.jumpToSource                                                                                                | `doc_open`                                                                                             |
| changes.getFromRevision                                                                                             | `vcs_file_at`, `fs_create_file`, `fs_delete`, the document's `save`                                    |
| changes.revert                                                                                                      | `vcs_create_patch` (`commits`, `reverse`), `vcs_apply_patch`                                           |

## 11. Not registered

- `vcs.log.createPatch` (**left out against the assignment**, which asked for an own patch dialog file to merge later): it opens the Create Patch dialog (`patches.tsx` `CreatePatchDialog`, with origin `commits`). That dialog belongs to `vcs.createPatch`, which the agent writing `workbench/vcs_operations.rs` builds now: file path, base path, encoding, copy to clipboard, and the question after saving. Once it exists, wire it with the commits of `Workbench::log_commits` as the origin and `PatchSource::Commits { commits (oldest first), reverse: false }`.
- `vcs.log.openExternal`: it needs a native folder chooser for several folders and a log tab of its own for repositories outside the project (`openLogTab("external", …)`), and neither exists here. `ide_host::ops::vcs::vcs_open_external_log` is there for it.

## 12. Deviations from the source, and assumptions to check first

### Deviations

- **Compare with Local / Compare Versions** open the diff window on the first differing file, with the others as its chain. The source opens a "Changes between …" file list in a window of its own (`ChangesBetweenDialog`); that list is not built. An empty comparison is told as "No changes against {hash} in 1 repository".
- **Go to Hash/Branch/Tag** is a plain prompt. The source's chooser also completes branch and tag names. It selects only commits the log has read: it does not page further, clear filters, or pick between repositories as `goToRef` does.
- **Go to Parent / Child** with several candidates offers them in a popup menu, not a titled chooser. Only loaded commits are candidates.
- **Undo Commit…** skips the change list chooser (`chooseTargetChangelist`), because change lists belong to another agent. The changes stay staged in the default list.
- **The progress notification** of a log operation has no Cancel button.
- **Multi-commit order**: a list argument is taken in the log's order (newest first), whatever order it came in. The source keeps the argument's order but every caller passes the log's order.
- **The changed files actions** take repository-relative paths as their argument (`["a.txt"]`) where the source takes `LogChange` objects, which are not serialisable here. Without an argument they act on the pane's selection, as the source does.
- **Delete Branch** of several remote branches asks once per branch, through `BranchCommand::DeleteSelection`. The source asks once for all of them.
- **The guards of `withCommit` / `withCommits`** are not reproduced: commits of a read-only log, and several selected commits from different repositories. This log reads one repository and nothing read-only.
- **Commit actions find their commits only once the Git tool window was shown**: they resolve keys among the commits the log has read. Before that, they say "Select a commit in the Git log".
- **View in Browser** takes the remote as the upstream up to the first `/`. The source's `slice(0, indexOf("/"))` would cut the last character of an upstream without a slash.

### Assumptions

- `git rebase` with `GIT_EDITOR=true` (set in `crates/ide-vcs/src/cli.rs`) keeps the combined message of a squash, so the subject is the first commit's. Check if `interactive_rebase_lists_the_commits_oldest_first_and_applies_the_plan` fails on `subjects_in`.
- `ide_host::ops::vcs::vcs_operation` returns `Err` when a cherry-pick stops on conflicts, and `CHERRY_PICK_HEAD` is left. Check: `crates/ide-vcs` `cherry_pick_with`.
- `RemoteOp::UpdateBranch` fetches the branch from its upstream in a local bare remote without network.
- `checkout_step`'s `Switched` notice is titled with the success text given, as for "Checked out {name}" in `branches.rs`.
- `deleted_branch_notice` titles the notice "Deleted branch {name}", as the existing test `new_branch_rename_and_delete_change_the_repositorys_branches` shows.
- `AsyncWindowContext::background_executor()` is available through its `AsyncApp` deref (used in `go_to_ref` to wait for a log that is still reading).
- `VcsState.roots` holds the same root string the test reads back for the bookmark.

## 13. Tests most likely to fail on first run

- `push_all_up_to_here_pushes_the_branch_to_the_bare_remote`: needs section 7.
- `edit_commit_message_rewords_the_head_and_an_older_commit_and_undo_restores_it`: its Undo needs section 2e.
- `collapse_and_expand_linear_branches_set_the_option_the_log_presents_its_graph_with`: it expects 3 of 6 rows (merge, `feature three` with its branch ref, `first`), and assumes the log's `refs` of `feature three` include `feature`.
- `drop_commit_removes_it_after_the_confirmation`:
  - it clicks `prompt-checkbox`, then presses Enter, and expects the focus to stay on the dialog;
  - it then expects `settings::set` with `SettingsScope::Global` to be read back by `settings::effective`.
- `the_changed_files_actions_act_on_the_files_of_the_selected_commit`:
  - Escape after each diff relies on the diff window having the keyboard;
  - Revert Selected Changes relies on `filter_patch` matching `a.txt` in the reversed patch's `diff --git` header.
- `the_branches_pane_actions_act_on_the_selected_branch`: the update goes through `run_remote`, and the test waits on the branch tip, not on a notice.
- `cherry_pick_applies_a_commit_of_another_branch_and_a_conflicting_one_opens_the_conflicts`: it needs the conflicted status to be read before the failure is reported (`refresh` runs first, as in `run_op`).

## 14. Spec files

None of the new tests is named after a spec sentence. The specs of these actions are unit specs, and existing `#[test]`s named after them already cover their models. The counts below come from `grep -rh "// src/features/vcs/__tests__/<file>" app-gpui/src`, distinct cases against the `it(` lines of the file.

| Spec file                | Covered  | Where                                                                                                                                                          |
| ------------------------ | -------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `historyOps.test.ts`     | 10 of 10 | the commit menu in `vcs_ops/git_log.rs`; the selection in `vcs_ops/log_store.rs`; tag names in `vcs_ops/operations.rs`; the rest in their own models           |
| `gitOps.test.ts`         | 14 of 14 | Undo Commit and the interactive rebase plan in `vcs_ops/rebase_plan.rs`, among others                                                                          |
| `deleteBranches.test.ts` | 10 of 10 | `vcs_ops/branch_ops.rs`                                                                                                                                        |
| `logPanes.test.tsx`      | 8 of 12  | not covered by any test: "offers Collapse and Expand Linear Branches as two actions" (there is no Graph Options menu); the other 3 were not checked one by one |
