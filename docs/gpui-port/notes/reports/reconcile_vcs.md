I've written the reconciled notes and edited five of the drafted files, but nothing has been compiled or run, so no test has passed yet. Notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/vcs_all_integration.md`

**1. Stages** (I checked the notes' anchors against the current files by script)

- **Stage 1, operations:** declare `vcs/git_forms.rs`, `vcs/git_form_dialog.rs`, `workbench/vcs_operations.rs`. 20 hunks in `vcs/mod.rs`, `prompt_dialogs.rs`, `branches.rs`, `commit.rs`, `mod.rs`. It holds the final `run_git_operation(this, cx, GitRun{op, progress, success, stopped, failure, conflicts_instead})`, makes all 16 test helpers `pub(super)` at once, and the new `init_repo`.
- **Stage 2, shelf:** declare `change_dialogs.rs`, `patch_dialogs.rs`, `shelf_view.rs`, `workbench/shelf.rs`. 45 hunks in `vcs/mod.rs`, `prompt_dialogs.rs`, `mod.rs`, `branches.rs`, `commit.rs`, `commit_box.rs`, `commit_view.rs`, `diff_window.rs`, `code_insight.rs`.
- **Stage 3, log actions:** declare `interactive_rebase_dialog.rs`, `log_message_dialog.rs`, `workbench/log_actions.rs`. 26 hunks in `vcs/mod.rs`, `mod.rs`, `branches.rs`, `conflicts.rs`, `log_window.rs`, `commit.rs`, `prompt_dialogs.rs`, `log_view.rs`.

**2. Edits to the drafted files**

- **`git_forms.rs`:** added `ResetForm::on_commit` (commit line instead of the field) and `TagForm::on_commit` (name-only layout). The webview has one Reset dialog with two layouts (`operations.tsx:329-401`) and two different Tag UIs (`:414-431` prompt, `:434-502` dialog), so one form type now covers both actions. Two model tests added.
- **`vcs_operations.rs`:** uses `GitRun`; `ask_form` and `git_log_view` are `pub(super)`; added `reload_log`; New Tag now reloads the log, as `operations.tsx:458` does.
- **`log_actions.rs`:**
  - Removed `LogOperation`/`run_log_operation` and the duplicate `git_log_view` (two definitions would not compile).
  - Reset and New Tag go through `GitFormDialog`.
  - Registered `vcs.log.createPatch` on `CreatePatchDialog`.
  - The commit menu reads `@{u}..HEAD` before it shows.
  - Tests now use the shared helpers; reset and tag tests rewritten; menu tests wait; Undo test waits for the second notice; added a Create Patch test and a pushed-commit test.
- **`shelf.rs`:** CRLF goes through `fs::read_text`; `PatchMade::Repo{repo, commits}` and `open_create_patch` are `pub(super)`; uses `conflicts::RESOLVE_CONFLICTS`.
- **Now unused, do not declare:** `vcs/log_reset_dialog.rs`, `vcs/log_tag_dialog.rs`.
- **Fields:** `vcs_operations`, `shelf`, `log_actions` were already distinct; no rename. They are now stacked after `conflicts:` in stage order.

**3. Open points**

- **a. CRLF:** the claim is true. `fs.rs:160-167` reads through `Project::text_of` (`project.rs:1191-1196`), and open documents are normalised too (`ide-text/src/lib.rs:160-162`). So the webview check (`commitChecks.tsx:396`) never fires in a project. The native check now reads the same way, and the test expects the detached-HEAD warning alone.
- **b. Git config:** only the tests' `git_in` is isolated (`mod.rs:7457-7458`); the backend's git is not (`ide-vcs/src/cli.rs:209-245`). `init_repo` now sets `core.autocrlf`, `core.hooksPath`, `commit.gpgsign`, `tag.gpgsign` and `merge.ff` in the repo, and excludes `/.openidea/` via `.git/info/exclude` instead of committing a `.gitignore`. Pull flags and `--no-autosquash` are already pinned by the backend (`lib.rs:1234`, `ops.rs:368`).
- **c. Pushed commits:** `logStore.ts:757-793` and `GitLog.tsx:1150-1214` disable the rewriting rows with no reason text (`ui/Menu.tsx:30-38`). Protected branches only affect cherry-pick's origin line (`operations.tsx:286`). I implemented the same condition and tested it against a local bare remote; the test asserts only the disabled state, since there is no reason text to check.

**4. Shortcuts** (the manifest's `when`/`keyWhen` match the source for all of these)

| Action                                    | Keys                               | Condition applied                          |
| ----------------------------------------- | ---------------------------------- | ------------------------------------------ |
| `vcs.operations`                          | Alt+\` / Ctrl+V                    | projectOpen                                |
| `vcs.toggleAmend`                         | Alt+M / Ctrl+Alt+M                 | vcsAvailable && activeToolWindow == commit |
| `vcs.update`                              | Ctrl+T / Meta+T                    | vcsAvailable                               |
| `vcs.rollback`                            | Ctrl+Alt+Z / Meta+Alt+Z            | vcsAvailable                               |
| `vcs.addToVcs`                            | Ctrl+Alt+A / Meta+Alt+A            | vcsAvailable                               |
| `vcs.changelist.move`                     | Alt+Shift+M / Meta+Shift+M         | commit window                              |
| `vcs.changelist.edit`                     | F2, Shift+F6 (Eclipse Shift+Alt+R) | commit window                              |
| `vcs.log.copyRevision`                    | Ctrl/Meta+Alt+Shift+C              | Git window or vcsLogFocus                  |
| `vcs.log.goToRef`                         | Ctrl+F / Meta+F                    | Git window or vcsLogFocus                  |
| `vcs.shelf.rename`, `vcs.log.editMessage` | Eclipse Alt+Shift+R                | vcsAvailable plus a selection check        |

- **Rollback in an editor:** the one place a shortcut does something different. With the caret on changed lines the webview runs Rollback Lines; here it opens the Rollback dialog.
- **`vcsLogFocus`:** never published natively, so those keys are stricter than in the webview.
- **Shared chord:** Eclipse Alt+Shift+R resolves the same way on both sides (`dispatcher.rs:198-200`, `keymap.ts:574-577`).

**5. For the integrator, needing a compiler**

- Check that `workbench.notifications` is reachable from `branches.rs`, and that `Control` derives `PartialEq`.
- `commit_takes_exactly_the_included_files…` expects `"Changes (2)"`; confirm the change-list grouping keeps that label.
- Stage 2 disables Reformat code, so the existing refused-commit test now clicks Analyze code instead (hunk 2.3g).
- Every existing commit test now runs Analyze code and waits up to 2 s, as the webview does. They all use `wait_long`.
- `log_actions.rs` imports `crate::agents::conversation::decode_uri_component`, and the other agent is editing `agents/**`. If it moves, `database/model.rs` has a copy.
- The log's Reset dialog lost the up/down keys between modes: `GitFormDialog` has no arrow keys for radio rows.
- I didn't read `change_dialogs.rs`, `shelf_view.rs`, `interactive_rebase_dialog.rs`, `log_message_dialog.rs` or most draft tests in full. The cross-draft checks there (duplicate action ids, duplicate methods, notification commands) were done with grep and scripts.
