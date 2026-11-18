# Git operations the menus name and nothing carries out: integration notes

All paths are relative to the worktree. Every statement about the code was read at `HEAD` (cae23afb0) unless it says "Hypothesis:".

## (a) The list

Legend for the native column: what `git grep … HEAD -- app-gpui/src` finds. Verdicts: `build` (built in this slice), `already there` (only a test was missing), `needs` (named), `cannot be tested`.

| Row                                                          | What the webview does (file:line)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | What is native at HEAD                                                                                                                                                                                                                                                                                                                                                                                                                  | Verdict                                                                                                                                                                             |
| ------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:vcs.createRepository`                                | `src/features/vcs/actions.tsx:266` (`when: projectOpen && projectLocal`) → `src/features/vcs/createRepository.ts:9-26`: the system folder chooser titled "Select Directory for Git Init" starting at the project root; `commands.vcsInit(dir)`; failure → error notification "Couldn't create Git repository" with the message as body; success → info "Created Git repository in {dir}", then `discoverRoots()` (`src/features/vcs/store.ts:395`: when the backend now lists roots, `vcsAvailable` turns on and the repository is read). Also offered by the title bar's "Version Control ▾" while the project has no repository (`src/workbench/TitleBar.tsx:98`, first entry).                                                                                                                                                                                                                                                                                     | Nothing: `git grep -n -i "createRepository\|create_repository" HEAD -- app-gpui/src` finds only New Project's checkbox. The fallback menu has three entries (`app-gpui/src/workbench/title_bar.rs:47`). The `when` clause cannot hold: `projectLocal` is published nowhere (`git grep -n -i projectlocal HEAD -- app-gpui/src` → only `app-gpui/src/app_models/windows.rs:977`); the webview sets it at `src/workbench/activate.ts:49`. | build                                                                                                                                                                               |
| `context:projectLocal` (raised by the row above)             | `src/workbench/activate.ts:49`: on while a project is open, the window is not remote and not Light Edit.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Not published (see above); `project.attach`'s `when` (`app-gpui/src/app_models/windows.rs:977`) names it too.                                                                                                                                                                                                                                                                                                                           | build (one line in `publish_context`)                                                                                                                                               |
| `ipc:vcs_init`                                               | Sites: `src/features/vcs/createRepository.ts:14` (above) and New Project's "Create Git repository" (`src/features/app/NewProject.tsx:229`, through `scaffold_create`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | `app-gpui/src/workbench/app_actions.rs:1167-1172` calls `ide_host::ops::vcs::vcs_init` for New Project; no test runs it (the test file that would is not declared, see `ipc:scaffold_create`).                                                                                                                                                                                                                                          | build (Create Git Repository) + declare `proofs6_app_actions`                                                                                                                       |
| `ipc:scaffold_create` (partial)                              | `src/features/app/NewProject.tsx:229`: Create runs the chosen generator, optionally `git init`, opens the project.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | Empty folder only (`app-gpui/src/workbench/app_actions.rs:1146-1176`); `app-gpui/src/workbench/app_actions/tests/proofs6_app_actions.rs` (29 lines, one test `new_project_with_git_creates_a_repository_in_the_new_folder`) is not declared: `app_actions.rs:1213` declares only `mod proofs20_layouts;`.                                                                                                                               | already there (the test): declaring it needs the `mod` line and the backend's git kept off the machine's configuration (hunk B2); the row stays partial (generators, progress view) |
| `action:vcs.clone`                                           | `src/features/vcs/actions.tsx:275` (no `when`: also without a project; the welcome screen's Clone button `src/features/welcome/ProjectsPage.tsx:55`) → `showCloneDialog(arg)` (`src/features/vcs/clone.tsx:112`). Dialog "Clone Repository" (`clone.tsx:248-373`): Version control: Git; URL (placeholder `https://github.com/user/repository.git`, a recent-URL dropdown); Directory, derived as `<parent>/<cloneDirName(url)>` until edited by hand; Browse… (folder chooser "Directory to Clone Into", sets the parent); "Shallow clone with a history truncated to [1] commits"; a hint with the validation message once a URL is typed; Clone (disabled while invalid or busy) / Cancel (cancels a running clone, else closes). While cloning: the progress line ("Cloning…" until git says more). Success: remembers the parent and the URL, closes, opens the project (`openProject(target)`); a failure stays in the dialog, a cancel says "Clone cancelled". | `app-gpui/src/vcs_ops/clone.rs` (model: `clone_dir_name`, `join_path`, `parent_of`, `validate_clone`, `parse_clone_depth`, with the spec's tests); no handler (`git grep -n "vcs.clone" HEAD -- app-gpui/src` → menu entries only: `menu_spec.rs` "Clone…", `title_bar.rs:47`).                                                                                                                                                         | build                                                                                                                                                                               |
| `ipc:vcs_clone`                                              | `src/features/vcs/clone.tsx:191` (above).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | Not called anywhere. Runs `git clone` (`crates/ide-vcs/src/lib.rs:677-697`, `cli::run_streaming`); a local folder path as URL needs no network.                                                                                                                                                                                                                                                                                         | build                                                                                                                                                                               |
| `unit:src/features/vcs/__tests__/clone.test.ts` (partial)    | 4 cases: cloneDirName, joinPath, validateClone, parseCloneDepth (+ outgoingQuery, which is `remote_ui`'s).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | All 5 cases are model tests (`vcs_ops::clone::tests`, `vcs_ops::remote_ui::tests`); the clause's "nothing in the app reaches it" is what this slice builds.                                                                                                                                                                                                                                                                             | build (the dialog reaches the model)                                                                                                                                                |
| `ipc:vcs_delete_tag`                                         | `src/features/vcs/branchOps.ts:421-455` (`deleteTag`): runOp "Delete tag {name}"; notification key `vcs.deletedTag.{name}`, title "Deleted tag {name}", buttons Restore and, with remotes, "Delete on Remote" / "Delete on Remotes". Offered by Tag › Delete (`src/features/vcs/branchMenu.ts:146`) in the Branches popup (`src/features/vcs/BranchWidget.tsx:464`, `:715`) and the Git log's Branches pane (`src/features/vcs/GitLog.tsx:613`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | No call (`git grep -n "delete_tag" HEAD -- app-gpui/src` → nothing). The Branches pane emits `BranchesPaneEvent::ShowTagMenu` (`app-gpui/src/vcs/log_branches.rs:623`) and the workbench only logs it (`app-gpui/src/workbench/log_window.rs:407`); the popup's tag rows do nothing (`app-gpui/src/vcs/branches_popup.rs:798`).                                                                                                         | build (the Git log's Branches pane)                                                                                                                                                 |
| `ipc:vcs_restore_tag`                                        | `src/features/vcs/branchOps.ts:432-440`: the notice's Restore runs "Restore tag {name}", success "Restored tag {name}".                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | No call. The notification routing matches `vcs.deleted.` (`app-gpui/src/workbench/mod.rs:1424`), which does not prefix `vcs.deletedTag.`.                                                                                                                                                                                                                                                                                               | build                                                                                                                                                                               |
| `ipc:vcs_stash_drop`                                         | `src/features/vcs/changeOps.tsx:532-545` (`dropStash`): confirmation "Drop Stash" / "Drop stash@{n} “msg”? This cannot be undone." (no message: "Drop stash@{n}? This cannot be undone."), Drop (danger); runOp "Drop stash@{n}", success "Dropped stash@{n}"; the list is read again. Sites: the Unstash dialog's toolbar Drop Stash (`changeOps.tsx:458-465`, `:497`; afterwards the dialog chooses `max(0, min(index, len-2))`), the Commit view's stash list (`src/features/vcs/CommitView.tsx:1371`, `:1395`).                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `app-gpui/src/vcs_ops/change_ops.rs:279` (`drop_stash`: the texts) used only by its own test; the Unstash form has no toolbar (`app-gpui/src/vcs/git_forms.rs:1212-1321`); the Commit window has no stash list.                                                                                                                                                                                                                         | build (the Unstash dialog's Drop Stash); the Commit view's stash list: needs that list                                                                                              |
| `action:vcs.stashSelected`                                   | `src/features/vcs/actions.tsx:704-722`: the action's paths, else the Commit window's selection while it has the keyboard (`changedPaths`, `actions.tsx:120`); none → info "Select the files to stash in the Commit view first"; else the Stash dialog without the repository picker, and `vcsStashPush(repo, message, untracked, keepIndex, paths)` (`changeOps.tsx:316-345`). Offered in the changes tree's menu after Create Patch… (`CommitView.tsx:417`, disabled without files).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | Not registered (`git grep -n stashSelected HEAD -- app-gpui/src` → nothing outside `assets/gen/actions.json:8877`); the changes tree's menu (`app-gpui/src/vcs_ops/commit_view.rs:185-188`) lacks it.                                                                                                                                                                                                                                   | build                                                                                                                                                                               |
| `unit:src/features/vcs/__tests__/stashSelectedFiles.test.ts` | 2 cases: the menu item targets the selected files; it is disabled without files.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | No row in `parity.json`; `node_menu` lacks the item.                                                                                                                                                                                                                                                                                                                                                                                    | build                                                                                                                                                                               |
| `ipc:vcs_stash_push` (partial)                               | Clause: the `paths` argument of Stash Selected Files.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | `vcs_operations.rs:529` always passes `None`.                                                                                                                                                                                                                                                                                                                                                                                           | build (Stash Selected passes the paths)                                                                                                                                             |
| `ipc:vcs_stashes` (partial)                                  | Clause: the Commit window's stash list of every root (`CommitView.tsx:133`, `:1324`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | Unstash dialog only (`vcs_operations.rs:542`).                                                                                                                                                                                                                                                                                                                                                                                          | needs: the Commit window's stash list (a view of its own: rows per root, Apply/Pop/Drop, Show Changes)                                                                              |
| `ipc:vcs_move_worktree`                                      | `src/features/vcs/gitMenuDialogs.tsx:756-782`: Worktrees › Move Worktree… (disabled on the main worktree and on the one holding the project, tooltip "Cannot move a worktree containing the open project."): folder chooser "Move Worktree" starting at the worktree's parent; `worktreeMoveTarget` (`src/features/vcs/worktrees.ts:32`); no move onto itself; confirm "Move Worktree" / "Move the worktree at {from} to {to}?" / Move; the moved row is selected; a failure shows in the list.                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | `vcs_add_worktree`, `vcs_remove_worktree`, `vcs_worktrees` only; the Worktrees form has Refresh, New, Remove, Open (`git_forms.rs:1701-1710`).                                                                                                                                                                                                                                                                                          | build                                                                                                                                                                               |
| `ipc:vcs_ignore_blame_revision`                              | `src/features/vcs/blame.ts:286-289`, `:332-344`: the annotation column's menu, for a committed line, "Ignore This Commit" (icon `hide`) after Copy Revision Number; success re-annotates the live file; failure → error "Cannot ignore the commit" with the message. (The brief calls it "Ignore Revision": the label is "Ignore This Commit".)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | `app-gpui/src/workbench/annotate.rs:303-320`: Show Commit in Git Log, Copy Revision Number, Close Annotations.                                                                                                                                                                                                                                                                                                                          | build                                                                                                                                                                               |
| `ipc:vcs_stage_hunks`                                        | `src/features/vcs/DiffEditor.tsx:980-997`: the diff viewer's per-change Stage button (a working-tree-vs-index diff): `vcsStageHunks(path, indices)`, then the status is read again; nothing overlapping → console line "Nothing to stage for this change".                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | No call; the native diff window draws no per-change controls (`git grep -n -i "stage" HEAD -- app-gpui/src/vcs_diff app-gpui/src/workbench/diff_window.rs` → nothing).                                                                                                                                                                                                                                                                  | needs: per-change gutter controls in the diff viewer (the Stage/Revert arrows of a change)                                                                                          |
| `ipc:vcs_file_mode`                                          | `src/features/vcs/diffInput.ts:310-322` (`loadSideMode`): each side's mode; "File mode changed" when the texts are equal and the modes differ.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | No call; no mode in `crate::vcs_diff::diff_input`.                                                                                                                                                                                                                                                                                                                                                                                      | needs: the diff input's side modes and the "File mode changed" header of the diff viewer                                                                                            |
| `ipc:vcs_revision_files`                                     | `src/features/vcs/PathFilterDialog.tsx:76` (the log's path filter lists HEAD's files), `RevisionBrowser.tsx:96`, `fileRevisions.ts:25`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | No call; no path filter dialog, no revision browser, no file chooser of a revision.                                                                                                                                                                                                                                                                                                                                                     | needs: the log's Paths filter dialog, Browse Repository at Revision, the file-of-a-revision chooser                                                                                 |
| `ipc:vcs_revision_children`                                  | `src/features/vcs/RevisionBrowser.tsx:80`, `:118`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | `vcs_ops/git_log.rs:70` offers "Browse Repository at Revision" with no window behind it.                                                                                                                                                                                                                                                                                                                                                | needs: the revision browser view                                                                                                                                                    |
| `ipc:vcs_compare_paths`                                      | `src/features/vcs/operations.tsx:657`: Compare with Branch or Revision… on a folder or several files.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | Refused on purpose (`app-gpui/src/workbench/diff_tools.rs:336-341`).                                                                                                                                                                                                                                                                                                                                                                    | needs: the path comparison view (a list of the changed files of the scope with their diffs)                                                                                         |

Rows found on the way (same pieces):

| Row                                                                                                                                                      | Webview                                                            | Native                                           | Verdict                                                                                                           |
| -------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ | ------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------- |
| title bar "Version Control ▾" entry `vcs.createRepository` (`module:src/workbench/TitleBar.tsx` is `[~]` in INVENTORY.md:2293)                           | `src/workbench/TitleBar.tsx:98` lists `vcs.createRepository` first | `title_bar.rs:47` lacks it                       | build (one-line hunk)                                                                                             |
| the changes tree's menu row "Stash Selected Files…"                                                                                                      | `CommitView.tsx:417`                                               | `commit_view.rs:185-188` lacks it                | build                                                                                                             |
| Branches popup › a tag's submenu (Checkout, New Branch from…, Compare with Current, Show Diff with Working Tree, Show History, Push to {remote}, Delete) | `BranchWidget.tsx:464`, `:715` with `tagMenu`                      | `branches_popup.rs:798`: a tag row opens nothing | needs: the popup's submenu for a tag row (its `Line::Tag` has no submenu; `open_submenu` builds branch rows only) |

Totals: build 15 (including the 3 rows found on the way: the fallback entry, the changes tree's row, `context:projectLocal`), already there 1 (`proofs6_app_actions`, on hunk B2), needs 8 (stage hunks, file mode, revision files, revision children, compare paths, the Commit view's stash list for `vcs_stashes`, the popup's tag submenu, and the Commit-view site of stash drop folded into `vcs_stash_drop`'s note), cannot be tested 0 (on hypothesis H1).

## Files written (all untracked, none declared)

Rename each on integration by dropping `_pending` (the `mod` lines below use the final names).

- `app-gpui/src/vcs_ops/git_ops_pending.rs` → `vcs_ops/git_ops.rs`: `DELETED_TAG_PREFIX`, `TagCommand`, `tag_menu`, `TagFollowUp`, `deleted_tag_notice`, `notice_key_of`, `delete_tag_title`, `push_tag_detail`, `remote_tag_detail`, `create_repository_notice`, `nothing_to_stash_notice`, `worktree_move_target`, `move_worktree_confirm`; 4 `#[test]`s.
- `app-gpui/src/vcs/clone_form_pending.rs` → `vcs/clone_form.rs`: `CloneForm` (a `GitForm`), `CloneAnswer`, `PARENT_KEY`, `RECENT_URLS_KEY`, `BROWSE`, `recent_urls`, `with_recent_url`; 4 `#[test]`s.
- `app-gpui/src/workbench/vcs_operations/menu_ops_pending.rs` → `workbench/vcs_operations/menu_ops.rs` (a child of `vcs_operations`, so it uses that module's private `open_form`, `close_form`, `current`, `repo_path`, `start`, `Answers`, `Workbench::selection`): registers `vcs.createRepository`, `vcs.clone`, `vcs.stashSelected`; `MenuOpsSlot`; `move_worktree`; `Workbench::{create_repository, stash_selected, show_tag_menu, run_tag_command, run_tag_follow_up}`.
- `app-gpui/src/workbench/vcs_operations/tests/proofs49_git_ops_pending.rs` → `…/tests/proofs49_git_ops.rs`: 6 window tests.
- `app-gpui/src/workbench/vcs_operations/tests/proofs49_clone_pending.rs` → `…/tests/proofs49_clone.rs`: 1 window test.
- `app-gpui/src/workbench/annotate/tests/proofs49_ignore_commit_pending.rs` → `…/annotate/tests/proofs49_ignore_commit.rs`: 1 window test.
- `app-gpui/src/vcs_ops/commit_view/tests/proofs49_stash_selected_pending.rs` → `…/commit_view/tests/proofs49_stash_selected.rs`: 2 spec `#[test]`s.

The window tests sit beside `vcs_operations` (as `proofs11_worktrees.rs` does) because they use that module's private test helpers (`wait_for_form`, `press`, `rows`, `list`, `can`, `no_form`, `origin_project`).

## (b) Hunks for existing files, by piece

Each piece lands alone in the order given; B0 first (every Git test of the slice needs it).

### B0. The backend's git never reads the machine's configuration in tests (`app-gpui/src/workbench/mod.rs`, `mod tests`, `start_in`)

Anchor:

```rust
            crate::terminal::test_support::isolate_shells(home.path(), cx);
```

Add after it:

```rust
            // Every git the backend starts reads neither the user's nor the system's configuration.
            ide_vcs::set_askpass_env(vec![
                ("GIT_CONFIG_GLOBAL".to_owned(), "/dev/null".to_owned()),
                ("GIT_CONFIG_SYSTEM".to_owned(), "/dev/null".to_owned()),
                ("GIT_CONFIG_NOSYSTEM".to_owned(), "1".to_owned()),
            ]);
```

Why it works: `crates/ide-vcs/src/cli.rs:252-281` (`command()`) is how every backend git child is built (`run_raw_env_once` `:361`, `:477`, `run_streaming_env` `:644`); it applies the `set_askpass_env` list (`:277-279`) before `ide_core::process::prepare`, which keeps variables the call site set (`crates/ide-core/src/process.rs:526-534`). Nothing in `app-gpui/src` or `crates/ide-host` calls `set_askpass_env` (only `src-tauri/src/state.rs:213`), so the list stays. It is process-wide: see H1.

### B1. Module lines

- `app-gpui/src/vcs_ops/mod.rs`, anchor `pub mod git_log;` → add after: `pub mod git_ops;`
- `app-gpui/src/vcs/mod.rs`, anchor `pub mod changes;` → add after: `pub mod clone_form;`
- `app-gpui/src/workbench/vcs_operations.rs`, anchor `/// What the window keeps for these operations.` → add before: `mod menu_ops;` and a blank line.
- `app-gpui/src/workbench/vcs_operations.rs`, `mod tests`, anchor `    mod proofs15_reload_all;` → add after: `    mod proofs49_clone;` and `    mod proofs49_git_ops;`
- `app-gpui/src/workbench/annotate.rs`, `mod tests`, anchor `    use std::path::Path;` (unique) → add after: a blank line and `    mod proofs49_ignore_commit;`
- `app-gpui/src/vcs_ops/commit_view.rs`, `mod tests`, anchor `    fn fs(rel: &str, staged: Option<ChangeKind>, unstaged: Option<ChangeKind>) -> FileStatus {` → add before: `    mod proofs49_stash_selected;` and a blank line.
- `app-gpui/src/workbench/app_actions.rs`, `mod tests`, anchor `    mod proofs20_layouts;` → add after: `    mod proofs6_app_actions;` (the existing file, unchanged; it needs B0, since its New Project runs the backend's `git init`).

### B2. The slot, the registration (`app-gpui/src/workbench/vcs_operations.rs`)

Anchor:

```rust
    /// Show Commit in Git Log waits for the log to be read before it selects the commit.
    reveal: Option<Subscription>,
```

Add after: `    menu_ops: menu_ops::MenuOpsSlot,`

Anchor:

```rust
    registry.handle_toggle(
        "vcs.toggleAmend",
```

Add before: `    menu_ops::register_actions(registry);`

### B3. Create Git Repository: `projectLocal` and the title bar's fallback

- `app-gpui/src/workbench/mod.rs` (`publish_context`), anchor `        set_context(window, cx, "projectOpen", self.project_tree().is_some());` → add after:

```rust
        // A project of this machine: this window has no remote projects, and Light Edit has no tree.
        set_context(window, cx, "projectLocal", self.project_tree().is_some());
```

- `app-gpui/src/workbench/title_bar.rs`, replace `pub const VCS_FALLBACK_ACTIONS: [&str; 3] = ["vcs.clone", "vcs.localHistory", "vcs.showLog"];` with `pub const VCS_FALLBACK_ACTIONS: [&str; 4] = ["vcs.createRepository", "vcs.clone", "vcs.localHistory", "vcs.showLog"];`

### B4. Stash Selected Files in the changes tree (`app-gpui/src/vcs_ops/commit_view.rs`, `node_menu`)

Anchor (inside `items.extend([` of `node_menu`, the first of the two `createPatch` rows is `list_menu`'s; this one is followed by `Copy as Patch`):

```rust
        Item::action_with("vcs.createPatch", json!(paths)),
        Item::item("Copy as Patch to Clipboard", ChangeCommand::CopyAsPatch(paths.clone())),
```

Replace with:

```rust
        Item::action_with("vcs.createPatch", json!(paths)),
        Item::action_with("vcs.stashSelected", json!(paths)).disabled(paths.is_empty()),
        Item::item("Copy as Patch to Clipboard", ChangeCommand::CopyAsPatch(paths.clone())),
```

### B5. The tag menu and its notification

- `app-gpui/src/workbench/log_window.rs`, replace `            BranchesPaneEvent::ShowTagMenu { tag, .. } => tracing::info!("tag {tag} has no menu here"),` with `            BranchesPaneEvent::ShowTagMenu { position, tag, repo } => self.show_tag_menu(tag, repo, position, window, cx),`
- `app-gpui/src/workbench/mod.rs` (`run_notification_action`), anchor `            EntryAction::Own(own) if own.command.starts_with("vcs.deleted.") => {` → add before:

```rust
            EntryAction::Own(own) if own.command.starts_with(crate::vcs_ops::git_ops::DELETED_TAG_PREFIX) => {
                if !self.run_tag_follow_up(&own.command, window, cx) {
                    tracing::debug!("the notification command {} has no handler any more", own.command);
                }
            }
```

### B6. The form hooks (`app-gpui/src/vcs/git_forms.rs`, `app-gpui/src/vcs/git_form_dialog.rs`)

`git_forms.rs`, anchor (the last variant of `FormAnswer`):

```rust
    NewWorktree {
        path: String,
        source: String,
        new_branch: Option<String>,
    },
}
```

Replace the closing `}` with:

```rust
    /// The Clone Repository dialog ([`crate::vcs::clone_form`]).
    Clone(crate::vcs::clone_form::CloneAnswer),
    /// Drop Stash of the Unstash dialog: stash `index`, described by `message`.
    DropStash {
        index: u32,
        message: String,
    },
}
```

`git_forms.rs`, anchor `    fn set_error(&mut self, _error: Option<String>) {}` (the trait's default) → add after:

```rust
    /// The progress of the work the answer started (`Some("")` before its first line), `None` once
    /// it ended; a form that shows none ignores it.
    fn set_progress(&mut self, _progress: Option<String>) {}
```

`git_form_dialog.rs`, anchor `    pub fn set_error(&mut self, error: Option<String>, cx: &mut Context<Self>) {` → add before:

```rust
    /// The progress of the work the answer started, shown in the dialog.
    pub fn set_progress(&mut self, progress: Option<String>, cx: &mut Context<Self>) {
        self.form.set_progress(progress);
        cx.notify();
    }

```

Every `match` on `FormAnswer` at HEAD ends in `_` or is a `let … else` (`git grep -n "FormAnswer::" HEAD -- app-gpui/src` lists `let Some(FormAnswer::…) = … else`, `update`'s `_ => Ok(())`, `GitFormEvent`'s clone): Hypothesis H4.

### B7. Drop Stash in the Unstash dialog

`git_forms.rs` (`UnstashForm::spec`), anchor:

```rust
            buttons: vec![
                FormButton::footer(OK, i18n.tx(label), true, self.chosen().is_some()),
                FormButton::cancel(i18n),
            ],
```

Replace with:

```rust
            buttons: vec![
                FormButton::toolbar("drop", i18n.tx("Drop Stash"), self.chosen().is_some()),
                FormButton::footer(OK, i18n.tx(label), true, self.chosen().is_some()),
                FormButton::cancel(i18n),
            ],
```

`git_forms.rs` (`UnstashForm::press`), anchor:

```rust
    fn press(&self, button: &str, _: &mut I18n) -> Option<FormAnswer> {
        let stash = self.chosen().filter(|_| button == OK)?;
```

Replace those two lines with:

```rust
    fn press(&self, button: &str, _: &mut I18n) -> Option<FormAnswer> {
        let stash = self.chosen()?;
        if button == "drop" {
            return Some(FormAnswer::DropStash { index: stash.index, message: stash.message.clone() });
        }
        if button != OK {
            return None;
        }
```

`vcs_operations.rs`, the import anchor `    AddToVcs, add_to_vcs, is_nested_repo_entry, rollback_candidates, rollback_plan, rollback_titles, unstash_titles,` → replace with `    AddToVcs, add_to_vcs, drop_stash, is_nested_repo_entry, rollback_candidates, rollback_plan, rollback_titles,` and add `    unstash_titles,` on the next line (rustfmt settles the layout).

`vcs_operations.rs`, replace the whole `async fn unstash(this: This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {` … `}` (HEAD lines 535-562, ending with `    run_op(&this, cx, &title, Some(success), false, work).await?;\n    Ok(())\n}` just before `async fn rollback(`) with:

```rust
/// Unstash Changes…: applies, pops or branches off the stash chosen. Drop Stash drops the chosen
/// stash once confirmed and the dialog opens again on the stashes read afresh, the next older one
/// chosen in its place.
async fn unstash(this: This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let repo = vcs_state(&this, cx, |state| state.repo.clone())?;
    let mut index = 0;
    loop {
        let path = repo_path(repo.as_deref());
        let read = call(&this, cx, move |host: Arc<Host>, label: String| async move {
            git::vcs_stashes(&host, &label, path).await
        })
        .await?;
        let stashes = read.unwrap_or_else(|error| {
            // The list stays empty: there is nothing to apply.
            tracing::warn!("the stashes cannot be listed: {error}");
            Vec::new()
        });
        let count = stashes.len();
        match ask_form(&this, cx, Box::new(UnstashForm::new(stashes, index))).await? {
            Some(FormAnswer::Unstash(options)) => {
                save_all(&this, cx).await?;
                let (title, success) = i18n(&this, cx, |i18n| unstash_titles(&options, i18n))?;
                let path = repo_path(repo.as_deref());
                let work = move |host: Arc<Host>, label: String| async move {
                    let branch = options.branch.clone();
                    git::vcs_stash_apply(&host, &label, path, options.index, options.pop, options.reinstate_index, branch)
                        .await
                };
                run_op(&this, cx, &title, Some(success), false, work).await?;
                return Ok(());
            }
            Some(FormAnswer::DropStash { index: dropped, message }) => {
                index = dropped;
                let (request, title, success) = i18n(&this, cx, |i18n| drop_stash(dropped, &message, i18n))?;
                if !confirm(&this, cx, request).await? {
                    continue;
                }
                let path = repo_path(repo.as_deref());
                let work = move |host: Arc<Host>, label: String| async move {
                    git::vcs_stash_drop(&host, &label, path, dropped).await
                };
                if run_op(&this, cx, &title, Some(success), false, work).await?.is_some() {
                    // The older stashes move up: the next one takes the dropped one's index, else the last.
                    index = dropped.min(u32::try_from(count.saturating_sub(2)).unwrap_or(u32::MAX));
                }
            }
            _ => return Ok(()),
        }
    }
}
```

### B8. Move Worktree

`git_forms.rs` (`WorktreesCommand`), anchor:

```rust
    /// Open the worktree at this path in a window of its own.
    Open(String),
```

Add before it:

```rust
    /// Move the worktree at `path` of the repository at `root` to a folder the user chooses.
    Move {
        root: String,
        path: String,
    },
```

`git_forms.rs` (`impl WorktreesForm`), anchor `    /// The selected worktree and the repository it belongs to.` → add before:

```rust
    /// Selects the row `key` (`w:<path>`), as after a move.
    pub fn select(&mut self, key: String) {
        self.selected = Some(key);
    }

```

`git_forms.rs` (`WorktreesForm::spec`), anchor `                FormButton::toolbar("open", i18n.tx("Open in New Window"), current.is_some()),` → add before:

```rust
                FormButton {
                    tooltip: Some(i18n.tx(if contains {
                        "Cannot move a worktree containing the open project."
                    } else {
                        "Move Worktree…"
                    })),
                    ..FormButton::toolbar("move", i18n.tx("Move Worktree…"), removable)
                },
```

`git_forms.rs` (`WorktreesForm::press`), anchor `            "open" => WorktreesCommand::Open(worktree_path(current?.1)),` → add before:

```rust
            "move" => {
                let (group, worktree) = current.filter(|(_, worktree)| !worktree.main)?;
                if self.contains_project() {
                    return None;
                }
                WorktreesCommand::Move { root: group.list.root.clone(), path: worktree_path(worktree) }
            }
```

`vcs_operations.rs` (`worktrees`), anchor `    let mut error = None;\n    loop {\n        let (roots, project_root, repo) =` → replace `    let mut error = None;` with `    let (mut error, mut select) = (None, None);`. Anchor `        form.set_error(error.take());` → add after:

```rust
        if let Some(key) = select.take() {
            form.select(key);
        }
```

Anchor:

```rust
                WorktreesCommand::Remove { root, path } => {
                    error = remove_worktree(&this, cx, root, path).await?;
                    break;
                }
```

Add after:

```rust
                WorktreesCommand::Move { root, path } => {
                    (error, select) = menu_ops::move_worktree(&this, cx, root, path).await?;
                    break;
                }
```

### B9. Ignore This Commit (`app-gpui/src/workbench/annotate.rs`)

Anchor `    CopyRevision(String),\n    Close,` (enum `AnnotationCommand`) → add between them: `    /// Ignore This Commit: annotate skips the commit from now on.\n    IgnoreCommit(String),`

Anchor `                MenuItem::item("Copy Revision Number", AnnotationCommand::CopyRevision(commit)).icon("copy"),` → replace with:

```rust
                MenuItem::item("Copy Revision Number", AnnotationCommand::CopyRevision(commit.clone())).icon("copy"),
                MenuItem::item("Ignore This Commit", AnnotationCommand::IgnoreCommit(commit)).icon("hide"),
```

Anchor `            AnnotationCommand::CopyRevision(commit) => cx.write_to_clipboard(ClipboardItem::new_string(commit)),` → add after:

```rust
            AnnotationCommand::IgnoreCommit(commit) => {
                let Some(editor) = editor.upgrade() else { return };
                let (id, path, label) = (editor.entity_id(), editor.read(cx).path().to_path_buf(), self.label.clone());
                let call = cx.try_global::<Backend>().map(|backend| {
                    backend.with_host(move |host| async move {
                        ide_host::ops::vcs::vcs_ignore_blame_revision(&host, &label, path, commit).await
                    })
                });
                cx.spawn(async move |this, cx| {
                    let answer = match call {
                        None => Err("The backend is not running".to_owned()),
                        Some(call) => match call.await {
                            Ok(Ok(())) => Ok(()),
                            Ok(Err(error)) => Err(error.message),
                            Err(error) => Err(error.to_string()),
                        },
                    };
                    // The window is gone: there is nothing to annotate again.
                    let _ = this.update(cx, |workbench, cx| match answer {
                        Ok(()) => workbench.schedule_reblame(id, Duration::ZERO, cx),
                        Err(message) => {
                            let mut request = vcs_notice(NotificationLevel::Error, "Cannot ignore the commit");
                            request.body = Some(message);
                            workbench.notify(request, cx);
                        }
                    });
                })
                .detach();
            }
```

And update the existing test expectation (section e).

## (c) Rows after the slice

| Row                                                                                                                        | State                                                                         | Tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| -------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:vcs.createRepository`                                                                                              | done                                                                          | `workbench::vcs_operations::tests::proofs49_git_ops::create_git_repository_initializes_the_folder_chosen_and_the_window_reads_it` (chooser answered, `.git` on disk, the notice, `vcsAvailable` and `VcsState.available` turn on, a file answer reports "Couldn't create Git repository", refused without a project). Not done: the chooser's start folder (gpui's `PathPromptOptions` has no default path).                                                                                                                          |
| `context:projectLocal`                                                                                                     | done                                                                          | same test (OFF before a project, ON with one, OFF after Close Project).                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `ipc:vcs_init`                                                                                                             | done                                                                          | same test; and `workbench::app_actions::tests::proofs6_app_actions::new_project_with_git_creates_a_repository_in_the_new_folder` once declared (B1, needs B0).                                                                                                                                                                                                                                                                                                                                                                        |
| `ipc:scaffold_create`                                                                                                      | partial (unchanged clause minus "the Git test file is not declared")          | `…proofs6_app_actions::…` added. Missing: every generator but the empty folder, the steps view.                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `action:vcs.clone`                                                                                                         | partial                                                                       | `…proofs49_clone::clone_clones_into_the_folder_chosen_and_opens_the_clone`. Missing: the progress bar (the line shows, not a bar), Cancel keeps the webview dialog open with "Clone cancelled" while the native dialog closes (Cancel always closes a `GitFormDialog`) and stops the clone, the clone sources sidebar (`cloneSourceList` contributions), fields not disabled while busy, the opened clone goes through `file.openProjectPath` (which may ask where to open when a project is open; the webview replaces the project). |
| `ipc:vcs_clone`                                                                                                            | done                                                                          | same test (a local path as URL, a missing one fails in the dialog).                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `unit:src/features/vcs/__tests__/clone.test.ts`                                                                            | done                                                                          | model cases already in `vcs_ops::clone::tests` (4) + `vcs_ops::remote_ui::tests` (1); the dialog now reaches them (window test above).                                                                                                                                                                                                                                                                                                                                                                                                |
| `ipc:vcs_delete_tag`                                                                                                       | done for the Git log's Branches pane                                          | `…proofs49_git_ops::deleting_a_tag_from_its_menu_offers_restore_which_puts_it_back`, `…a_tag_is_pushed_from_its_menu_and_deleted_on_the_remote_from_the_notification`. The Branches popup's tag submenu stays missing (needs).                                                                                                                                                                                                                                                                                                        |
| `ipc:vcs_restore_tag`                                                                                                      | done                                                                          | `…deleting_a_tag_from_its_menu_offers_restore_which_puts_it_back` (same target, annotated object back).                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `ipc:vcs_stash_drop`                                                                                                       | partial                                                                       | `…drop_stash_asks_then_drops_the_chosen_stash_and_lists_the_rest`. Missing: the Commit view's stash list (its Drop) and the Unstash dialog's Show Changes.                                                                                                                                                                                                                                                                                                                                                                            |
| `action:vcs.stashSelected`                                                                                                 | done                                                                          | `…stash_selected_files_stashes_only_the_files_given` (no selection → the notice; argument paths → only those stashed).                                                                                                                                                                                                                                                                                                                                                                                                                |
| `ipc:vcs_stash_push`                                                                                                       | done                                                                          | same test (paths passed) + `stash_then_unstash_restores_the_change`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `unit:src/features/vcs/__tests__/stashSelectedFiles.test.ts`                                                               | done (2/2)                                                                    | `vcs_ops::commit_view::tests::proofs49_stash_selected::targets_the_selected_files`, `…::is_disabled_when_no_file_is_selected`.                                                                                                                                                                                                                                                                                                                                                                                                        |
| `ipc:vcs_move_worktree`                                                                                                    | done                                                                          | `…move_worktree_moves_a_linked_worktree_into_the_folder_chosen`. Missing: the chooser's start folder.                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `ipc:vcs_ignore_blame_revision`                                                                                            | done                                                                          | `workbench::annotate::tests::proofs49_ignore_commit::ignore_this_commit_blames_its_lines_on_the_commit_before`. The webview's Annotate at Revision… and Annotate Previous Revision rows stay missing (not this slice).                                                                                                                                                                                                                                                                                                                |
| `ipc:vcs_stashes`                                                                                                          | partial (unchanged)                                                           | — needs the Commit window's stash list.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `ipc:vcs_stage_hunks`, `ipc:vcs_file_mode`, `ipc:vcs_revision_files`, `ipc:vcs_revision_children`, `ipc:vcs_compare_paths` | no entry (unchanged)                                                          | — needs, as in (a).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `module:src/workbench/TitleBar.tsx`                                                                                        | partial (unchanged clause, plus the fallback now lists Create Git Repository) | no new test asserts the fallback's rows; `vcs_fallback_menu` is data and the row list is the webview's (`TitleBar.tsx:98`).                                                                                                                                                                                                                                                                                                                                                                                                           |

Spec `clone.test.ts`: cloneDirName › derives the folder git would create → `vcs_ops::clone::tests::derives_the_folder_git_would_create`; joinPath / validateClone › joins with a single separator → `…::joins_with_a_single_separator`; › requires a URL and an absolute directory → `…::requires_a_url_and_an_absolute_directory`; parseCloneDepth › accepts positive Git depths only → `…::accepts_positive_git_depths_only`; outgoingQuery › uses the upstream range when tracked → `vcs_ops::remote_ui::tests` (existing). Spec `stashSelectedFiles.test.ts`: both cases above.

Model tests not from a spec: `vcs_ops::git_ops::tests::{lists_the_rows_of_a_tag_with_a_push_row_per_remote, the_deleted_tag_notice_offers_restore_and_deletes_on_the_remotes_it_knows, moves_a_worktree_into_the_folder_chosen_unless_it_bears_its_name, create_repository_reports_the_folder_or_the_failure}`, `vcs::clone_form::tests::{the_directory_follows_the_url_until_it_is_typed, clone_answers_only_a_valid_form_and_says_what_is_missing, while_cloning_only_cancel_can_be_pressed, remembers_the_urls_cloned_newest_first}`.

IPC commands each action ends in: `vcs.createRepository` → `vcs_init`; `vcs.clone` → `vcs_clone` (+ reads `recent_projects`' store directly through `ide_workspace::RecentProjects::load`); `vcs.stashSelected` → `vcs_stash_push`; tag menu → `vcs_delete_tag`, `vcs_restore_tag`, `vcs_remote` (`pushTag`, `deleteRemoteTag`), `vcs_checkout`, `vcs_create_branch`; Unstash › Drop Stash → `vcs_stash_drop`, `vcs_stashes`; Worktrees › Move → `vcs_move_worktree`, `vcs_worktrees`; Ignore This Commit → `vcs_ignore_blame_revision`, `vcs_blame`.

Restricted Mode: `vcs_delete_tag`, `vcs_restore_tag`, `vcs_stash_drop`, `vcs_stash_push`, `vcs_move_worktree`, `vcs_ignore_blame_revision` call `host.require_trust` (`crates/ide-host/src/ops/vcs/commands.rs:756`, `:762`, `:849`, `:812`, `:1195`, `:591`); `vcs_init` and `vcs_clone` do not, and the webview's `when` clauses (`projectOpen && projectLocal`, none) do not ask for trust either: the native app allows the same, no more.

## (d) Hypotheses, each with its check

- H1: setting the askpass list in `start_in` changes nothing for the other tests (they all set a repository-local identity and keys in `init_repo`, and under `cargo test` the list is process-wide). Check: `cargo nextest run -p openidea_gpui workbench::` and the same with `cargo test -p openidea_gpui workbench:: -- --test-threads=8`; any test that commits through the backend without a local identity would fail with "Please tell me who you are".
- H2: gpui's `prompt_for_paths` answers `Result<anyhow::Result<Option<Vec<PathBuf>>>, _>` (as `app_actions.rs:789-802` reads it); `chosen_folder` is generic over the outer error to keep that unread. Check: compile.
- H3: `git blame` as the backend runs it honours `blame.ignoreRevsFile`, which `ignore_blame_revision` writes to the local config; the line goes to Ada's commit. Check: the test `ignore_this_commit_blames_its_lines_on_the_commit_before`; or read `crates/ide-vcs/src/blame.rs` for `--ignore-revs-file`.
- H4: no exhaustive `match` on `FormAnswer` or `WorktreesCommand` exists other than `worktrees` (patched in B8). Check: `git grep -n "WorktreesCommand::\|FormAnswer::" HEAD -- app-gpui/src` and compile.
- H5: the watcher's rescan after `git init` in the project root emits `VcsChanged` (`crates/ide-host/src/project.rs:447-466`, `:916-921`), which turns `VcsState.available` on through `vcs_backend_event` (`branches.rs:1470-1479`). Check: the Create Git Repository test's second wait.
- H6: the tag row is found by the Branches pane's search (the Tags group starts closed; `filter_branch_tree` opens folders while searching). Check: the tag tests' "the tag is listed" wait.
- H7: `BranchesPaneEvent::ShowTagMenu` reaches the workbench through `log_branches_event` (the pane is subscribed in `mount_git_log`). Check: the tag tests' "the tag's menu" wait.
- H8: the stash message the backend lists is git's reflog subject (`On main: newer`); the test only checks the end of the question. Check: `crates/ide-vcs/src/refs.rs:161`.
- H9: `RemoteOp::DeleteRemoteTag` deletes `refs/tags/<tag>` on the remote with `git push <remote> --delete`. Check: `git grep -n "DeleteRemoteTag" -- crates/ide-host crates/ide-vcs`.

## (e) Existing tests whose expectations change

- `workbench::annotate::tests::the_columns_menu_copies_the_revision_and_closes_the_annotations` (`app-gpui/src/workbench/annotate.rs:574`): the committed line's menu becomes `["Show Commit in Git Log", "Copy Revision Number", "Ignore This Commit", "-", "Close Annotations"]` (webview `src/features/vcs/blame.ts:274-304`; the two Annotate at Revision rows stay out).
- `workbench::title_bar` tests read `VCS_FALLBACK_ACTIONS` by name only (`title_bar.rs:467`, `:1764`): none asserts its length; Hypothesis: none breaks (check: `git grep -n "VCS_FALLBACK_ACTIONS\|vcs_fallback_menu" HEAD -- app-gpui/src`).
- `vcs::git_forms::tests` (`git_forms.rs:2183-2198`) press only OK on the Unstash form: unchanged. The Worktrees and Unstash window tests read buttons by id: unchanged.
- No test lists every row of `node_menu` (`git grep -n "Copy as Patch to Clipboard" HEAD -- app-gpui/src` finds only builders and the shelf test).

## (f) The tests most likely to fail first

- `create_git_repository_…`: the wait for `VcsState.available` depends on the watcher's rescan (H5); if the slot stays unavailable, add `workbench.vcs_backend_event(&IdeEvent::VcsChanged { root }, cx)` after the notice in `init_chosen`.
- `clone_clones_into_the_folder_chosen_and_opens_the_clone`: `file.openProjectPath` in a window without a project may not ask about trust the way New Project's opening does; the assertion order (trust dialog, then `project_root`) may need the flow's real steps.
- `a_tag_is_pushed_…`: the second `open_tag_menu` relies on the pane keeping its query and rows after the menu closed.
- `drop_stash_…`: Escape on the confirmation must answer Cancel (it does for `PromptDialog::confirm` in the remotes test with Enter; Escape is assumed).
- Every test needs B0; without it the backend's `git init`/`git clone` read the machine's configuration (templates, default branch, hooks path).

## (g) What a look at the real window must check

- Git › Create Git Repository… (and Version Control ▾ in a project without a repository): the system folder chooser's title, the notice, the branch widget appearing.
- Get from Version Control… from the welcome screen: the Clone Repository dialog's layout (Version control select, URL, recent URLs select, Directory, Shallow + depth + "commits", Browse… at the footer's start), the progress line while cloning, a failure in red.
- The Git log's Branches pane: right button on a tag shows the tag's menu at the pointer; Delete shows "Deleted tag v1" with Restore.
- Unstash Changes…: the Drop Stash toolbar button above the list, the confirmation, the list after.
- Worktrees…: Move Worktree… between Remove and Open, disabled with its tooltip on the main worktree.
- The annotation column's menu: Ignore This Commit with the `hide` icon.
