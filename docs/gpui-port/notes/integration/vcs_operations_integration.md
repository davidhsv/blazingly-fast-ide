# Integrating the Git repository operations into the workbench

New files, not compiled yet:

- `app-gpui/src/vcs/git_forms.rs`: the forms of the dialogs as data, plus their plain tests.
- `app-gpui/src/vcs/git_form_dialog.rs`: the view that draws any form, plus its GPUI tests.
- `app-gpui/src/workbench/vcs_operations.rs`: the actions, the flows and the end-to-end tests.

Sections 1 to 6 are all required. Every anchor is quoted from the file as it is now. Line numbers are left out because `mod.rs` is moving. Run `cargo fmt` after pasting.

## 1. `app-gpui/src/vcs/mod.rs`

Add these lines in alphabetical order, after `pub mod conflicts_dialog;`:

```rust
pub mod git_form_dialog;
pub mod git_forms;
```

## 2. `app-gpui/src/vcs/prompt_dialogs.rs`: share the drawing helpers

`git_form_dialog.rs` draws its checkboxes, buttons and fields with the same helpers as the prompt dialogs. Make four functions `pub(crate)`:

- `fn code_font(cx: &App) -> SharedString {` becomes `pub(crate) fn code_font(cx: &App) -> SharedString {`
- `fn checkbox(cx: &App, label: String, on: bool) -> Div {` becomes `pub(crate) fn checkbox(cx: &App, label: String, on: bool) -> Div {`
- `fn button(cx: &App, label: String, primary: bool, danger: bool, enabled: bool) -> Div {` becomes `pub(crate) fn button(cx: &App, label: String, primary: bool, danger: bool, enabled: bool) -> Div {`
- `fn field_frame(cx: &App, invalid: bool) -> Div {` becomes `pub(crate) fn field_frame(cx: &App, invalid: bool) -> Div {`

## 3. `app-gpui/src/workbench/branches.rs`

### 3a. Visibility for the sibling module

- `fn i18n<R>(this: &This, cx: &mut AsyncWindowContext, f: impl FnOnce(&mut I18n) -> R) -> anyhow::Result<R> {` becomes `pub(super) fn i18n<R>(…`.
- `async fn run_remote(` becomes `pub(super) async fn run_remote(`.
- `async fn ask(this: &This, cx: &mut AsyncWindowContext, request: AskRequest) -> anyhow::Result<DialogAnswer> {` becomes `pub(super) async fn ask(…`.
- `async fn confirm(this: &This, cx: &mut AsyncWindowContext, request: ConfirmRequest) -> anyhow::Result<bool> {` becomes `pub(super) async fn confirm(…`.
- `async fn run_git_operation(` becomes `pub(super) async fn run_git_operation(`.

### 3b. `run_git_operation` takes texts built at run time

Reset HEAD's success text is built from the branch, the commit and the mode, so it cannot be `&'static str`. In the parameter list of `run_git_operation`, replace

```rust
    progress: &'static str,
    success: &'static str,
```

with

```rust
    progress: &str,
    success: &str,
```

The body only borrows them inside synchronous `this.update` closures (`tx(progress)`, `vcs_notice(.., success)`, `console.log(.., success, ..)`), so nothing else changes. The one existing caller, `control_operation`, passes `&'static str` values, which still fit.

## 4. `app-gpui/src/workbench/commit.rs`

### 4a. The Amend state, for the toggle of `vcs.toggleAmend`

In `impl Workbench`, after `pub fn commit_box(&self) -> Option<&Entity<CommitBox>> { … }`, add:

```rust
    /// Whether Amend is checked in the window's commit state.
    pub(super) fn commit_amend(&self, cx: &App) -> bool {
        self.commit.as_ref().is_some_and(|slot| slot.state.read(cx).amend)
    }
```

Then add `App` to the import `use gpui_kit::{AppContext, AsyncWindowContext, Context, Entity, Subscription, Window};`.

### 4b. The message history, for an Amend checked without the message box

`fn remember_commit_message(&mut self, message: &str, cx: &mut Context<Self>) {` becomes `pub(super) fn remember_commit_message(…`.

## 5. `app-gpui/src/workbench/mod.rs`

### 5a. The module

After `mod todo_window;` add:

```rust
mod vcs_operations;
```

### 5b. The `Workbench` field

In `pub struct Workbench`, after

```rust
    conflicts: conflicts::ConflictsSlot,
```

add:

```rust
    /// What the Git menu's repository operations keep: Show Commit in Git Log waiting for the log.
    vcs_operations: vcs_operations::VcsOperationsSlot,
```

### 5c. Its initial value

In `Workbench::new`, in `let mut workbench = Self { … }`, after `conflicts: Default::default(),` add:

```rust
            vcs_operations: Default::default(),
```

### 5d. The actions

In `pub fn register_actions(cx: &mut App)`, after `conflicts::register_actions(registry);` add:

```rust
    vcs_operations::register_actions(registry);
```

### 5e. The test helpers the module's tests use

The end-to-end tests are in `vcs_operations.rs` (`mod tests`, which does `use super::super::tests::{…}`). In `#[cfg(test)] mod tests` of `mod.rs`, prefix each of these with `pub(super) `:

- `    fn wait_long(cx: &mut VisualTestContext, what: &str, done: impl Fn(&mut VisualTestContext) -> bool) {`
- `    fn run_action(cx: &mut VisualTestContext, id: &str, arg: Option<serde_json::Value>) {`
- `    fn git_in(dir: &std::path::Path, args: &[&str]) -> String {`
- `    fn init_repo(root: &std::path::Path) {`
- `    fn git_head(root: &std::path::Path) -> String {`
- `    fn git_project(` (the one that returns `(Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 2], PathBuf)`)
- `    fn notice_titled(`
- `    fn wait_for_prompt(`
- `    fn notification_titles(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {`
- `    fn wait_for_notice(workbench: &Entity<Workbench>, title: &str, cx: &mut VisualTestContext) {`

Without the prefix the helpers are private to `workbench::tests`, and `workbench::vcs_operations::tests` is not inside it.

## 6. Menus

Nothing is added. `menu_spec.rs` already has every action registered here in the Git menu: `vcs.update`, `vcs.pull`, `vcs.merge`, `vcs.merge.abort`, `vcs.rebase`, `vcs.rebase.abort/continue/skip`, `vcs.newTag`, `vcs.resetHead`, `vcs.newWorktree`, `vcs.worktrees`, `vcs.showLog`, `vcs.stash`, `vcs.unstash`, `vcs.rollback`, `vcs.addToVcs`, `vcs.manageRemotes` and `vcs.operations`. The Project view's context menu (`project_view/context_menu.rs`) and the Commit window's (`vcs_ops/commit_view.rs`) already name `vcs.rollback`, `vcs.addToVcs`, `vcs.ignoreShared`, `vcs.ignoreLocal`, `vcs.merge`, `vcs.rebase`, `vcs.newTag`, `vcs.resetHead`, `vcs.stash` and `vcs.unstash`. With the actions registered, those rows now show.

## 7. What `vcs_operations.rs` holds

- **`register_actions`** registers 29 action ids, all through `registry.handle`. All are in `assets/gen/actions.json`. `vcs.toggleAmend` goes through `handle_toggle`, read with `for_window` as `ide.toggleMode` does.
- **Forms:**
  - `show_git_form` opens a `GitFormDialog` in the vcs dialog layer. Its answers go into an unbounded channel. Cancel and Escape close it.
  - `ask_form` waits for one answer and closes the dialog.
  - New Worktree keeps its dialog open on a failure (`set_error`).
  - Git Remotes and Worktrees reopen after each entry they ask about.
- **IPC commands each action stands for (parity records):**
  - `vcs.pull`: `vcsRemotes`, `vcsBranches`, then `vcsRemote` with `{op: "pullFrom"}`.
  - `vcs.update`: `settingsSet` (update method, Don't show again), then `vcsRemote` with `{op: "pull"}`. Reset to the Remote Branch uses `vcsRemote` with `{op: "resetToUpstream"}`.
  - `vcs.merge`: `vcsBranches`, `vcsMergeWith`.
  - `vcs.rebase`: `vcsBranches`, `vcsRebaseWith`.
  - The eight controls: `vcsOperation` with `continue`, `skip` or `abort`, through `control_operation`.
  - `vcs.resetHead`: `vcsOperation` with `{op: "reset"}`.
  - `vcs.newTag`: `vcsTags`, `vcsCreateTag`.
  - `vcs.stash`: `vcsStashPush`.
  - `vcs.unstash`: `vcsStashes`, `vcsStashApply`.
  - `vcs.rollback`: `vcsUnstage`, `vcsDiscard`.
  - `vcs.addToVcs`: `vcsStage`.
  - `vcs.ignoreShared` and `vcs.ignoreLocal`: `vcsAddToIgnore`.
  - `vcs.manageRemotes`: `vcsRemotes`, `vcsEditRemote`.
  - `vcs.newWorktree`: `vcsBranches`, `vcsAddWorktree`.
  - `vcs.worktrees`: `vcsWorktrees`, `vcsRemoveWorktree`.
  - `vcs.toggleAmend`: `vcsLog` (HEAD, limit 1).
  - `vcs.refresh`, `vcs.showLog`, `vcs.showCommit`, `vcs.openDiff` and `vcs.operations` call no command of their own (the refresh is the window's usual read).

## 8. Not wired, and why

- **`vcs.compareWithBranch`.** It needs the revision chooser popup (`RevisionChooser.tsx`) and the "Changes between" comparison window. Neither has a view in `app-gpui`. A text prompt would be a different dialog.
- **`vcs.showLog` with `{branch}` or `{path}`.** The Rust `GitLog` has no tabs. Such an argument shows the main log and logs why. The webview opens a tab of its own.
- **`vcs.showCommit` paging.** The commit is selected only if it is among the commits read. The webview pages the log until it finds it, and clears active filters first. Neither is done here.
- **`vcs.rollback` in a focused editor with the caret on changed lines (Rollback Lines).** It needs the gutter's line hunks in the editor, which belongs to the gutter feature.
- **Tag message.** It is a one-line field; the webview has a 4-row textarea.
- **Pull dialog layout.** The remote and the branch are stacked, not side by side.
- **New Tag and Remotes do not reload the Git log** after they change refs. The log's `reload` lives behind `LogSlot` (private to `log_window.rs`).
- **Add to .gitignore does not reload the project tree.** The webview calls `reloadAll()`; here the tree follows the file watcher.

## 9. Assumptions, with where to check

1. **Async fn items as `impl AsyncFnOnce(This, &mut AsyncWindowContext) -> anyhow::Result<()>`** (`start`).
   - If rustc refuses the higher-ranked bound, write each registration out instead: `on_workbench(window, cx, |_, window, cx| cx.spawn_in(window, async move |this, cx| finished(pull(this, cx).await)).detach())`.
2. **`git rebase/cherry-pick/revert --continue` need no editor.** `GIT_EDITOR=true` is set by the backend's git command builder (`crates/ide-vcs/src/cli.rs`, as the conflicts notes say).
3. **`BranchInfo.operation` reads `"cherry-pick"` and `"revert"`** while `CHERRY_PICK_HEAD` / `REVERT_HEAD` exist. Check how `ide-vcs` computes it if the cherry-pick test times out on "the cherry-pick is read".
4. **`Stash.message` is git's `On main: half done`.** The test only checks the end of the label.
5. **Deferred options inside the dialog layer.** The options list is `deferred(..).with_priority(2)` inside the `deferred(..).with_priority(1)` dialog layer. This is the same nesting the Push dialog's remote picker uses.
6. **A click on a list row's checkbox does not also select the row**: `cx.stop_propagation()` is called in its `on_click`. If both fire, the only effect is that the row is selected too.
7. **`TextField` has no `set_placeholder`.** An empty field whose placeholder changes (New Worktree's suggested branch) is made again. Adding `pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>, cx: &mut Context<Self>)` to `ui/text_field.rs` would make this simpler.

## 10. Compile risks

- `AsyncFnOnce` in `start` (see 9.1).
- `cx.subscribe(&entity, move |dialog: &mut Self, entity, event: &TextFieldEvent, cx| …)` in `git_form_dialog.rs` relies on `Context::subscribe`'s callback taking the emitter by value (`Entity<TextField>`).
- `dialog.update(cx, …)` inside `this.update(cx, |_, cx| …)` in `new_worktree`.
- The match in `GitFormDialog::render_row` binds `id` as `&&'static str` and dereferences it (`*id`) for the select, radio and list.
- `Box::<ResetForm>::default()` passed where `Box<dyn GitForm>` is expected.

## 11. Tests most likely to fail on first run

- **`a_stopped_cherry_pick_and_revert_are_aborted_and_continued`**
  - It depends on 9.2 and 9.3.
  - The revert setup expects git to stop on a modify/delete conflict.
- **`the_operations_popup_lists_the_operations_in_order_and_runs_the_chosen_one`**
  - It compares against the registry at run time. If another module registers `vcs.showHistory`, `vcs.annotate` or `vcs.localHistory` with a `when` that is false here, the "all enabled" assertion fails.
- **`new_worktree_creates_one_that_the_worktrees_dialog_lists_and_removes`**
  - The worktree's key is `w:` plus the path as `git worktree list` prints it, compared with the canonicalised temp path. On macOS both should be `/private/var/…`.
- **`update_project_pulls_with_the_method_chosen_and_resets_to_the_remote_branch`**
  - It assumes a project-scope `settings::set` is visible to `settings::effective("w1", …)` at once.
